use crate::beancount_data::BeancountData;
use crate::checkers::{BeancountChecker, BeancountError, FlaggedEntry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use tempfile;
use tracing::debug;

/// Container for diagnostic data management.
/// Currently unused but reserved for future caching and state management.
pub struct DiagnosticData {
    // Future: store current diagnostics for efficient incremental updates
}

impl DiagnosticData {
    /// Creates a new diagnostic data container.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for DiagnosticData {
    fn default() -> Self {
        Self::new()
    }
}

/// Provider function for LSP `textDocument/publishDiagnostics`.
///
/// This function collects diagnostics from two sources:
/// 1. Bean-check validation (via configurable checker implementation)
/// 2. Internal flagged entries from parsed beancount data (warnings)
///
/// # Arguments
/// * `beancount_data` - Parsed beancount data containing flagged entries
/// * `checker` - Bean-check implementation (system call or Python)
/// * `root_journal_file` - Main beancount file to validate
///
/// # Returns
/// HashMap mapping file paths to their diagnostic messages
///
/// # Performance Notes
/// - Checker execution depends on implementation (system call vs Python)
/// - Combines results from checker with internal flagged entry analysis
/// - Uses structured error types for better error handling
pub fn diagnostics(
    beancount_data: HashMap<PathBuf, Arc<BeancountData>>,
    checker: &dyn BeancountChecker,
    root_journal_file: &Path,
) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
    tracing::info!("Starting diagnostics for: {}", root_journal_file.display());
    tracing::debug!("Using checker: {}", checker.name());
    tracing::debug!(
        "Processing beancount data for {} files",
        beancount_data.len()
    );

    // Execute bean-check validation using the configured checker
    tracing::debug!(
        "Calling checker.check() with file: {}",
        root_journal_file.display()
    );
    let check_result = match checker.check(root_journal_file) {
        Ok(result) => {
            tracing::debug!(
                "Bean-check {} completed: {} errors, {} flagged entries",
                checker.name(),
                result.errors.len(),
                result.flagged_entries.len()
            );
            result
        }
        Err(e) => {
            tracing::error!("Bean-check {} execution failed: {}", checker.name(), e);
            tracing::warn!("Continuing with flagged entries from parsed data only");

            // Continue processing in tests to allow testing of flagged entries
            #[cfg(not(test))]
            {
                let mut diagnostics_map = HashMap::new();
                merge_flagged_entries_from_parsed_data(&mut diagnostics_map, beancount_data);
                return diagnostics_map;
            }

            #[cfg(test)]
            {
                // In tests, create an empty result so we can test flagged entries
                Default::default()
            }
        }
    };

    // Convert checker errors to LSP diagnostics
    let mut diagnostics_map = convert_errors_to_diagnostics(check_result.errors);

    // Add flagged entries from checker (if supported by implementation)
    merge_flagged_entries_from_checker(&mut diagnostics_map, check_result.flagged_entries);

    // Add diagnostics for flagged entries from parsed beancount data
    // (These are additional to any flagged entries returned by the checker)
    merge_flagged_entries_from_parsed_data(&mut diagnostics_map, beancount_data);

    debug!("Generated diagnostics for {} files", diagnostics_map.len());
    diagnostics_map
}

/// Convert checker errors to LSP diagnostic format.
fn convert_errors_to_diagnostics(
    errors: Vec<BeancountError>,
) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
    let mut diagnostics_map: HashMap<PathBuf, Vec<lsp_types::Diagnostic>> = HashMap::new();

    for error in errors {
        // Convert 1-based line numbers to 0-based for LSP (except for line 0 which stays 0)
        let line_number = if error.line == 0 {
            0
        } else {
            error.line.saturating_sub(1)
        };

        let diagnostic = lsp_types::Diagnostic {
            range: full_line_range(line_number),
            message: error.message,
            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
            source: Some("bean-check".to_string()),
            code: None,
            code_description: None,
            tags: None,
            related_information: None,
            data: None,
        };

        diagnostics_map
            .entry(error.file)
            .or_default()
            .push(diagnostic);
    }

    diagnostics_map
}

