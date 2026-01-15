use super::BeancountChecker;
use super::types::*;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::debug;

/// Checker that runs the bundled python/bean_check.py via `python -c`.
#[derive(Debug, Clone)]
pub struct SystemPythonChecker {
    python_cmd: PathBuf,
}

const EMBEDDED_BEAN_CHECK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../python/bean_check.py"
));

impl SystemPythonChecker {
    pub fn new(python_cmd: PathBuf) -> Self {
        Self { python_cmd }
    }

    fn python_code_for_script(&self) -> String {
        EMBEDDED_BEAN_CHECK.to_string()
    }

    fn parse_stdout(
        &self,
        stdout: &[u8],
        root_journal_file: &Path,
    ) -> (Vec<BeancountError>, Vec<FlaggedEntry>) {
        #[derive(Debug, serde::Deserialize)]
        struct PythonCheckError {
            #[serde(default)]
            file: Option<String>,
            #[serde(default)]
            line: Option<u32>,
            #[serde(default)]
            message: Option<String>,
        }

        #[derive(Debug, serde::Deserialize)]
        struct PythonFlaggedEntry {
            #[serde(default)]
            file: Option<String>,
            #[serde(default)]
            line: Option<u32>,
            #[serde(default)]
            message: Option<String>,
        }

        let stdout_str = match std::str::from_utf8(stdout) {
            Ok(s) => s,
            Err(e) => {
                debug!("Failed to parse python checker stdout as UTF-8: {}", e);
                return (Vec::new(), Vec::new());
            }
        };

        let mut lines = stdout_str.lines();
        let errors_line = lines.next().unwrap_or("[]");
        let flagged_line = lines.next().unwrap_or("[]");

        let errors_json: Vec<PythonCheckError> =
            serde_json::from_str(errors_line).unwrap_or_default();
        let flagged_json: Vec<PythonFlaggedEntry> =
            serde_json::from_str(flagged_line).unwrap_or_default();

        let errors = errors_json
            .into_iter()
            .map(|err| {
                let line_number = err.line.unwrap_or(0);
                let file_path = if line_number == 0 {
                    root_journal_file.to_path_buf()
                } else if let Some(file) = err.file {
                    match PathBuf::from(&file).canonicalize() {
                        Ok(path) => path,
                        Err(_) => PathBuf::from(file),
                    }
                } else {
                    root_journal_file.to_path_buf()
                };

                let message = err.message.unwrap_or_default();
                BeancountError::new(file_path, line_number, message)
            })
            .collect();

        let flagged_entries = flagged_json
            .into_iter()
            .map(|entry| {
                let line_number = entry.line.unwrap_or(0);
                let file_path = if line_number == 0 {
                    root_journal_file.to_path_buf()
                } else if let Some(file) = entry.file {
                    match PathBuf::from(&file).canonicalize() {
                        Ok(path) => path,
                        Err(_) => PathBuf::from(file),
                    }
                } else {
                    root_journal_file.to_path_buf()
                };

                let message = entry.message.unwrap_or_else(|| "Flagged Entry".to_string());
                FlaggedEntry::new(file_path, line_number, message)
            })
            .collect();

        (errors, flagged_entries)
    }
}

impl BeancountChecker for SystemPythonChecker {
    fn check(&self, journal_file: &Path) -> Result<BeancountCheckResult> {
        debug!(
            "SystemPythonChecker: executing python -c for {}",
            journal_file.display()
        );
        debug!(
            "SystemPythonChecker: using python {}",
            self.python_cmd.display()
        );

        let output = Command::new(&self.python_cmd)
            .arg("-c")
            .arg(self.python_code_for_script())
            .arg(journal_file)
            .output()
            .context(format!(
                "Failed to execute python checker: {}",
                self.python_cmd.display()
            ))?;

        let (errors, flagged_entries) = self.parse_stdout(&output.stdout, journal_file);

        Ok(BeancountCheckResult {
            errors,
            flagged_entries,
        })
    }

    fn name(&self) -> &'static str {
        "SystemPythonChecker"
    }

    fn is_available(&self) -> bool {
        is_python_available(&self.python_cmd)
    }
}

pub(crate) fn is_python_available(python_cmd: &Path) -> bool {
    Command::new(python_cmd)
        .arg("-c")
        .arg("import beancount, sys; sys.exit(0)")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_beancount_file(content: &str) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");
        fs::write(&file_path, content).expect("Failed to write temp file");
        (temp_dir, file_path)
    }

    #[test]
    fn test_system_python_checker_new() {
        let python_cmd = PathBuf::from("python3");
        let checker = SystemPythonChecker::new(python_cmd.clone());
        assert_eq!(checker.python_cmd, python_cmd);
        assert_eq!(checker.name(), "SystemPythonChecker");
    }

    #[test]
    fn test_python_code_for_script() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let script = checker.python_code_for_script();

        // Verify the embedded script is included
        assert!(!script.is_empty());
        assert!(script.contains("beancount"));
    }

    #[test]
    fn test_parse_stdout_valid_json() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        // Simulate Python checker output: two JSON arrays on separate lines
        let stdout = br#"[{"file": "/path/to/file.bean", "line": 10, "message": "Test error"}]
