//! Experimental checker backed by [rustledger](https://github.com/rustledger/rustledger).
//!
//! Runs `rledger check --format json` as a subprocess. rustledger is
//! GPL-3.0-only, so it is deliberately invoked as an external binary rather
//! than linked as a crate (this project is MIT).

use super::BeancountChecker;
use super::types::*;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::debug;

/// One entry of `rledger check --format json` output.
#[derive(Debug, Deserialize)]
struct RustledgerDiagnostic {
    file: String,
    /// 1-based line number
    line: u32,
    /// 1-based column, counted in Unicode scalar values (not bytes or UTF-16)
    #[serde(default)]
    column: Option<u32>,
    /// 1-based end line (exclusive end position)
    #[serde(default)]
    end_line: Option<u32>,
    /// 1-based end column, counted in Unicode scalar values
    #[serde(default)]
    end_column: Option<u32>,
    severity: String,
    code: String,
    message: String,
    #[serde(default)]
    hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RustledgerOutput {
    diagnostics: Vec<RustledgerDiagnostic>,
}

/// Checker that shells out to the rustledger `rledger` binary.
#[derive(Debug, Clone)]
pub struct RustledgerChecker {
    rledger_cmd: PathBuf,
}

impl RustledgerChecker {
    pub fn new(rledger_cmd: PathBuf) -> Self {
        Self { rledger_cmd }
    }

    /// Parse the JSON document printed on stdout into checker errors.
    ///
    /// Relative paths are resolved against `base_dir` (the directory rledger ran in).
    /// `read_file` supplies file contents for converting rustledger's character
    /// columns into UTF-16 offsets; if it returns `None` the error covers the whole line.
    fn parse_output(
        stdout: &[u8],
        base_dir: &Path,
        read_file: impl Fn(&Path) -> Option<String>,
    ) -> Result<Vec<BeancountError>> {
        let output: RustledgerOutput =
            serde_json::from_slice(stdout).context("Failed to parse rledger JSON output")?;

        // Each file is read at most once, even if it has many diagnostics.
        let mut lines_cache: HashMap<PathBuf, Option<Vec<String>>> = HashMap::new();

        Ok(output
            .diagnostics
            .into_iter()
            .map(|d| {
                let path = PathBuf::from(&d.file);
                let path = if path.is_absolute() {
                    path
                } else {
                    base_dir.join(path)
                };
                let file = path.canonicalize().unwrap_or(path);

                let mut message = format!("[{}] {}", d.code, d.message);
                if let Some(hint) = d.hint.filter(|h| !h.is_empty()) {
                    message.push_str("\nhint: ");
                    message.push_str(&hint);
                }

                let severity = if d.severity.eq_ignore_ascii_case("warning") {
                    BeancountSeverity::Warning
                } else {
                    BeancountSeverity::Error
                };

                let span = match (d.column, d.end_line, d.end_column) {
                    (Some(col), Some(end_line), Some(end_col)) => {
                        let lines = lines_cache.entry(file.clone()).or_insert_with(|| {
                            read_file(&file)
                                .map(|text| text.split('\n').map(String::from).collect())
                        });
                        lines.as_deref().map(|lines| ErrorSpan {
                            start_character: utf16_offset(lines, d.line, col),
                            end_line,
                            end_character: utf16_offset(lines, end_line, end_col),
                        })
                    }
                    _ => None,
                };

                let error = BeancountError::new(file, d.line, message).with_severity(severity);
                match span {
                    Some(span) => error.with_span(span),
                    None => error,
                }
            })
            .collect())
    }
}

/// Convert a 1-based character column on a 1-based line into a 0-based UTF-16 offset.
///
/// Lines past the end of the file (rustledger's exclusive end often points at
/// the start of the next line) fall back to `column - 1`.
fn utf16_offset(lines: &[String], line: u32, column: u32) -> u32 {
    let chars_before = column.saturating_sub(1) as usize;
    match line.checked_sub(1).and_then(|idx| lines.get(idx as usize)) {
        Some(text) => text
            .chars()
            .take(chars_before)
            .map(|c| c.len_utf16() as u32)
            .sum(),
        None => chars_before as u32,
    }
}

impl BeancountChecker for RustledgerChecker {
    fn check(&self, journal_file: &Path) -> Result<BeancountCheckResult> {
        let root_journal_file = journal_file
            .canonicalize()
            .unwrap_or_else(|_| journal_file.to_path_buf());
        let base_dir = root_journal_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        debug!(
            "RustledgerChecker: running {} check on {}",
            self.rledger_cmd.display(),
            root_journal_file.display()
        );

        let output = Command::new(&self.rledger_cmd)
            .current_dir(&base_dir)
            // --no-cache: don't write cache files next to the user's ledger
            .args(["check", "--format", "json", "--no-cache"])
            .arg(&root_journal_file)
            .output()
            .with_context(|| {
                format!(
                    "Failed to execute rledger command: {}",
                    self.rledger_cmd.display()
                )
            })?;

        debug!("RustledgerChecker: exit status {}", output.status);

        // rledger exits 1 when the ledger has errors but still prints the JSON
        // document; anything unparseable is a real execution failure.
        match Self::parse_output(&output.stdout, &base_dir, |path| {
            std::fs::read_to_string(path).ok()
        }) {
            Ok(errors) => {
                debug!("RustledgerChecker: found {} diagnostics", errors.len());
                Ok(BeancountCheckResult::with_errors(errors))
            }
            Err(e) => bail!(
                "rledger check failed (status {}): {e:#}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
    }

    fn name(&self) -> &'static str {
        "Rustledger"
    }

    fn diagnostic_source(&self) -> &'static str {
        "rustledger"
    }

