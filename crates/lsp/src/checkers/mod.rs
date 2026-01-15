use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use which::which;

#[cfg(feature = "python-embedded")]
mod pyo3_embedded;
#[cfg(not(feature = "python-embedded"))]
mod pyo3_embedded_stub;
mod python_checker;
pub mod system_call;
pub mod types;

#[cfg(feature = "python-embedded")]
pub use pyo3_embedded::PyO3EmbeddedChecker;
#[cfg(not(feature = "python-embedded"))]
pub use pyo3_embedded_stub::PyO3EmbeddedChecker;
pub use python_checker::SystemPythonChecker;
use python_checker::is_python_available;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeancountCheckMethod {
    /// Use system call to execute bean-check binary (traditional approach)
    SystemCall,
    /// Use embedded Python via PyO3 to call beancount library directly (best performance)
    PythonEmbedded,
    /// Use python interpreter to run embedded bean_check code
    PythonSystem,
}

/// Configuration options for bean-check execution.
#[derive(Debug, Clone)]
pub struct BeancountCheckConfig {
    /// Which execution method to use
    pub method: Option<BeancountCheckMethod>,
    /// Path to bean-check executable (for SystemCall method)
    pub bean_check_cmd: Option<PathBuf>,
    /// Path to Python executable (for Python method)
    pub python_cmd: Option<PathBuf>,
}