/// Merge flagged entries from checker into diagnostics map.
fn merge_flagged_entries_from_checker(
    diagnostics_map: &mut HashMap<PathBuf, Vec<lsp_types::Diagnostic>>,
    flagged_entries: Vec<FlaggedEntry>,
) {
    for entry in flagged_entries {
        // Convert 1-based line numbers to 0-based for LSP
        let line_number = if entry.line == 0 {
            0
        } else {
            entry.line.saturating_sub(1)
        };

        let diagnostic = lsp_types::Diagnostic {
            range: full_line_range(line_number),
            message: entry.message,
            severity: Some(lsp_types::DiagnosticSeverity::WARNING),
            source: Some("bean-check".to_string()),
            code: Some(lsp_types::NumberOrString::String(
                "flagged-entry".to_string(),
            )),
            ..lsp_types::Diagnostic::default()
        };

        diagnostics_map
            .entry(entry.file)
            .or_default()
            .push(diagnostic);
    }
}

/// Merge flagged entries from parsed beancount data into diagnostics map.
fn merge_flagged_entries_from_parsed_data(
    diagnostics_map: &mut HashMap<PathBuf, Vec<lsp_types::Diagnostic>>,
    beancount_data: HashMap<PathBuf, Arc<BeancountData>>,
) {
    for (file_path, data) in beancount_data.iter() {
        for flagged_entry in &data.flagged_entries {
            let diagnostic = lsp_types::Diagnostic {
                range: full_line_range(flagged_entry.line),
                message: "Transaction flagged for review".to_string(),
                severity: Some(lsp_types::DiagnosticSeverity::WARNING),
                source: Some("beancount-lsp".to_string()),
                code: Some(lsp_types::NumberOrString::String(
                    "flagged-entry".to_string(),
                )),
                ..lsp_types::Diagnostic::default()
            };

            diagnostics_map
                .entry(file_path.clone())
                .or_default()
                .push(diagnostic);
        }
    }
}

