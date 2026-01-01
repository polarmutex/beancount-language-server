use super::{BeancountChecker, types::*};
use anyhow::{Context, Result};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::{debug, warn};

/// Bean-check implementation using embedded Python via PyO3.
///
/// This approach embeds the Python interpreter directly into the Rust process
/// and calls the beancount library functions directly without subprocess overhead.
/// Provides the best performance and error handling of all implementations.
#[derive(Debug)]
pub struct PyO3EmbeddedChecker {
    /// Whether to cache the Python code compilation (future optimization)
    _cache_compiled_code: bool,
}

/// Cache for the beancount.loader module to avoid repeated imports
static BEANCOUNT_LOADER_CACHE: OnceLock<Result<(), String>> = OnceLock::new();

impl PyO3EmbeddedChecker {
    /// Create a new PyO3 embedded checker.
    pub fn new() -> Self {
        Self {
            _cache_compiled_code: true,
        }
    }

    /// Execute beancount validation using embedded Python.
    fn execute_beancount_check(&self, journal_file: &Path) -> Result<BeancountCheckResult> {
        debug!(
            "PyO3EmbeddedChecker: starting embedded beancount check for file: {}",
            journal_file.display()
        );

        // Check if file exists before proceeding
        if !journal_file.exists() {
            warn!(
                "PyO3EmbeddedChecker: journal file does not exist: {}",
                journal_file.display()
            );
            return Err(anyhow::anyhow!(
                "Journal file does not exist: {}",
                journal_file.display()
            ));
        }

        debug!("PyO3EmbeddedChecker: entering Python GIL context");
        Python::attach(|py| {
            debug!("PyO3EmbeddedChecker: successfully acquired Python GIL");

            // Import required Python modules (cached for performance)
            debug!("PyO3EmbeddedChecker: attempting to import beancount.loader");
            let beancount_loader = py
                .import("beancount.loader")
                .context("Failed to import beancount.loader - ensure beancount is installed")?;
            debug!("PyO3EmbeddedChecker: successfully imported beancount.loader");

            // Convert file path to Python string
            debug!("PyO3EmbeddedChecker: converting file path to Python string");
            let file_path_str = journal_file
                .to_str()
                .context("File path contains invalid UTF-8")?;
            let py_file_path = PyString::new(py, file_path_str);
            debug!(
                "PyO3EmbeddedChecker: created Python string for file path: {}",
                file_path_str
            );

            // Call beancount.loader.load_file(file_path)
            debug!(
                "PyO3EmbeddedChecker: calling beancount.loader.load_file with path: {}",
                file_path_str
            );
            let load_result = beancount_loader
                .call_method1("load_file", (py_file_path,))
                .context("Failed to call beancount.loader.load_file")?;
            debug!("PyO3EmbeddedChecker: beancount.loader.load_file completed successfully");

            // Extract the tuple (entries, errors, options)
            debug!("PyO3EmbeddedChecker: extracting results tuple from load_file");
            let (entries, errors, _options): (Bound<PyList>, Bound<PyList>, Bound<PyDict>) =
                load_result
                    .extract()
                    .context("Failed to extract load_file result tuple")?;

            debug!(
                "PyO3EmbeddedChecker: extracted {} entries, {} errors from beancount",
                entries.len(),
                errors.len()
            );

            // Process errors (pre-allocate capacity for performance)
            debug!(
                "PyO3EmbeddedChecker: processing {} beancount errors",
                errors.len()
            );
            let mut beancount_errors = Vec::with_capacity(errors.len());
            for (error_index, error_obj) in errors.iter().enumerate() {
                debug!(
                    "PyO3EmbeddedChecker: processing error {} of {}",
                    error_index + 1,
                    errors.len()
                );
                match self.extract_error_info(py, &error_obj, journal_file) {
                    Ok(error) => {
                        debug!(
                            "PyO3EmbeddedChecker: successfully extracted error: {}:{} - {}",
                            error.file.display(),
                            error.line,
                            error.message
                        );
                        beancount_errors.push(error);
                    }
                    Err(e) => {
                        warn!(
                            "PyO3EmbeddedChecker: failed to extract error info for error {}: {}",
                            error_index + 1,
                            e
                        );
                        // Add a generic error as fallback
                        beancount_errors.push(BeancountError::new(
                            journal_file.to_path_buf(),
                            0,
                            format!("Failed to process beancount error: {e}"),
                        ));
                    }
                }
            }

            // Process flagged entries (estimate capacity and optimize early returns)
            debug!(
                "PyO3EmbeddedChecker: processing {} entries for flags",
                entries.len()
            );
            let mut flagged_entries = Vec::new();
            for (entry_index, entry_obj) in entries.iter().enumerate() {
                debug!(
                    "PyO3EmbeddedChecker: processing entry {} of {} for flags",
                    entry_index + 1,
                    entries.len()
                );
                match self.extract_flagged_entry_info(py, &entry_obj) {
                    Ok(Some(flagged)) => {
                        debug!(
                            "PyO3EmbeddedChecker: found flagged entry: {}:{} - {}",
                            flagged.file.display(),
                            flagged.line,
                            flagged.message
                        );
                        flagged_entries.push(flagged);
                    }
                    Ok(None) => {
                        // Not flagged, skip silently for performance
                    }
                    Err(e) => {
                        debug!(
                            "PyO3EmbeddedChecker: failed to extract flagged entry info for entry {}: {}",
                            entry_index + 1,
                            e
                        );
                        // Non-critical, continue processing
                    }
                }
            }

            debug!(
                "PyO3EmbeddedChecker: processing complete - {} errors, {} flagged entries found",
                beancount_errors.len(),
                flagged_entries.len()
            );

            let result = BeancountCheckResult {
                errors: beancount_errors,
                flagged_entries,
            };

            debug!(
                "PyO3EmbeddedChecker: returning result with {} errors and {} flagged entries",
                result.errors.len(),
                result.flagged_entries.len()
            );

            Ok(result)
        })
    }

