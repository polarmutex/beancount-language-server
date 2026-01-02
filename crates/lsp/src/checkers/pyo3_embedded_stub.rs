/// Stub implementation used when the `python-embedded` feature is disabled.
pub struct PyO3EmbeddedChecker;

impl PyO3EmbeddedChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PyO3EmbeddedChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::checkers::BeancountChecker for PyO3EmbeddedChecker {
    fn check(
        &self,
        _journal_file: &std::path::Path,
    ) -> anyhow::Result<crate::checkers::BeancountCheckResult> {
        Err(anyhow::anyhow!(
            "PyO3 embedded checker not available - compile with 'python-embedded' feature",
        ))
    }

    fn name(&self) -> &'static str {
        "PyO3Embedded (disabled)"
    }

    fn is_available(&self) -> bool {
        false
    }
}

#[cfg(not(feature = "python-embedded"))]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkers::BeancountChecker;

    #[test]
    fn test_pyo3_checker_creation_disabled() {
        let checker = PyO3EmbeddedChecker::new();
        assert_eq!(checker.name(), "PyO3Embedded (disabled)");
    }

    #[test]
    fn test_pyo3_checker_disabled() {
        let checker = PyO3EmbeddedChecker::new();
        assert!(!checker.is_available());
        assert_eq!(checker.name(), "PyO3Embedded (disabled)");
    }
}