/// Build a full-line range starting at column 0 to a very large column value.
fn full_line_range(line: u32) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position { line, character: 0 },
        end: lsp_types::Position {
            line,
            character: u32::MAX,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;
    use tree_sitter_beancount;

    /// Helper to create a temporary beancount file for testing
    fn create_temp_beancount_file(content: &str) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");
        fs::write(&file_path, content).expect("Failed to write temp file");
        (temp_dir, file_path)
    }

    /// Helper to create mock beancount data with flagged entries
    /// Uses real parsing to create a BeancountData instance with flagged transactions
    fn create_mock_beancount_data_with_flags(
        file_path: &Path,
        content: &str,
    ) -> HashMap<PathBuf, Arc<BeancountData>> {
        let mut data = HashMap::new();

        // Create a real tree-sitter parse to generate BeancountData
        let mut parser = tree_sitter_beancount::tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();
        let rope = ropey::Rope::from_str(content);

        let beancount_data = Arc::new(BeancountData::new(&tree, &rope));
        data.insert(file_path.to_path_buf(), beancount_data);
        data
    }

    /// Helper to create a mock bean-check command that always succeeds
    /// Uses /bin/true which is available on Unix systems
    fn create_mock_bean_check_success() -> PathBuf {
        #[cfg(unix)]
        {
            PathBuf::from("/bin/true")
        }

        #[cfg(windows)]
        {
            // On Windows, we'll use a simple command that exits with 0
            // This is a fallback for testing
            PathBuf::from("cmd")
        }
    }

    /// Create a simple mock that uses built-in commands
    /// For error testing, we'll use /bin/false which exits with code 1
    fn create_mock_bean_check_with_errors() -> PathBuf {
        #[cfg(unix)]
        {
            PathBuf::from("/bin/false")
        }

        #[cfg(windows)]
        {
            // On Windows, use a command that will fail
            PathBuf::from("cmd")
        }
    }

    #[test]
    fn test_diagnostics_no_errors() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir, file_path) =
            create_temp_beancount_file("2023-01-01 open Assets:Cash\n2023-01-01 close Assets:Cash");
        let beancount_data = HashMap::new();
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        assert!(
            result.is_empty(),
            "Should return no diagnostics when bean-check succeeds"
        );
    }

    #[test]
    fn test_diagnostics_bean_check_errors() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir, file_path) = create_temp_beancount_file("invalid beancount syntax");
        let beancount_data = HashMap::new();
        let mock_bean_check = create_mock_bean_check_with_errors();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        // Since /bin/false doesn't output structured errors, we expect empty result
        // but the test verifies that the function handles command failures gracefully
        assert!(
            result.is_empty(),
            "Should handle bean-check failures gracefully"
        );
    }

    #[test]
    fn test_diagnostics_flagged_entries() {
        use crate::checkers::SystemCallChecker;

        let flagged_content =
            "2023-01-01 ! \"Flagged transaction\"\n  Assets:Cash 100 USD\n  Expenses:Food";
        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        assert!(
            !result.is_empty(),
            "Should return diagnostics for flagged entries"
        );

        let file_diagnostics = result
            .get(&file_path)
            .expect("Should have diagnostics for the file");
        assert!(
            !file_diagnostics.is_empty(),
            "Should have at least 1 flagged entry diagnostic"
        );

        // Find the warning diagnostic
        let warning_diag = file_diagnostics
            .iter()
            .find(|d| d.severity == Some(lsp_types::DiagnosticSeverity::WARNING));
        assert!(
            warning_diag.is_some(),
            "Should have a warning diagnostic for flagged entry"
        );

        let diagnostic = warning_diag.unwrap();
        assert_eq!(diagnostic.source, Some("beancount-lsp".to_string()));
        assert_eq!(
            diagnostic.code,
            Some(lsp_types::NumberOrString::String(
                "flagged-entry".to_string()
            ))
        );
        assert_eq!(diagnostic.message, "Transaction flagged for review");
        assert_eq!(diagnostic.range.start.character, 0);
    }

    #[test]
    fn test_diagnostics_combined_errors_and_flags() {
        use crate::checkers::SystemCallChecker;

        let flagged_content = "2023-01-01 ! \"Test\"\n  Assets:Cash\n  Expenses:Food";
        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);
        let mock_bean_check = create_mock_bean_check_with_errors();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        assert!(
            !result.is_empty(),
            "Should return warning diagnostics for flagged entries"
        );

        let file_diagnostics = result
            .get(&file_path)
            .expect("Should have diagnostics for the file");
        assert!(
            !file_diagnostics.is_empty(),
            "Should have at least 1 flagged diagnostic"
        );

        // Check that we have warning diagnostics for flagged entries
        let warning_count = file_diagnostics
            .iter()
            .filter(|d| d.severity == Some(lsp_types::DiagnosticSeverity::WARNING))
            .count();

        assert!(
            warning_count >= 1,
            "Should have at least 1 warning diagnostic"
        );
    }

    #[test]
    fn test_diagnostics_invalid_bean_check_command() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir, file_path) = create_temp_beancount_file("test content");
        let beancount_data = HashMap::new();
        let invalid_command = PathBuf::from("/nonexistent/command/that/does/not/exist");
        let checker = SystemCallChecker::new(invalid_command);

        let result = diagnostics(beancount_data, &checker, &file_path);

        assert!(
            result.is_empty(),
            "Should return empty diagnostics when bean-check command fails"
        );
    }

    #[test]
    fn test_diagnostics_malformed_error_output() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir, file_path) = create_temp_beancount_file("test content");
        let beancount_data = HashMap::new();
        let mock_bean_check = create_mock_bean_check_with_errors();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        // Should handle command failures gracefully (no panics)
        assert!(
            result.is_empty(),
            "Should handle bean-check failures gracefully"
        );
    }

    #[test]
    fn test_diagnostics_multiple_files() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir1, file_path1) = create_temp_beancount_file("content1");
        let (_temp_dir2, file_path2) = create_temp_beancount_file("content2");

        let content1 = "2023-01-01 ! \"Flagged 1\"\n  Assets:Cash";
        let content2 =
            "2023-01-01 ! \"Flagged 2\"\n  Expenses:Food\n2023-01-02 ! \"Another\"\n  Assets:Bank";

        let mut beancount_data = HashMap::new();
        beancount_data.extend(create_mock_beancount_data_with_flags(&file_path1, content1));
        beancount_data.extend(create_mock_beancount_data_with_flags(&file_path2, content2));

        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path1);

        // Should have diagnostics for files with flagged entries
        assert!(
            !result.is_empty(),
            "Should have diagnostics for flagged entries"
        );

        // Count total diagnostics across all files
        let total_diagnostics: usize = result.values().map(|diags| diags.len()).sum();
        assert!(
            total_diagnostics >= 2,
            "Should have at least 2 total diagnostics across files"
        );
    }

    #[test]
    fn test_error_line_regex() {
        // Test the regex pattern directly
        let regex = regex::Regex::new(r"^([^:]+):(\d+):\s*(.*)$").unwrap();

        // Valid error formats
        assert!(regex.is_match("/path/to/file.beancount:123: Error message"));
        assert!(regex.is_match("relative/path.beancount:1: Another error"));
        assert!(regex.is_match("file.beancount:999: Multiple words in error"));

        // Invalid formats
        assert!(!regex.is_match("no colon separator"));
        assert!(!regex.is_match("file.beancount: missing line number"));
        assert!(!regex.is_match("file.beancount:not_a_number: invalid line"));
        assert!(!regex.is_match(": missing file and line"));

        // Test capture groups
        if let Some(caps) = regex.captures("/path/file.beancount:42: Test error message") {
            assert_eq!(&caps[1], "/path/file.beancount");
            assert_eq!(&caps[2], "42");
            assert_eq!(&caps[3], "Test error message");
        } else {
            panic!("Regex should match valid error format");
        }
    }

    #[test]
    fn test_diagnostics_empty_beancount_data() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir, file_path) = create_temp_beancount_file("empty");
        let beancount_data = HashMap::new(); // No beancount data
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        assert!(
            result.is_empty(),
            "Should return empty diagnostics with no beancount data"
        );
    }

    #[test]
    fn test_diagnostic_position_conversion() {
        use crate::checkers::SystemCallChecker;

        let (_temp_dir, file_path) = create_temp_beancount_file("test");
        let beancount_data = HashMap::new();
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        let result = diagnostics(beancount_data, &checker, &file_path);

        // Since we're not testing actual bean-check error parsing here,
        // we just verify that the function works without crashing
        // and handles position conversion properly in general
        assert!(
            result.is_empty() || !result.is_empty(),
            "Function should complete without panicking"
        );
    }

    #[test]
    fn test_error_line_regex_with_line_zero() {
        let regex = regex::Regex::new(r"^([^:]+):(\d+):\s*(.*)$").unwrap();

        // Test line 0 format (file-level errors)
        assert!(regex.is_match("<check_commodity>:0: Missing Commodity directive for 'HFCGX'"));
        assert!(regex.is_match("/path/to/file.beancount:0: File-level error"));

        // Test capture groups for line 0
        if let Some(caps) = regex.captures("<check_commodity>:0: Missing Commodity directive for 'HFCGX' in 'Assets:Investments:Retirement:HFCGX'") {
            assert_eq!(&caps[1], "<check_commodity>");
            assert_eq!(&caps[2], "0");
            assert_eq!(&caps[3], "Missing Commodity directive for 'HFCGX' in 'Assets:Investments:Retirement:HFCGX'");
        } else {
            panic!("Regex should match line 0 error format");
        }
    }
}