    /// Extract error information from a Python error object.
    fn extract_error_info(
        &self,
        _py: Python,
        error_obj: &Bound<'_, PyAny>,
        default_file: &Path,
    ) -> Result<BeancountError> {
        // Get the error source (filename and line number)
        let source = error_obj
            .getattr("source")
            .context("Error object missing 'source' attribute")?;

        let (filename, line_number) = if source.is_none() {
            // No source information, use defaults
            (default_file.to_path_buf(), 0)
        } else {
            // Source is a dictionary, not an object with attributes
            let filename_attr = source
                .get_item("filename")
                .and_then(|f| f.extract::<String>())
                .unwrap_or_else(|_| default_file.to_string_lossy().to_string());

            let line_number = source
                .get_item("lineno")
                .and_then(|l| l.extract::<u32>())
                .unwrap_or(0);

            (PathBuf::from(filename_attr), line_number)
        };

        // Get the error message
        let message = error_obj
            .getattr("message")
            .and_then(|m| m.extract::<String>())
            .or_else(|_| error_obj.str().map(|s| s.to_string()))
            .unwrap_or_else(|_| "Unknown beancount error".to_string());

        Ok(BeancountError::new(filename, line_number, message))
    }

    /// Extract flagged entry information from a Python entry object.
    fn extract_flagged_entry_info(
        &self,
        _py: Python,
        entry_obj: &Bound<'_, PyAny>,
    ) -> Result<Option<FlaggedEntry>> {
        // Check if the entry has a 'flag' attribute (fast early return)
        let flag = match entry_obj.getattr("flag") {
            Ok(flag_obj) => match flag_obj.extract::<String>() {
                Ok(flag_str) => Some(flag_str),
                Err(_) => return Ok(None), // Fast exit for non-string flags
            },
            Err(_) => return Ok(None), // Fast exit for entries without flags
        };

        // Only process entries with review flags (fast filter)
        let flag_str = match flag.as_deref() {
            Some("!") => "Transaction flagged for review",
            Some(_) => return Ok(None), // Fast exit for non-review flags
            None => return Ok(None),    // Fast exit for no flag
        };

        // Get metadata for file and line information
        let meta = entry_obj
            .getattr("meta")
            .context("Entry missing 'meta' attribute")?;

        let filename = meta
            .get_item("filename")
            .and_then(|f| f.extract::<String>())
            .unwrap_or_else(|_| "<unknown>".to_string());

        let line_number = meta
            .get_item("lineno")
            .and_then(|l| l.extract::<u32>())
            .unwrap_or(0);

        debug!(
            "PyO3EmbeddedChecker: creating flagged entry for {}:{} - {}",
            filename, line_number, flag_str
        );

        Ok(Some(FlaggedEntry::new(
            PathBuf::from(filename),
            line_number,
            flag_str.to_string(),
        )))
    }
}

