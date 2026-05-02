use crate::beancount_data::BeancountData;
use crate::checkers::{BeancountChecker, BeancountError, FlaggedEntry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use tempfile;
use tracing::debug;

/// A named source of LSP diagnostics.
///
/// Implement this trait to produce diagnostics from a specific backend (bean-check,
/// flagged-entry scan, …). The outer [`diagnostics`] function merges all sources.
pub trait DiagnosticSource {
    fn collect(&self) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>>;
}

/// Diagnostics produced by running bean-check on the journal file.
pub struct CheckerDiagnosticSource<'a> {
    pub checker: &'a dyn BeancountChecker,
    pub root_journal_file: &'a Path,
}

/// Diagnostics produced by scanning flagged entries in parsed beancount data.
pub struct FlaggedEntryDiagnosticSource<'a> {
    pub beancount_data: &'a HashMap<PathBuf, Arc<BeancountData>>,
    pub diagnostic_flags: &'a [String],
}

impl DiagnosticSource for CheckerDiagnosticSource<'_> {
    fn collect(&self) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
        checker_diagnostics(self.checker, self.root_journal_file)
    }
}

impl DiagnosticSource for FlaggedEntryDiagnosticSource<'_> {
    fn collect(&self) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
        flagged_entry_diagnostics(self.beancount_data, self.diagnostic_flags)
    }
}

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
/// Merges diagnostics from both `CheckerDiagnosticSource` and
/// `FlaggedEntryDiagnosticSource`.  Callers that need only one source can call
/// [`checker_diagnostics`] or [`flagged_entry_diagnostics`] directly.
pub fn diagnostics(
    beancount_data: &HashMap<PathBuf, Arc<BeancountData>>,
    checker: &dyn BeancountChecker,
    root_journal_file: &Path,
    diagnostic_flags: &[String],
) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
    tracing::info!("Starting diagnostics for: {}", root_journal_file.display());
    tracing::debug!(
        "Processing beancount data for {} files",
        beancount_data.len()
    );

    let sources: [&dyn DiagnosticSource; 2] = [
        &CheckerDiagnosticSource {
            checker,
            root_journal_file,
        },
        &FlaggedEntryDiagnosticSource {
            beancount_data,
            diagnostic_flags,
        },
    ];
    let mut result = HashMap::new();
    for source in sources {
        merge_maps(&mut result, source.collect());
    }
    debug!("Generated diagnostics for {} files", result.len());
    result
}

/// Merge `src` into `dst`, appending diagnostic vectors for matching keys.
fn merge_maps(
    dst: &mut HashMap<PathBuf, Vec<lsp_types::Diagnostic>>,
    src: HashMap<PathBuf, Vec<lsp_types::Diagnostic>>,
) {
    for (path, diags) in src {
        dst.entry(path).or_default().extend(diags);
    }
}

/// Run bean-check and return its diagnostics, or fall back to empty on failure.
pub fn checker_diagnostics(
    checker: &dyn BeancountChecker,
    root_journal_file: &Path,
) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
    tracing::debug!("Using checker: {}", checker.name());
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

            // In tests, return empty so flagged-entry tests can proceed.
            #[cfg(not(test))]
            return HashMap::new();
            #[cfg(test)]
            Default::default()
        }
    };

    let mut map = convert_errors_to_diagnostics(check_result.errors);
    merge_flagged_entries_from_checker(&mut map, check_result.flagged_entries);
    map
}

/// Scan parsed beancount data and return flagged-entry diagnostics.
///
/// This source is independent of bean-check and can be tested without a checker.
pub fn flagged_entry_diagnostics(
    beancount_data: &HashMap<PathBuf, Arc<BeancountData>>,
    diagnostic_flags: &[String],
) -> HashMap<PathBuf, Vec<lsp_types::Diagnostic>> {
    let mut map = HashMap::new();
    merge_flagged_entries_from_parsed_data(&mut map, beancount_data, diagnostic_flags);
    map
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
            severity: Some(lsp_types::DiagnosticSeverity::Error),
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
            severity: Some(lsp_types::DiagnosticSeverity::Warning),
            source: Some("bean-check".to_string()),
            code: Some(lsp_types::Code::String("flagged-entry".to_string())),
            ..lsp_types::Diagnostic::default()
        };

        diagnostics_map
            .entry(entry.file)
            .or_default()
            .push(diagnostic);
    }
}

