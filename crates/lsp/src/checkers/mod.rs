use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::debug;

pub mod pyo3_embedded;
pub mod system_call;
pub mod types;

pub use pyo3_embedded::PyO3EmbeddedChecker;
pub use system_call::SystemCallChecker;
pub use types::*;

/// Trait for different bean-check execution strategies.
///
/// This allows the language server to support multiple ways of running
/// bean-check validation, including system calls and Python integration.
pub trait BeancountChecker: Send + Sync {
    /// Execute bean-check validation on the given journal file.
    ///
    /// # Arguments
    /// * `journal_file` - Path to the main beancount journal file to validate
    ///
    /// # Returns
    /// * `Ok(BeancountCheckResult)` - Validation results with errors and flagged entries
    /// * `Err(anyhow::Error)` - Execution error (command not found, parsing failed, etc.)
    fn check(&self, journal_file: &Path) -> Result<BeancountCheckResult>;

    /// Get a human-readable name for this checker implementation.
    /// Used for logging and debugging purposes.
    fn name(&self) -> &'static str;

    /// Check if this checker implementation is available on the current system.
    /// For example, system call checker needs bean-check binary, Python checker needs Python.
    fn is_available(&self) -> bool;
}

/// Configuration for bean-check execution method selection.
#[derive(Debug, Clone)]
pub enum BeancountCheckMethod {
    /// Use system call to execute bean-check binary (traditional approach)
    SystemCall,
    /// Use embedded Python via PyO3 to call beancount library directly (best performance)
    PythonEmbedded,
}

impl Default for BeancountCheckMethod {
    fn default() -> Self {
        // Default to system call for backward compatibility
        Self::SystemCall
    }
}

/// Configuration options for bean-check execution.
#[derive(Debug, Clone)]
pub struct BeancountCheckConfig {
    /// Which execution method to use
    pub method: BeancountCheckMethod,
    /// Path to bean-check executable (for SystemCall method)
    pub bean_check_cmd: PathBuf,
    /// Path to Python executable (for Python method)
    pub python_cmd: PathBuf,
    /// Path to the Python script (for Python method)
    pub python_script_path: PathBuf,
}

impl Default for BeancountCheckConfig {
    fn default() -> Self {
        Self {
            method: BeancountCheckMethod::default(),
            bean_check_cmd: PathBuf::from("bean-check"),
            python_cmd: PathBuf::from("python3"),
            python_script_path: PathBuf::from("python/bean_check.py"),
        }
    }
}

/// Factory function to create a checker based on configuration.
///
/// # Arguments
/// * `config` - Configuration specifying which checker to create and its options
///
/// # Returns
/// * Boxed trait object implementing BeancountChecker
pub fn create_checker(config: &BeancountCheckConfig) -> Box<dyn BeancountChecker> {
    debug!("Creating bean checker with method: {:?}", config.method);

    let checker: Box<dyn BeancountChecker> = match config.method {
        BeancountCheckMethod::SystemCall => {
            debug!(
                "Creating SystemCallChecker with command: {}",
                config.bean_check_cmd.display()
            );
            Box::new(SystemCallChecker::new(config.bean_check_cmd.clone()))
        }
        BeancountCheckMethod::PythonEmbedded => {
            debug!("Creating PyO3EmbeddedChecker");
            Box::new(PyO3EmbeddedChecker::new())
        }
    };

    debug!(
        "Created checker: {}, availability: {}",
        checker.name(),
        checker.is_available()
    );
    checker
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BeancountCheckConfig::default();
        assert!(matches!(config.method, BeancountCheckMethod::SystemCall));
        assert_eq!(config.bean_check_cmd, PathBuf::from("bean-check"));
        assert_eq!(config.python_cmd, PathBuf::from("python3"));
    }

    #[test]
    fn test_factory_system_call() {
        let config = BeancountCheckConfig::default();
        let checker = create_checker(&config);
        assert_eq!(checker.name(), "SystemCall");
    }

    #[test]
    fn test_factory_python_embedded() {
        let config = BeancountCheckConfig {
            method: BeancountCheckMethod::PythonEmbedded,
            ..Default::default()
        };
        let checker = create_checker(&config);

        #[cfg(feature = "python-embedded")]
        assert_eq!(checker.name(), "PyO3Embedded");

        #[cfg(not(feature = "python-embedded"))]
        assert_eq!(checker.name(), "PyO3Embedded (disabled)");
    }
}