[{"file": "/path/to/flagged.bean", "line": 20, "message": "Flagged entry"}]"#;

        let (errors, flagged_entries) = checker.parse_stdout(stdout, &file_path);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].message, "Test error");

        assert_eq!(flagged_entries.len(), 1);
        assert_eq!(flagged_entries[0].line, 20);
        assert_eq!(flagged_entries[0].message, "Flagged entry");
    }

    #[test]
    fn test_parse_stdout_empty_arrays() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        let stdout = b"[]\n[]";
        let (errors, flagged_entries) = checker.parse_stdout(stdout, &file_path);

        assert_eq!(errors.len(), 0);
        assert_eq!(flagged_entries.len(), 0);
    }

    #[test]
    fn test_parse_stdout_missing_optional_fields() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        // Error with missing file field
        let stdout = br#"[{"line": 5, "message": "Error without file"}]
[]"#;

        let (errors, _) = checker.parse_stdout(stdout, &file_path);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 5);
        assert_eq!(errors[0].message, "Error without file");
    }

    #[test]
    fn test_parse_stdout_line_zero_uses_root_file() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        let stdout = br#"[{"line": 0, "message": "Root-level error"}]
[]"#;

        let (errors, _) = checker.parse_stdout(stdout, &file_path);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 0);
        assert_eq!(errors[0].file, file_path);
    }

    #[test]
    fn test_parse_stdout_invalid_utf8() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        // Invalid UTF-8 bytes
        let stdout = &[0xFF, 0xFE, 0xFD];
        let (errors, flagged_entries) = checker.parse_stdout(stdout, &file_path);

        // Should gracefully handle invalid UTF-8
        assert_eq!(errors.len(), 0);
        assert_eq!(flagged_entries.len(), 0);
    }

    #[test]
    fn test_parse_stdout_invalid_json() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        // Invalid JSON should default to empty arrays
        let stdout = b"not valid json\nstill not json";
        let (errors, flagged_entries) = checker.parse_stdout(stdout, &file_path);

        assert_eq!(errors.len(), 0);
        assert_eq!(flagged_entries.len(), 0);
    }

    #[test]
    fn test_parse_stdout_flagged_entry_default_message() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        // Flagged entry without message field should use default
        let stdout = b"[]\n[{\"line\": 30}]";
        let (_, flagged_entries) = checker.parse_stdout(stdout, &file_path);

        assert_eq!(flagged_entries.len(), 1);
        assert_eq!(flagged_entries[0].message, "Flagged Entry");
    }

    #[test]
    fn test_parse_stdout_multiple_errors() {
        let checker = SystemPythonChecker::new(PathBuf::from("python3"));
        let (_temp_dir, file_path) = create_temp_beancount_file("");

        // JSON arrays must be on single lines (first line is errors, second is flagged)
        let stdout = b"[{\"file\": \"/a.bean\", \"line\": 1, \"message\": \"Error 1\"}, {\"file\": \"/b.bean\", \"line\": 2, \"message\": \"Error 2\"}, {\"file\": \"/c.bean\", \"line\": 3, \"message\": \"Error 3\"}]\n[{\"file\": \"/d.bean\", \"line\": 10, \"message\": \"Flag 1\"}, {\"file\": \"/e.bean\", \"line\": 20, \"message\": \"Flag 2\"}]";

        let (errors, flagged_entries) = checker.parse_stdout(stdout, &file_path);

        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0].message, "Error 1");
        assert_eq!(errors[1].message, "Error 2");
        assert_eq!(errors[2].message, "Error 3");

        assert_eq!(flagged_entries.len(), 2);
        assert_eq!(flagged_entries[0].message, "Flag 1");
        assert_eq!(flagged_entries[1].message, "Flag 2");
    }

    #[test]
    fn test_is_python_available_nonexistent() {
        // Test with a command that definitely doesn't exist
        let result = is_python_available(Path::new("/nonexistent/python"));
        assert!(!result);
    }

    #[test]
    fn test_is_python_available_not_python() {
        // Test with a command that doesn't exist
        // This ensures the function properly handles invalid Python commands
        let not_python = Path::new("/this/path/does/not/exist/python");

        let result = is_python_available(not_python);
        // Should fail because the command doesn't exist
        assert!(!result);
    }

    #[test]
    #[cfg_attr(not(feature = "integration-tests"), ignore)]
    fn test_is_python_available_real_python() {
        // This test requires Python + beancount to be installed
        // Skip if not available (only run with integration-tests feature)
        let python_cmd = if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
        };

        // This may pass or fail depending on environment
        let _ = is_python_available(&python_cmd);
        // Just ensure it doesn't panic
    }

    #[test]
    #[cfg_attr(not(feature = "integration-tests"), ignore)]
    fn test_check_integration_valid_file() {
        // Integration test that requires Python + beancount
        let python_cmd = if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
        };

        let checker = SystemPythonChecker::new(python_cmd);

        // Skip if Python/beancount not available
        if !checker.is_available() {
            return;
        }

        let (_temp_dir, file_path) = create_temp_beancount_file(
            "2023-01-01 open Assets:Cash\n\
             2023-01-02 open Expenses:Food\n",
        );

        let result = checker.check(&file_path);
        assert!(result.is_ok());
        let check_result = result.unwrap();
        // Valid file should have no errors
        assert_eq!(check_result.errors.len(), 0);
    }

    #[test]
    #[cfg_attr(not(feature = "integration-tests"), ignore)]
    fn test_check_integration_with_errors() {
        let python_cmd = if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
        };

        let checker = SystemPythonChecker::new(python_cmd);

        if !checker.is_available() {
            return;
        }

        // Create a file with unbalanced transaction
        // Note: Use proper indentation (2 spaces for postings)
        let content = concat!(
            "2023-01-01 open Assets:Cash\n",
            "2023-01-01 open Expenses:Food\n",
            "\n",
            "2023-01-02 * \"Test transaction\"\n",
            "  Assets:Cash     100.00 USD\n",
            "  Expenses:Food   -50.00 USD\n",
        );
        let (_temp_dir, file_path) = create_temp_beancount_file(content);

        let result = checker.check(&file_path);
        assert!(result.is_ok());
        let check_result = result.unwrap();

        // Should detect the unbalanced transaction
        assert!(
            !check_result.errors.is_empty(),
            "Expected errors for unbalanced transaction. Got: {:?}",
            check_result.errors
        );

        // Verify error contains relevant information
        let has_transaction_error = check_result.errors.iter().any(|err| {
            let msg = err.message.to_lowercase();
            msg.contains("balance") || msg.contains("does not balance")
        });
        assert!(
            has_transaction_error,
            "Expected balance error. Got errors: {:?}",
            check_result
                .errors
                .iter()
                .map(|e| &e.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg_attr(not(feature = "integration-tests"), ignore)]
    fn test_check_integration_balance_assertion_failure() {
        let python_cmd = if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
        };

        let checker = SystemPythonChecker::new(python_cmd);

        if !checker.is_available() {
            return;
        }

        // Create a file with balance assertion that will fail
        let (_temp_dir, file_path) = create_temp_beancount_file(
            "2023-01-01 open Assets:Cash\n\
             2023-01-01 open Equity:Opening\n\
             \n\
             2023-01-02 * \"Opening balance\"\n\
               Assets:Cash  100.00 USD\n\
               Equity:Opening\n\
             \n\
             2023-01-03 balance Assets:Cash  200.00 USD\n", // Wrong balance
        );

        let result = checker.check(&file_path);
        assert!(result.is_ok());
        let check_result = result.unwrap();

        // Should detect balance assertion failure
        assert!(
            !check_result.errors.is_empty(),
            "Expected balance assertion error"
        );

        // Verify error is on the balance line (line 8)
        let balance_error = check_result
            .errors
            .iter()
            .find(|err| err.line == 8 && err.message.to_lowercase().contains("balance"));
        assert!(balance_error.is_some(), "Expected balance error on line 8");
    }

    #[test]
    #[cfg_attr(not(feature = "integration-tests"), ignore)]
    fn test_check_integration_undeclared_account() {
        let python_cmd = if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
        };

        let checker = SystemPythonChecker::new(python_cmd);

        if !checker.is_available() {
            return;
        }

        // Use an account without opening it first
        let (_temp_dir, file_path) = create_temp_beancount_file(
            "2023-01-01 * \"Test transaction\"\n\
               Assets:Cash  100 USD\n\
               Expenses:Food  -100 USD\n",
        );

        let result = checker.check(&file_path);
        assert!(result.is_ok());
        let check_result = result.unwrap();

        // Should detect undeclared accounts
        assert!(
            !check_result.errors.is_empty(),
            "Expected errors for undeclared accounts"
        );
    }
}
