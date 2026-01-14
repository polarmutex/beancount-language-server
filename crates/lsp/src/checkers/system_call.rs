use super::BeancountChecker;
use super::types::*;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use tracing::debug;

/// Static regex for parsing bean-check error output.
/// Pattern: "file:line: error_message" with greedy path capture so embedded
/// ':' in paths (e.g., Windows drive letters or unusual paths) still match.
/// Compiled once at startup for optimal performance.
static ERROR_LINE_REGEX: OnceLock<regex::Regex> = OnceLock::new();

fn get_error_line_regex() -> &'static regex::Regex {
    ERROR_LINE_REGEX.get_or_init(|| {
        regex::Regex::new(r"^(.*):(\d+):\s*(.*)$").expect("Failed to compile error line regex")
    })
}

/// Bean-check implementation using system calls to execute the bean-check binary.
///
/// This is the traditional approach that executes bean-check as a subprocess
/// and parses its stderr output for error messages.
#[derive(Debug, Clone)]
pub struct SystemCallChecker {
    /// Path to the bean-check executable
    bean_check_cmd: PathBuf,
}

impl SystemCallChecker {
    /// Create a new system call checker with the specified bean-check command path.
    pub fn new(bean_check_cmd: PathBuf) -> Self {
        Self { bean_check_cmd }
    }

    /// Parse bean-check stderr output into structured errors.
    fn parse_stderr_output(&self, stderr: &[u8], root_journal_file: &Path) -> Vec<BeancountError> {
        let stderr_str = match std::str::from_utf8(stderr) {
            Ok(s) => s,
            Err(e) => {
                debug!("Failed to parse bean-check stderr as UTF-8: {}", e);
                return Vec::new();
            }
        };

        let mut errors = Vec::new();
        let regex = get_error_line_regex();

        for line in stderr_str.lines() {
            debug!("Processing error line: {}", line);

            // Primary parse path: regex for most outputs
            if let Some(caps) = regex.captures(line) {
                if let Some(err) =
                    Self::build_error(&caps[1], &caps[2], &caps[3], root_journal_file)
                {
                    errors.push(err);
                }
                continue;
            }

            // Fallback: split from the right to tolerate unexpected extra ':' in paths
            if let Some((file_part, line_part, msg_part)) = Self::split_fallback(line)
                && let Some(err) =
                    Self::build_error(file_part, line_part, msg_part, root_journal_file)
            {
                errors.push(err);
            }
        }

        errors
    }
}

impl SystemCallChecker {
    /// Build an error from path/line/message pieces, handling canonicalization and line parsing.
    fn build_error(
        file_part: &str,
        line_part: &str,
        msg_part: &str,
        root_journal_file: &Path,
    ) -> Option<BeancountError> {
        let line_number = match line_part.parse::<u32>() {
            Ok(num) => num,
            Err(e) => {
                debug!("Failed to parse line number '{}': {}", line_part, e);
                return None;
            }
        };

        let file_path = if line_number == 0 {
            root_journal_file.to_path_buf()
        } else {
            match PathBuf::from(file_part).canonicalize() {
                Ok(path) => path,
                Err(_) => PathBuf::from(file_part),
            }
        };

        Some(BeancountError::new(
            file_path,
            line_number,
            msg_part.trim().to_string(),
        ))
    }

    /// Fallback splitter that rsplits on ':' to allow extra colons in the path portion.
    fn split_fallback(line: &str) -> Option<(&str, &str, &str)> {
        let mut parts = line.rsplitn(3, ':');
        let msg_part = parts.next()?;
        let line_part = parts.next()?;
        let file_part = parts.next()?;
        Some((file_part, line_part, msg_part))
    }
}

