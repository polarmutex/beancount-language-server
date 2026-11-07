use std::path::PathBuf;

/// Result of bean-check validation containing both errors and flagged entries.
#[derive(Debug, Clone, PartialEq)]
pub struct BeancountCheckResult {
    /// Validation errors from bean-check (syntax errors, semantic errors, etc.)
    pub errors: Vec<BeancountError>,
    /// Entries marked with flags like '!' for review
    pub flagged_entries: Vec<FlaggedEntry>,
}

impl BeancountCheckResult {
    /// Create a new empty result.
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            flagged_entries: Vec::new(),
        }
    }

    /// Create a result with only errors.
    pub fn with_errors(errors: Vec<BeancountError>) -> Self {
        Self {
            errors,
            flagged_entries: Vec::new(),
        }
    }

    /// Create a result with only flagged entries.
    pub fn with_flagged_entries(flagged_entries: Vec<FlaggedEntry>) -> Self {
        Self {
            errors: Vec::new(),
            flagged_entries,
        }
    }

    /// Check if the result contains any errors or flagged entries.
    pub fn has_issues(&self) -> bool {
        !self.errors.is_empty() || !self.flagged_entries.is_empty()
    }

    /// Get total number of issues (errors + flagged entries).
    pub fn issue_count(&self) -> usize {
        self.errors.len() + self.flagged_entries.len()
    }
}

impl Default for BeancountCheckResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Severity of a checker diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BeancountSeverity {
    #[default]
    Error,
    Warning,
}

/// Precise extent of a diagnostic, already converted to LSP coordinates.
///
/// `end_line` is 1-based like [`BeancountError::line`]; character offsets are
/// 0-based UTF-16 code units, as the LSP requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorSpan {
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// Represents a validation error from bean-check.
#[derive(Debug, Clone, PartialEq)]
pub struct BeancountError {
    /// File where the error occurred
    pub file: PathBuf,
    /// Line number (1-based, 0 for file-level errors)
    pub line: u32,
    /// Error message
    pub message: String,
    /// Diagnostic severity (bean-check only reports errors; rustledger also reports warnings)
    pub severity: BeancountSeverity,
    /// Exact range, when the checker reports columns; `None` highlights the whole line
    pub span: Option<ErrorSpan>,
}

impl BeancountError {
    /// Create a new beancount error with `Error` severity.
    pub fn new(file: PathBuf, line: u32, message: String) -> Self {
        Self {
            file,
            line,
            message,
            severity: BeancountSeverity::Error,
            span: None,
        }
    }

    /// Attach an exact range to this error.
    pub fn with_span(mut self, span: ErrorSpan) -> Self {
        self.span = Some(span);
        self
    }

    /// Override the severity of this error.
    pub fn with_severity(mut self, severity: BeancountSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Check if this is a file-level error (line 0).
    pub fn is_file_level(&self) -> bool {
        self.line == 0
    }
}

/// Represents an entry flagged for review (e.g., with '!' flag).
#[derive(Debug, Clone, PartialEq)]
pub struct FlaggedEntry {
    /// File containing the flagged entry
    pub file: PathBuf,
    /// Line number (1-based)
    pub line: u32,
    /// Message describing the flag
    pub message: String,
}

impl FlaggedEntry {
    /// Create a new flagged entry.
    pub fn new(file: PathBuf, line: u32, message: String) -> Self {
        Self {
            file,
            line,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beancount_check_result_empty() {
        let result = BeancountCheckResult::new();
        assert!(!result.has_issues());
        assert_eq!(result.issue_count(), 0);
    }

    #[test]
    fn test_beancount_check_result_with_errors() {
        let errors = vec![BeancountError::new(
            PathBuf::from("test.beancount"),
            5,
            "Syntax error".to_string(),
        )];
        let result = BeancountCheckResult::with_errors(errors);
        assert!(result.has_issues());
        assert_eq!(result.issue_count(), 1);
    }

    #[test]
    fn test_beancount_check_result_with_flagged() {
        let flagged = vec![FlaggedEntry::new(
            PathBuf::from("test.beancount"),
            10,
            "Flagged for review".to_string(),
        )];
        let result = BeancountCheckResult::with_flagged_entries(flagged);
        assert!(result.has_issues());
        assert_eq!(result.issue_count(), 1);
    }

    #[test]
    fn test_beancount_error_file_level() {
        let error = BeancountError::new(
            PathBuf::from("test.beancount"),
            0,
            "File-level error".to_string(),
        );
        assert!(error.is_file_level());

        let line_error =
            BeancountError::new(PathBuf::from("test.beancount"), 5, "Line error".to_string());
        assert!(!line_error.is_file_level());
    }
}