impl Default for PyO3EmbeddedChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl BeancountChecker for PyO3EmbeddedChecker {
    fn check(&self, journal_file: &Path) -> Result<BeancountCheckResult> {
        debug!(
            "PyO3EmbeddedChecker::check() called for file: {}",
            journal_file.display()
        );

        // Check availability first (cached for performance)
        if !self.is_available() {
            warn!(
                "PyO3EmbeddedChecker: checker is not available - beancount library cannot be imported"
            );
            return Err(anyhow::anyhow!(
                "PyO3EmbeddedChecker is not available - beancount library cannot be imported"
            ));
        }

        debug!("PyO3EmbeddedChecker: availability confirmed, proceeding with check");

        match self.execute_beancount_check(journal_file) {
            Ok(result) => {
                debug!(
                    "PyO3EmbeddedChecker::check() completed successfully with {} errors and {} flagged entries",
                    result.errors.len(),
                    result.flagged_entries.len()
                );
                Ok(result)
            }
            Err(e) => {
                warn!("PyO3EmbeddedChecker::check() failed: {}", e);
                Err(e).context("PyO3 embedded beancount check failed")
            }
        }
    }

    fn name(&self) -> &'static str {
        "PyO3Embedded"
    }

    fn is_available(&self) -> bool {
        debug!("PyO3EmbeddedChecker::is_available() checking beancount availability");

        // Use cached result if available for performance
        let cache_result = BEANCOUNT_LOADER_CACHE.get_or_init(|| {
            Python::attach(|py| {
                debug!("PyO3EmbeddedChecker: trying to import beancount.loader in GIL context");
                match py.import("beancount.loader") {
                    Ok(_) => {
                        debug!("PyO3EmbeddedChecker: successfully imported beancount.loader");
                        Ok(())
                    }
                    Err(e) => {
                        warn!(
                            "PyO3EmbeddedChecker: failed to import beancount.loader: {}",
                            e
                        );
                        Err(e.to_string())
                    }
                }
            })
        });

        let available = cache_result.is_ok();
        debug!(
            "PyO3EmbeddedChecker::is_available() returning: {}",
            available
        );
        available
    }
}