impl Default for BeancountCheckConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl BeancountCheckConfig {
    pub fn new() -> Self {
        Self {
            method: None,
            bean_check_cmd: None,
            python_cmd: None,
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
fn find_python_from_venv(root_dir: &Path) -> Option<PathBuf> {
    let venv_dir = root_dir.join(".venv");

    if cfg!(unix) {
        let python = venv_dir.join("bin").join("python");
        if python.is_file() {
            return Some(python);
        }
        let python3 = venv_dir.join("bin").join("python3");
        if python3.is_file() {
            return Some(python3);
        }
    }

    if cfg!(windows) {
        let python = venv_dir.join("Scripts").join("python.exe");
        if python.is_file() {
            return Some(python);
        }
        let python3 = venv_dir.join("Scripts").join("python3.exe");
        if python3.is_file() {
            return Some(python3);
        }
    }

    None
}

fn find_in_path(exe_name: &str) -> Option<PathBuf> {
    which(exe_name)
        .ok()
        .and_then(|path| path.canonicalize().ok().or(Some(path)))
}

fn resolve_python_cmd(config: &BeancountCheckConfig, root_dir: &Path) -> Option<PathBuf> {
    let user_cmd = config.python_cmd.clone();

    if let Some(cmd) = &user_cmd
        && !cmd.as_os_str().is_empty()
    {
        return Some(cmd.clone());
    }

    if let Some(venv_python) = find_python_from_venv(root_dir) {
        return Some(venv_python);
    }

    if let Some(python3) = find_in_path("python3")
        && is_python_available(&python3)
    {
        return Some(python3);
    }

    if let Some(python) = find_in_path("python")
        && is_python_available(&python)
    {
        return Some(python);
    }

    None
}

fn resolve_bean_check_cmd(config: &BeancountCheckConfig, root_dir: &Path) -> Option<PathBuf> {
    if let Some(cmd) = &config.bean_check_cmd
        && !cmd.as_os_str().is_empty()
    {
        return Some(cmd.clone());
    }

    let venv_dir = root_dir.join(".venv");
    if cfg!(unix) {
        let venv_bean_check = venv_dir.join("bin").join("bean-check");
        if venv_bean_check.is_file() {
            return Some(venv_bean_check);
        }
    }

    if cfg!(windows) {
        let venv_bean_check = venv_dir.join("Scripts").join("bean-check.exe");
        if venv_bean_check.is_file() {
            return Some(venv_bean_check);
        }
        let venv_bean_check = venv_dir.join("Scripts").join("bean-check");
        if venv_bean_check.is_file() {
            return Some(venv_bean_check);
        }
    }

    if let Some(candidate) = find_in_path("bean-check") {
        return Some(candidate);
    }

    None
}

fn create_python_checker(
    config: &BeancountCheckConfig,
    root_dir: &Path,
) -> Option<SystemPythonChecker> {
    let python_cmd = resolve_python_cmd(config, root_dir)?;
    let checker = SystemPythonChecker::new(python_cmd);
    if checker.is_available() {
        Some(checker)
    } else {
        None
    }
}

fn create_system_call_checker(config: &BeancountCheckConfig, root_dir: &Path) -> SystemCallChecker {
    let bean_check_cmd = resolve_bean_check_cmd(config, root_dir).unwrap_or_else(|| {
        config
            .bean_check_cmd
            .clone()
            .unwrap_or_else(|| PathBuf::from("bean-check"))
    });
    SystemCallChecker::new(bean_check_cmd)
}

pub fn create_checker(
    config: &BeancountCheckConfig,
    root_dir: &Path,
) -> Option<Box<dyn BeancountChecker>> {
    debug!("Creating bean checker with method: {:?}", config.method);

    let method = config.method;

    let checker: Option<Box<dyn BeancountChecker>> = match method {
        Some(BeancountCheckMethod::PythonEmbedded) => {
            let pyo3 = PyO3EmbeddedChecker::new();
            if pyo3.is_available() {
                debug!("Using PyO3EmbeddedChecker");
                Some(Box::new(pyo3))
            } else if let Some(python_checker) = create_python_checker(config, root_dir) {
                debug!("PyO3EmbeddedChecker unavailable; using SystemPythonChecker");
                Some(Box::new(python_checker))
            } else {
                let system_checker = create_system_call_checker(config, root_dir);
                if system_checker.is_available() {
                    Some(Box::new(system_checker))
                } else {
                    None
                }
            }
        }
        Some(BeancountCheckMethod::PythonSystem) => {
            if let Some(python_checker) = create_python_checker(config, root_dir) {
                debug!("Using SystemPythonChecker");
                Some(Box::new(python_checker))
            } else {
                let system_checker = create_system_call_checker(config, root_dir);
                if system_checker.is_available() {
                    Some(Box::new(system_checker))
                } else {
                    None
                }
            }
        }
        Some(BeancountCheckMethod::SystemCall) => {
            let system_checker = create_system_call_checker(config, root_dir);
            if system_checker.is_available() {
                Some(Box::new(system_checker))
            } else {
                None
            }
        }
        None => {
            let pyo3 = PyO3EmbeddedChecker::new();
            if pyo3.is_available() {
                debug!("Using PyO3EmbeddedChecker (preferred order)");
                Some(Box::new(pyo3))
            } else if let Some(python_checker) = create_python_checker(config, root_dir) {
                debug!("Using SystemPythonChecker (preferred order)");
                Some(Box::new(python_checker))
            } else {
                let system_checker = create_system_call_checker(config, root_dir);
                if system_checker.is_available() {
                    Some(Box::new(system_checker))
                } else {
                    None
                }
            }
        }
    };

    if let Some(checker) = &checker {
        info!(
            "Selected checker: {}, availability: {}",
            checker.name(),
            checker.is_available()
        );
    } else {
        info!("No checker available");
    }

    checker
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BeancountCheckConfig::new();
        assert!(config.method.is_none());
        assert!(config.bean_check_cmd.is_none());
        assert!(config.python_cmd.is_none());
    }

    #[test]
    fn test_factory_system_call() {
        let config = BeancountCheckConfig::new();
        let checker = create_checker(&config, Path::new("."));
        if let Some(checker) = checker {
            assert!(checker.name() == "SystemCall" || checker.name() == "SystemPythonChecker");
        }
    }

    #[test]
    fn test_factory_python_embedded() {
        let config = BeancountCheckConfig {
            method: Some(BeancountCheckMethod::PythonEmbedded),
            ..BeancountCheckConfig::new()
        };
        let checker = create_checker(&config, Path::new("."));

        #[cfg(feature = "python-embedded")]
        if let Some(checker) = checker {
            assert_eq!(checker.name(), "PyO3Embedded");
        }

        #[cfg(not(feature = "python-embedded"))]
        if let Some(checker) = checker {
            assert!(checker.name() == "SystemPythonChecker" || checker.name() == "SystemCall");
        }
    }
}