impl BeancountChecker for SystemCallChecker {
    fn check(&self, journal_file: &Path) -> Result<BeancountCheckResult> {
        debug!(
            "SystemCallChecker: executing bean-check on {}",
            journal_file.display()
        );
        debug!(
            "SystemCallChecker: using command {}",
            self.bean_check_cmd.display()
        );

        let output = Command::new(&self.bean_check_cmd)
            .arg(journal_file)
            .output()
            .context(format!(
                "Failed to execute bean-check command: {}",
                self.bean_check_cmd.display()
            ))?;

        debug!(
            "SystemCallChecker: command executed, status: {}",
            output.status
        );
        debug!("SystemCallChecker: stderr length: {}", output.stderr.len());

        let errors = if !output.status.success() {
            debug!("SystemCallChecker: parsing error output");
            self.parse_stderr_output(&output.stderr, journal_file)
        } else {
            debug!("SystemCallChecker: no errors found");
            Vec::new()
        };

        debug!("SystemCallChecker: found {} errors", errors.len());

        Ok(BeancountCheckResult {
            errors,
            flagged_entries: Vec::new(), // System call checker doesn't handle flagged entries
        })
    }

    fn name(&self) -> &'static str {
        "SystemCall"
    }

    fn is_available(&self) -> bool {
        // Try to run bean-check with --help to see if it's available
        Command::new(&self.bean_check_cmd)
            .arg("--help")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to create a temporary beancount file for testing
    fn create_temp_beancount_file(content: &str) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.beancount");
        fs::write(&file_path, content).expect("Failed to write temp file");
        (temp_dir, file_path)
    }

    /// Helper to create a mock bean-check command that always succeeds
    fn create_mock_bean_check_success() -> PathBuf {
        #[cfg(unix)]
        {
            PathBuf::from("/bin/true")
        }
        #[cfg(windows)]
        {
            PathBuf::from("cmd")
        }
    }

    /// Helper to create a mock bean-check command that always fails
    fn create_mock_bean_check_failure() -> PathBuf {
        #[cfg(unix)]
        {
            PathBuf::from("/bin/false")
        }
        #[cfg(windows)]
        {
            PathBuf::from("cmd")
        }
    }

    #[test]
    fn test_system_call_checker_new() {
        let cmd = PathBuf::from("bean-check");
        let checker = SystemCallChecker::new(cmd.clone());
        assert_eq!(checker.bean_check_cmd, cmd);
        assert_eq!(checker.name(), "SystemCall");
    }

    #[test]
    fn test_system_call_checker_success() {
        let (_temp_dir, file_path) = create_temp_beancount_file("2023-01-01 open Assets:Cash");
        let checker = SystemCallChecker::new(create_mock_bean_check_success());

        let result = checker.check(&file_path);
        // Some systems might not have /bin/true, so just check it doesn't panic
        match result {
            Ok(check_result) => {
                // If successful, should have no errors (since /bin/true outputs nothing)
                assert_eq!(check_result.errors.len(), 0);
                assert_eq!(check_result.flagged_entries.len(), 0);
            }
            Err(_) => {
                // If the mock command fails, that's OK for this test environment
                // The test verifies the function handles commands gracefully
            }
        }
    }

    #[test]
    fn test_system_call_checker_failure() {
        let (_temp_dir, file_path) = create_temp_beancount_file("invalid content");
        let checker = SystemCallChecker::new(create_mock_bean_check_failure());

        let result = checker.check(&file_path);
        // Some systems might not have /bin/false, so handle both cases
        match result {
            Ok(check_result) => {
                // If command succeeds but returns failure status, should have no parsed errors
                // (since /bin/false doesn't output structured bean-check errors)
                assert_eq!(check_result.errors.len(), 0);
            }
            Err(_) => {
                // If the mock command fails to execute, that's OK for this test environment
                // The test verifies the function handles command failures gracefully
            }
        }
    }

    #[test]
    fn test_system_call_checker_invalid_command() {
        let (_temp_dir, file_path) = create_temp_beancount_file("test content");
        let checker = SystemCallChecker::new(PathBuf::from("/nonexistent/command"));

        let result = checker.check(&file_path);
        assert!(result.is_err()); // Should fail to execute
    }

    #[test]
    fn test_parse_stderr_output() {
        let checker = SystemCallChecker::new(PathBuf::from("bean-check"));
        let stderr = b"/path/to/file.beancount:123: Test error message\nanother/file.beancount:456: Another error";
        let root_file = PathBuf::from("/root/main.beancount");

        let errors = checker.parse_stderr_output(stderr, &root_file);
        assert_eq!(errors.len(), 2);

        assert_eq!(errors[0].line, 123);
        assert_eq!(errors[0].message, "Test error message");
        assert_eq!(errors[1].line, 456);
        assert_eq!(errors[1].message, "Another error");
    }

    #[test]
    fn test_parse_stderr_output_line_zero() {
        let checker = SystemCallChecker::new(PathBuf::from("bean-check"));
        let stderr = b"<check_commodity>:0: Missing Commodity directive for 'USD'";
        let root_file = PathBuf::from("/root/main.beancount");

        let errors = checker.parse_stderr_output(stderr, &root_file);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 0);
        assert_eq!(errors[0].file, root_file);
        assert_eq!(errors[0].message, "Missing Commodity directive for 'USD'");
    }

    #[cfg(windows)]
    #[test]
    fn test_parse_windows_balance_error() {
        let checker = SystemCallChecker::new(PathBuf::from("bean-check"));
        let stderr = b"C:\\Users\\TestUser\\projects\\example\\2026\\main.bean:109: Balance failed for 'Liabilities:Card': expected -13954.35 CNY != accumulated -3954.35 CNY (10000.00 too much)\r\n\r\n   2026-01-10 balance Liabilities:Card                             -13954.35 CNY\r\n";
        let root_file = PathBuf::from("C:/Users/TestUser/projects/example/2026/main.bean");

        let errors = checker.parse_stderr_output(stderr, &root_file);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 109);
        assert_eq!(
            errors[0].file,
            PathBuf::from("C:/Users/TestUser/projects/example/2026/main.bean")
        );
        assert!(
            errors[0]
                .message
                .starts_with("Balance failed for 'Liabilities:Card': expected -13954.35 CNY")
        );
    }

    #[test]
    fn test_error_line_regex() {
        let regex = get_error_line_regex();

        // Valid formats
        assert!(regex.is_match("/path/to/file.beancount:123: Error message"));
        assert!(regex.is_match("relative/path.beancount:1: Another error"));
        assert!(regex.is_match("file.beancount:0: File-level error"));
        assert!(regex.is_match("C:/path/to/file.beancount:7: Windows drive"));
        assert!(regex.is_match("C:\\path\\to\\file.beancount:9: Windows backslash"));

        // Invalid formats
        assert!(!regex.is_match("no colon separator"));
        assert!(!regex.is_match("file.beancount: missing line number"));
        assert!(!regex.is_match("file.beancount:not_a_number: invalid line"));

        // Test capture groups
        if let Some(caps) = regex.captures("/path/file.beancount:42: Test error message") {
            assert_eq!(&caps[1], "/path/file.beancount");
            assert_eq!(&caps[2], "42");
            assert_eq!(&caps[3], "Test error message");
        } else {
            panic!("Regex should match valid error format");
        }

        // Ensure backslash-separated paths are captured fully
        if let Some(caps) = regex.captures("C:\\Users\\test\\file.beancount:13: Backslash path") {
            assert_eq!(&caps[1], "C:\\Users\\test\\file.beancount");
            assert_eq!(&caps[2], "13");
            assert_eq!(&caps[3], "Backslash path");
        } else {
            panic!("Regex should match backslash-separated Windows paths");
        }
    }

    #[test]
    fn test_split_fallback_allows_extra_colon_in_path() {
        let checker = SystemCallChecker::new(PathBuf::from("bean-check"));
        let stderr = b"C:/weird:path/01.bean:12: extra colon path";
        let root_file = PathBuf::from("C:/weird:path/01.bean");

        let errors = checker.parse_stderr_output(stderr, &root_file);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 12);
        assert_eq!(errors[0].file, PathBuf::from("C:/weird:path/01.bean"));
        assert_eq!(errors[0].message, "extra colon path");
    }
}