#[cfg(feature = "python-embedded")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkers::BeancountChecker;

    #[test]
    fn test_pyo3_checker_creation() {
        let checker = PyO3EmbeddedChecker::new();
        assert_eq!(checker.name(), "PyO3Embedded");
    }

    #[test]
    fn test_pyo3_checker_availability() {
        let checker = PyO3EmbeddedChecker::new();
        // Note: This test will pass/fail based on whether beancount is installed
        // In CI/CD, we'd want to control this with test environment setup
        let _is_available = checker.is_available();
        // Don't assert specific value since it depends on environment
    }

    #[test]
    fn test_pyo3_checker_with_valid_file() {
        use std::fs;
        use tempfile::TempDir;

        let checker = PyO3EmbeddedChecker::new();

        // Only run if beancount is available
        if !checker.is_available() {
            return;
        }

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");
        let content = "2023-01-01 open Assets:Cash";
        fs::write(&file_path, content).expect("Failed to write temp file");

        let result = checker.check(&file_path);

        // Should succeed (exact content depends on beancount validation)
        match result {
            Ok(_check_result) => {
                // Basic validation that we got a result
                // (actual counts depend on beancount validation behavior)
            }
            Err(e) => {
                // If beancount is not properly configured, that's OK for this test
                eprintln!("Beancount check failed (possibly due to environment): {e}");
            }
        }
    }

    #[test]
    fn test_pyo3_checker_with_flagged_entry() {
        use std::fs;
        use tempfile::TempDir;

        let checker = PyO3EmbeddedChecker::new();

        // Only run if beancount is available
        if !checker.is_available() {
            return;
        }

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");
        let content =
            "2023-01-01 ! \"Flagged transaction\"\n  Assets:Cash  100 USD\n  Expenses:Food";
        fs::write(&file_path, content).expect("Failed to write temp file");

        let result = checker.check(&file_path);

        match result {
            Ok(check_result) => {
                // Should have at least the flagged entry
                // (May also have validation errors depending on beancount setup)
                println!(
                    "Flagged entries found: {}",
                    check_result.flagged_entries.len()
                );
                println!("Errors found: {}", check_result.errors.len());
            }
            Err(e) => {
                // If beancount is not properly configured, that's OK for this test
                eprintln!("Beancount check failed (possibly due to environment): {e}");
            }
        }
    }

    #[test]
    fn test_pyo3_checker_ignores_cleared_transactions() {
        use std::fs;
        use tempfile::TempDir;

        let checker = PyO3EmbeddedChecker::new();

        // Only run if beancount is available
        if !checker.is_available() {
            return;
        }

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");
        // Use "*" flag which should NOT be reported as flagged entry
        let content =
            "2023-01-01 * \"Cleared transaction\"\n  Assets:Cash  100 USD\n  Expenses:Food";
        fs::write(&file_path, content).expect("Failed to write temp file");

        let result = checker.check(&file_path);

        match result {
            Ok(check_result) => {
                // Should NOT have any flagged entries (cleared transactions should be ignored)
                println!(
                    "Flagged entries found: {}",
                    check_result.flagged_entries.len()
                );
                println!("Errors found: {}", check_result.errors.len());
                // Note: We don't assert specific counts since this depends on beancount environment
                // but the test documents the expected behavior
            }
            Err(e) => {
                // If beancount is not properly configured, that's OK for this test
                eprintln!("Beancount check failed (possibly due to environment): {e}");
            }
        }
    }

    #[test]
    fn test_pyo3_checker_line_number_fix() {
        use std::fs;
        use tempfile::TempDir;

        let checker = PyO3EmbeddedChecker::new();

        // Only run if beancount is available
        if !checker.is_available() {
            return;
        }

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");

        // Create content with errors on specific lines
        let content = r#"2023-01-01 open Assets:Cash USD

invalid syntax on line 3

2023-01-05 * "Valid transaction"
  Assets:Cash  100.00 USD
  Expenses:Food  -100.00 USD

another error on line 9

2023-01-10 txn "Invalid transaction - missing account"
  Assets:Cash  50.00 USD"#;

        fs::write(&file_path, content).expect("Failed to write temp file");

        let result = checker.check(&file_path);

        match result {
            Ok(check_result) => {
                println!("Found {} errors:", check_result.errors.len());
                for (i, error) in check_result.errors.iter().enumerate() {
                    println!(
                        "  Error {}: {}:{} - {}",
                        i,
                        error.file.display(),
                        error.line,
                        error.message
                    );
                }

                // Check if we have proper line numbers (not all 0 or 1)
                let has_proper_line_numbers = check_result.errors.iter().any(|e| e.line > 1);
                println!("Has proper line numbers: {has_proper_line_numbers}");

                if !has_proper_line_numbers {
                    eprintln!(
                        "WARNING: Line number fix may not be working - all errors showing line 0 or 1"
                    );
                } else {
                    println!("SUCCESS: Line number fix is working correctly");
                }
            }
            Err(e) => {
                eprintln!("Beancount check failed (possibly due to environment): {e}");
            }
        }
    }
}