    fn is_available(&self) -> bool {
        Command::new(&self.rledger_cmd)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "diagnostics": [
        {"file": "/ledger/main.beancount", "line": 3, "column": 1, "end_line": 6,
         "end_column": 1, "severity": "error", "phase": "validate", "code": "E1001",
         "message": "Account Expenses:Nope was never opened"},
        {"file": "sub/inc.beancount", "line": 7, "column": 1, "end_line": 7,
         "end_column": 1, "severity": "warning", "phase": "validate", "code": "W8001",
         "message": "something odd", "hint": "do the thing"}
      ],
      "error_count": 1, "warning_count": 1, "parse_error_count": 0, "validate_error_count": 1
    }"#;

    #[test]
    fn parses_json_diagnostics() {
        let errors =
            RustledgerChecker::parse_output(SAMPLE.as_bytes(), Path::new("/ledger"), |_| None)
                .unwrap();
        assert_eq!(errors.len(), 2);

        assert_eq!(errors[0].file, PathBuf::from("/ledger/main.beancount"));
        assert_eq!(errors[0].line, 3);
        assert_eq!(
            errors[0].message,
            "[E1001] Account Expenses:Nope was never opened"
        );
        assert_eq!(errors[0].severity, BeancountSeverity::Error);

        assert_eq!(errors[1].file, PathBuf::from("/ledger/sub/inc.beancount"));
        assert_eq!(
            errors[1].message,
            "[W8001] something odd\nhint: do the thing"
        );
        assert_eq!(errors[1].severity, BeancountSeverity::Warning);
    }

    #[test]
    fn empty_diagnostics() {
        let json = r#"{"diagnostics": [], "error_count": 0, "warning_count": 0,
                       "parse_error_count": 0, "validate_error_count": 0}"#;
        let errors =
            RustledgerChecker::parse_output(json.as_bytes(), Path::new("/"), |_| None).unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(RustledgerChecker::parse_output(b"not json", Path::new("/"), |_| None).is_err());
    }

    #[test]
    fn unreadable_file_has_no_span() {
        let errors =
            RustledgerChecker::parse_output(SAMPLE.as_bytes(), Path::new("/ledger"), |_| None)
                .unwrap();
        assert!(errors.iter().all(|e| e.span.is_none()));
    }

    #[test]
    fn columns_are_converted_to_utf16() {
        // "😀" is 1 char but 2 UTF-16 units; "é" is 1 char and 1 unit (2 bytes).
        let content = "2020-01-02 * \"😀é\" \"x\"\n  Assets:Bank\n";
        let json = r#"{"diagnostics": [
            {"file": "/l/main.beancount", "line": 1, "column": 17, "end_line": 2,
             "end_column": 14, "severity": "error", "code": "E1", "message": "m"},
            {"file": "/l/main.beancount", "line": 1, "column": 1, "end_line": 3,
             "end_column": 1, "severity": "error", "code": "E2", "message": "m"}
        ]}"#;
        let errors = RustledgerChecker::parse_output(json.as_bytes(), Path::new("/l"), |p| {
            assert_eq!(p, Path::new("/l/main.beancount"));
            Some(content.to_string())
        })
        .unwrap();

        // Column 17 is the closing quote after `😀é`: 16 chars before it, 17 UTF-16 units.
        assert_eq!(
            errors[0].span,
            Some(ErrorSpan {
                start_character: 17,
                end_line: 2,
                end_character: 13,
            })
        );
        // Exclusive end on the (empty) line after the last newline.
        assert_eq!(
            errors[1].span,
            Some(ErrorSpan {
                start_character: 0,
                end_line: 3,
                end_character: 0,
            })
        );
    }

    #[test]
    fn uses_rustledger_diagnostic_source() {
        let checker = RustledgerChecker::new(PathBuf::from("rledger"));
        assert_eq!(checker.diagnostic_source(), "rustledger");
    }

    #[test]
    fn missing_binary_is_unavailable() {
        let checker = RustledgerChecker::new(PathBuf::from("/nonexistent/rledger"));
        assert!(!checker.is_available());
        assert_eq!(checker.name(), "Rustledger");
    }

    #[test]
    fn missing_binary_check_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("main.beancount");
        std::fs::write(&file, "").unwrap();
        let checker = RustledgerChecker::new(PathBuf::from("/nonexistent/rledger"));
        assert!(checker.check(&file).is_err());
    }

    /// End-to-end against a real binary; set `RLEDGER_BIN` to run.
    #[test]
    #[ignore]
    fn real_rledger() {
        let Ok(bin) = std::env::var("RLEDGER_BIN") else {
            return;
        };
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("main.beancount");
        std::fs::write(
            &file,
            "2020-01-01 open Assets:Bank USD\n2020-01-02 * \"x\"\n  Assets:Bank 1 USD\n  Expenses:Nope\n",
        )
        .unwrap();
        let checker = RustledgerChecker::new(PathBuf::from(bin));
        assert!(checker.is_available());
        let result = checker.check(&file).unwrap();
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.line == 2 && e.message.contains("E1001"))
        );
    }
}
