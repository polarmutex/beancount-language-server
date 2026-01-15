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