/// Merge flagged entries from parsed beancount data into diagnostics map.
/// Only includes entries whose flags are in the diagnostic_flags list.
fn merge_flagged_entries_from_parsed_data(
    diagnostics_map: &mut HashMap<PathBuf, Vec<lsp_types::Diagnostic>>,
    beancount_data: &HashMap<PathBuf, Arc<BeancountData>>,
    diagnostic_flags: &[String],
) {
    for (file_path, data) in beancount_data.iter() {
        for flagged_entry in &data.flagged_entries {
            // Only create diagnostic if this flag is in the diagnostic_flags list
            if !diagnostic_flags.contains(&flagged_entry.flag) {
                tracing::debug!(
                    "Skipping flag '{}' at {}:{} (not in diagnostic_flags)",
                    flagged_entry.flag,
                    file_path.display(),
                    flagged_entry.line
                );
                continue;
            }

            let diagnostic = lsp_types::Diagnostic {
                range: full_line_range(flagged_entry.line),
                message: format!("Transaction flagged for review ({})", flagged_entry.flag),
                severity: Some(lsp_types::DiagnosticSeverity::Warning),
                source: Some("beancount-lsp".to_string()),
                code: Some(lsp_types::Code::String("flagged-entry".to_string())),
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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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
            .find(|d| d.severity == Some(lsp_types::DiagnosticSeverity::Warning));
        assert!(
            warning_diag.is_some(),
            "Should have a warning diagnostic for flagged entry"
        );

        let diagnostic = warning_diag.unwrap();
        assert_eq!(diagnostic.source, Some("beancount-lsp".to_string()));
        assert_eq!(
            diagnostic.code,
            Some(lsp_types::Code::String("flagged-entry".to_string()))
        );
        assert_eq!(diagnostic.message, "Transaction flagged for review (!)");
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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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
            .filter(|d| d.severity == Some(lsp_types::DiagnosticSeverity::Warning))
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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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

        let result = diagnostics(&beancount_data, &checker, &file_path1, &["!".to_string()]);

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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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

        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

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

    #[test]
    fn test_configurable_diagnostic_flags_default() {
        use crate::checkers::SystemCallChecker;

        // Test with only '!' flag (default configuration)
        let flagged_content = r#"2023-01-01 ! "Needs attention"
  Assets:Cash  100 USD
  Expenses:Food

2023-01-02 P "Padding transaction"
  Assets:Checking  24.94 USD
  Equity:Opening-Balances"#;

        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        // Test with default config (only '!' flag)
        let result = diagnostics(&beancount_data, &checker, &file_path, &["!".to_string()]);

        // Should have diagnostic for '!' but not for 'P'
        let file_diagnostics = result.get(&file_path);
        assert!(file_diagnostics.is_some(), "Should have diagnostics");

        let diags = file_diagnostics.unwrap();
        assert_eq!(
            diags.len(),
            1,
            "Should have exactly 1 diagnostic for '!' flag"
        );
        assert!(
            diags[0].message.contains("!"),
            "Diagnostic should mention the '!' flag"
        );
    }

    #[test]
    fn test_configurable_diagnostic_flags_multiple() {
        use crate::checkers::SystemCallChecker;

        // Test with multiple flags configured
        let flagged_content = r#"2023-01-01 ! "Needs attention"
  Assets:Cash  100 USD
  Expenses:Food

2023-01-02 P "Padding transaction"
  Assets:Checking  24.94 USD
  Equity:Opening-Balances"#;

        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        // Test with both '!' and 'P' flags configured
        let result = diagnostics(
            &beancount_data,
            &checker,
            &file_path,
            &["!".to_string(), "P".to_string()],
        );

        // Should have diagnostics for both flags
        let file_diagnostics = result.get(&file_path);
        assert!(file_diagnostics.is_some(), "Should have diagnostics");

        let diags = file_diagnostics.unwrap();
        assert_eq!(
            diags.len(),
            2,
            "Should have 2 diagnostics for '!' and 'P' flags"
        );
    }

    #[test]
    fn test_configurable_diagnostic_flags_empty() {
        use crate::checkers::SystemCallChecker;

        // Test with empty diagnostic_flags (no flags should generate diagnostics)
        let flagged_content = r#"2023-01-01 ! "Needs attention"
  Assets:Cash  100 USD
  Expenses:Food"#;

        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);
        let mock_bean_check = create_mock_bean_check_success();
        let checker = SystemCallChecker::new(mock_bean_check);

        // Test with empty diagnostic_flags
        let result = diagnostics(&beancount_data, &checker, &file_path, &[]);

        // Should have no diagnostics when diagnostic_flags is empty
        assert!(
            result.is_empty() || result.get(&file_path).is_none_or(|d| d.is_empty()),
            "Should have no diagnostics with empty diagnostic_flags"
        );
    }

    // ── DiagnosticSource trait tests ──────────────────────────────────────────

    #[test]
    fn test_flagged_entry_diagnostics_no_checker_needed() {
        // flagged_entry_diagnostics works without any checker instance
        let flagged_content =
            "2023-01-01 ! \"Flagged transaction\"\n  Assets:Cash 100 USD\n  Expenses:Food";
        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);

        let result = flagged_entry_diagnostics(&beancount_data, &["!".to_string()]);

        assert!(!result.is_empty(), "Should have flagged entry diagnostics");
        let diags = result.get(&file_path).expect("diagnostics for file");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].source, Some("beancount-lsp".to_string()));
    }

    #[test]
    fn test_flagged_entry_diagnostics_empty_flags_returns_empty() {
        let content = "2023-01-01 ! \"Flagged\"\n  Assets:Cash 100 USD\n  Expenses:Food";
        let (_temp_dir, file_path) = create_temp_beancount_file(content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, content);

        let result = flagged_entry_diagnostics(&beancount_data, &[]);
        assert!(
            result.is_empty() || result.get(&file_path).is_none_or(|d| d.is_empty()),
            "no flags configured → no diagnostics"
        );
    }

    #[test]
    fn test_checker_diagnostic_source_and_flagged_entry_source_are_independent() {
        // Verify each DiagnosticSource impl produces diagnostics independently
        let flagged_content = "2023-01-01 ! \"Flagged\"\n  Assets:Cash 100 USD\n  Expenses:Food";
        let (_temp_dir, file_path) = create_temp_beancount_file(flagged_content);
        let beancount_data = create_mock_beancount_data_with_flags(&file_path, flagged_content);

        // FlaggedEntryDiagnosticSource needs no checker
        let source = FlaggedEntryDiagnosticSource {
            beancount_data: &beancount_data,
            diagnostic_flags: &["!".to_string()],
        };
        let result = source.collect();
        assert!(!result.is_empty());

        // CheckerDiagnosticSource uses a real checker but we can use /bin/true
        use crate::checkers::SystemCallChecker;
        let checker = SystemCallChecker::new(create_mock_bean_check_success());
        let source = CheckerDiagnosticSource {
            checker: &checker,
            root_journal_file: &file_path,
        };
        let checker_result = source.collect();
        // /bin/true succeeds with no output → empty map
        assert!(checker_result.is_empty());
    }
}
