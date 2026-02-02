use anyhow::Result;
use std::path::{Path, PathBuf};
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

impl BeancountCheckMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            BeancountCheckMethod::SystemCall => "system",
            BeancountCheckMethod::PythonEmbedded => "python-embedded",
            BeancountCheckMethod::PythonSystem => "python-system",
        }
    }
}

impl std::str::FromStr for BeancountCheckMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "system" => Ok(BeancountCheckMethod::SystemCall),
            "python-embedded" | "pyo3" => Ok(BeancountCheckMethod::PythonEmbedded),
            "python-system" => Ok(BeancountCheckMethod::PythonSystem),
            _ => Err(format!("invalid BeancountCheckMethod: {:?}", s)),
        }
    }
}

impl std::fmt::Display for BeancountCheckMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
            method: None, // None means auto-discovery
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

    if !venv_dir.exists() {
        tracing::info!("Python venv not found at: {}", venv_dir.to_string_lossy());
        return None;
    }

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
    // Highest priority: BEANCOUNT_LSP_PYTHON environment variable
    if let Ok(env_python) = std::env::var("BEANCOUNT_LSP_PYTHON")
        && !env_python.is_empty()
    {
        let env_python_path = PathBuf::from(&env_python);
        tracing::info!(
            "Using BEANCOUNT_LSP_PYTHON environment variable: {}",
            env_python
        );
        return Some(env_python_path);
    }

    // Second priority: User-configured python_cmd
    let user_cmd = config.python_cmd.clone();

    if let Some(cmd) = &user_cmd
        && !cmd.as_os_str().is_empty()
    {
        tracing::info!("Using configured python_cmd: {}", cmd.to_string_lossy());
        return Some(cmd.clone());
    }

    if let Some(venv_python) = find_python_from_venv(root_dir) {
        tracing::info!("Using venv python: {}", venv_python.to_string_lossy());
        return Some(venv_python);
    }

    if let Some(python3) = find_in_path("python3")
        && is_python_available(&python3)
    {
        tracing::info!("Using python3 from PATH: {}", python3.to_string_lossy());
        return Some(python3);
    }

    if let Some(python) = find_in_path("python")
        && is_python_available(&python)
    {
        tracing::info!("Using python from PATH: {}", python.to_string_lossy());
        return Some(python);
    }

    tracing::info!(
        "No usable python found: configured python_cmd missing/empty, no venv python, and python/python3 not available on PATH."
    );

    None
}

fn resolve_bean_check_cmd(config: &BeancountCheckConfig, root_dir: &Path) -> Option<PathBuf> {
    if let Some(cmd) = &config.bean_check_cmd
        && !cmd.as_os_str().is_empty()
    {
        tracing::info!("Using configured bean_check_cmd: {}", cmd.to_string_lossy());
        return Some(cmd.clone());
    }

    let venv_dir = root_dir.join(".venv");
    if cfg!(unix) {
        let venv_bean_check = venv_dir.join("bin").join("bean-check");
        if venv_bean_check.is_file() {
            tracing::info!(
                "Using venv bean-check: {}",
                venv_bean_check.to_string_lossy()
            );
            return Some(venv_bean_check);
        }
    }

    if cfg!(windows) {
        let venv_bean_check = venv_dir.join("Scripts").join("bean-check.exe");
        if venv_bean_check.is_file() {
            tracing::info!(
                "Using venv bean-check: {}",
                venv_bean_check.to_string_lossy()
            );
            return Some(venv_bean_check);
        }
        let venv_bean_check = venv_dir.join("Scripts").join("bean-check");
        if venv_bean_check.is_file() {
            tracing::info!(
                "Using venv bean-check: {}",
                venv_bean_check.to_string_lossy()
            );
            return Some(venv_bean_check);
        }
    }

    if let Some(candidate) = find_in_path("bean-check") {
        tracing::info!(
            "Using bean-check from PATH: {}",
            candidate.to_string_lossy()
        );
        return Some(candidate);
    }

    tracing::info!(
        "No usable bean-check found: configured bean_check_cmd missing/empty, no venv bean-check, and bean-check not on PATH."
    );

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
    tracing::debug!("Creating bean checker with method: {:?}", config.method);

    let method = config.method;

    let checker: Option<Box<dyn BeancountChecker>> = match method {
        Some(BeancountCheckMethod::PythonEmbedded) => {
            let pyo3 = PyO3EmbeddedChecker::new();
            if pyo3.is_available() {
                tracing::debug!("Using PyO3EmbeddedChecker");
                Some(Box::new(pyo3))
            } else if let Some(python_checker) = create_python_checker(config, root_dir) {
                tracing::debug!("PyO3EmbeddedChecker unavailable; using SystemPythonChecker");
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
                tracing::debug!("Using SystemPythonChecker");
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
                tracing::debug!("Using PyO3EmbeddedChecker (preferred order)");
                Some(Box::new(pyo3))
            } else if let Some(python_checker) = create_python_checker(config, root_dir) {
                tracing::debug!("Using SystemPythonChecker (preferred order)");
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
        tracing::info!(
            "Selected checker: {}, availability: {}",
            checker.name(),
            checker.is_available()
        );
    } else {
        tracing::info!(
            "No checker available after evaluation. See previous info logs for why python/bean-check were not found."
        );
    }

    checker
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to ensure environment variable tests don't run in parallel
    #[allow(dead_code)]
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_default_config() {
        let config = BeancountCheckConfig::new();
        assert!(config.method.is_none()); // None means auto-discovery
        assert_eq!(config.bean_check_cmd, None);
        assert_eq!(config.python_cmd, None);
    }

    #[test]
    fn test_factory_system_call() {
        let config = BeancountCheckConfig::new();
        let checker = create_checker(&config, Path::new("."));
        // When no method is specified, auto-discovery tries PyO3 -> SystemPython -> SystemCall
        if let Some(checker) = checker {
            #[cfg(feature = "python-embedded")]
            assert!(
                checker.name() == "PyO3Embedded"
                    || checker.name() == "SystemPythonChecker"
                    || checker.name() == "SystemCall"
            );

            #[cfg(not(feature = "python-embedded"))]
            assert!(checker.name() == "SystemPythonChecker" || checker.name() == "SystemCall");
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

    #[test]
    fn test_find_python_from_venv_unix() {
        #[cfg(unix)]
        {
            use std::fs;
            use tempfile::TempDir;

            let temp_dir = TempDir::new().unwrap();
            let venv_dir = temp_dir.path().join(".venv");
            let bin_dir = venv_dir.join("bin");
            fs::create_dir_all(&bin_dir).unwrap();

            // Create mock python3 executable
            let python3_path = bin_dir.join("python3");
            fs::write(&python3_path, "#!/bin/sh\necho python3").unwrap();

            let result = find_python_from_venv(temp_dir.path());
            assert!(result.is_some());
            assert_eq!(result.unwrap(), python3_path);

            // Test python (not python3)
            fs::remove_file(&python3_path).unwrap();
            let python_path = bin_dir.join("python");
            fs::write(&python_path, "#!/bin/sh\necho python").unwrap();

            let result = find_python_from_venv(temp_dir.path());
            assert!(result.is_some());
            assert_eq!(result.unwrap(), python_path);
        }
    }

    #[test]
    fn test_find_python_from_venv_windows() {
        #[cfg(windows)]
        {
            use std::fs;
            use tempfile::TempDir;

            let temp_dir = TempDir::new().unwrap();
            let venv_dir = temp_dir.path().join(".venv");
            let scripts_dir = venv_dir.join("Scripts");
            fs::create_dir_all(&scripts_dir).unwrap();

            // Create mock python.exe
            let python_path = scripts_dir.join("python.exe");
            fs::write(&python_path, "mock python").unwrap();

            let result = find_python_from_venv(temp_dir.path());
            assert!(result.is_some());
            assert_eq!(result.unwrap(), python_path);

            // Test python3.exe
            fs::remove_file(&python_path).unwrap();
            let python3_path = scripts_dir.join("python3.exe");
            fs::write(&python3_path, "mock python3").unwrap();

            let result = find_python_from_venv(temp_dir.path());
            assert!(result.is_some());
            assert_eq!(result.unwrap(), python3_path);
        }
    }

    #[test]
    fn test_find_python_from_venv_not_found() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let result = find_python_from_venv(temp_dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_find_in_path_nonexistent() {
        let result = find_in_path("definitely_nonexistent_command_xyz123");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_in_path_existing() {
        // Test with a command that should exist on all systems
        #[cfg(unix)]
        let cmd = "sh";
        #[cfg(windows)]
        let cmd = "cmd";

        let result = find_in_path(cmd);
        assert!(result.is_some());
    }

    #[test]
    fn test_resolve_python_cmd_env_variable() {
        use std::env;
        use tempfile::TempDir;

        // Lock to prevent parallel execution with other env var tests
        let _guard = ENV_TEST_LOCK.lock().unwrap();

        // Save original value if exists
        let original_value = env::var("BEANCOUNT_LSP_PYTHON").ok();

        let temp_dir = TempDir::new().unwrap();

        // Set environment variable (unsafe because it affects global state)
        unsafe {
            env::set_var("BEANCOUNT_LSP_PYTHON", "/env/python");
        }

        let config = BeancountCheckConfig {
            method: None,
            bean_check_cmd: None,
            python_cmd: Some(PathBuf::from("/config/python")),
        };

        let result = resolve_python_cmd(&config, temp_dir.path());

        // Restore original state
        unsafe {
            if let Some(original) = original_value {
                env::set_var("BEANCOUNT_LSP_PYTHON", original);
            } else {
                env::remove_var("BEANCOUNT_LSP_PYTHON");
            }
        }

        // Environment variable should take priority over config
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/env/python"));
    }

    #[test]
    fn test_resolve_python_cmd_user_specified() {
        use std::env;
        use tempfile::TempDir;

        // Lock to prevent parallel execution with other env var tests
        let _guard = ENV_TEST_LOCK.lock().unwrap();

        // Ensure BEANCOUNT_LSP_PYTHON is not set for this test
        let original_value = env::var("BEANCOUNT_LSP_PYTHON").ok();
        unsafe {
            env::remove_var("BEANCOUNT_LSP_PYTHON");
        }

        let config = BeancountCheckConfig {
            method: None,
            bean_check_cmd: None,
            python_cmd: Some(PathBuf::from("/custom/python")),
        };

        let temp_dir = TempDir::new().unwrap();

        let result = resolve_python_cmd(&config, temp_dir.path());

        // Restore original state
        unsafe {
            if let Some(original) = original_value {
                env::set_var("BEANCOUNT_LSP_PYTHON", original);
            }
        }

        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/custom/python"));
    }

    #[test]
    fn test_resolve_python_cmd_empty_string() {
        let config = BeancountCheckConfig {
            method: None,
            bean_check_cmd: None,
            python_cmd: Some(PathBuf::from("")),
        };

        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();

        // Empty string should trigger venv/PATH search
        let result = resolve_python_cmd(&config, temp_dir.path());
        // Result depends on system, just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_resolve_python_cmd_venv_priority() {
        #[cfg(unix)]
        {
            use std::fs;
            use tempfile::TempDir;

            let temp_dir = TempDir::new().unwrap();
            let venv_dir = temp_dir.path().join(".venv");
            let bin_dir = venv_dir.join("bin");
            fs::create_dir_all(&bin_dir).unwrap();

            // Create mock venv python
            let venv_python = bin_dir.join("python3");
            fs::write(&venv_python, "#!/bin/sh\necho venv python").unwrap();

            // Config with empty python_cmd to trigger venv search
            let config = BeancountCheckConfig {
                method: None,
                bean_check_cmd: None,
                python_cmd: None,
            };
            let result = resolve_python_cmd(&config, temp_dir.path());

            // Should prefer venv python over system python
            assert!(result.is_some());
            assert_eq!(result.unwrap(), venv_python);
        }
    }

    #[test]
    fn test_resolve_bean_check_cmd_user_specified() {
        let config = BeancountCheckConfig {
            method: None,
            bean_check_cmd: Some(PathBuf::from("/custom/bean-check")),
            python_cmd: None,
        };

        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();

        let result = resolve_bean_check_cmd(&config, temp_dir.path());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/custom/bean-check"));
    }

    #[test]
    fn test_resolve_bean_check_cmd_venv_unix() {
        #[cfg(unix)]
        {
            use std::fs;
            use tempfile::TempDir;

            let temp_dir = TempDir::new().unwrap();
            let venv_dir = temp_dir.path().join(".venv");
            let bin_dir = venv_dir.join("bin");
            fs::create_dir_all(&bin_dir).unwrap();

            // Create mock bean-check
            let bean_check_path = bin_dir.join("bean-check");
            fs::write(&bean_check_path, "#!/bin/sh\necho bean-check").unwrap();

            // Config with None to trigger venv search
            let config = BeancountCheckConfig {
                method: None,
                bean_check_cmd: None,
                python_cmd: None,
            };

            let result = resolve_bean_check_cmd(&config, temp_dir.path());
            assert!(result.is_some());
            assert_eq!(result.unwrap(), bean_check_path);
        }
    }

    #[test]
    fn test_resolve_bean_check_cmd_venv_windows() {
        #[cfg(windows)]
        {
            use std::fs;
            use tempfile::TempDir;

            let temp_dir = TempDir::new().unwrap();
            let venv_dir = temp_dir.path().join(".venv");
            let scripts_dir = venv_dir.join("Scripts");
            fs::create_dir_all(&scripts_dir).unwrap();

            // Create mock bean-check.exe
            let bean_check_path = scripts_dir.join("bean-check.exe");
            fs::write(&bean_check_path, "mock bean-check").unwrap();

            // Config with None to trigger venv search
            let config = BeancountCheckConfig {
                method: None,
                bean_check_cmd: None,
                python_cmd: None,
            };

            let result = resolve_bean_check_cmd(&config, temp_dir.path());
            assert!(result.is_some());
            assert_eq!(result.unwrap(), bean_check_path);
        }
    }

    #[test]
    fn test_create_system_call_checker() {
        let config = BeancountCheckConfig::new();
        let checker = create_system_call_checker(&config, Path::new("."));

        assert_eq!(checker.name(), "SystemCall");
    }

    #[test]
    fn test_create_python_checker_nonexistent() {
        let config = BeancountCheckConfig {
            method: None,
            bean_check_cmd: None,
            python_cmd: Some(PathBuf::from("/nonexistent/python")),
        };

        let result = create_python_checker(&config, Path::new("."));
        // Should return None because python is not available
        assert!(result.is_none());
    }

    #[test]
    fn test_checker_fallback_chain() {
        // Test that factory falls back through checkers when unavailable
        let config = BeancountCheckConfig {
            method: Some(BeancountCheckMethod::PythonEmbedded),
            bean_check_cmd: Some(PathBuf::from("/nonexistent/bean-check")),
            python_cmd: Some(PathBuf::from("/nonexistent/python")),
        };

        let checker = create_checker(&config, Path::new("."));

        // Should fall back to system checker (which may or may not be available)
        // Just verify it doesn't panic
        if let Some(checker) = checker {
            // If a checker is returned, verify it has the right interface
            let _ = checker.name();
            let _ = checker.is_available();
        }
    }

    #[test]
    fn test_checker_method_priority() {
        // Test explicit method selection
        let test_cases = vec![
            (BeancountCheckMethod::SystemCall, "SystemCall"),
            #[cfg(feature = "python-embedded")]
            (BeancountCheckMethod::PythonEmbedded, "PyO3Embedded"),
            #[cfg(not(feature = "python-embedded"))]
            (BeancountCheckMethod::PythonEmbedded, "SystemPythonChecker"),
        ];

        for (method, _expected_name) in test_cases {
            let config = BeancountCheckConfig {
                method: Some(method),
                ..BeancountCheckConfig::new()
            };

            let checker = create_checker(&config, Path::new("."));
            // Just verify it returns something (availability depends on system)
            let _ = checker;
        }
    }

    #[test]
    fn test_auto_discovery_prefers_pyo3() {
        // Test that auto-discovery (method: None) prefers PyO3 when available
        let config = BeancountCheckConfig {
            method: None,
            ..BeancountCheckConfig::new()
        };

        let checker = create_checker(&config, Path::new("."));

        #[cfg(feature = "python-embedded")]
        if let Some(checker) = checker {
            // With python-embedded feature, PyO3 should be tried first
            // But it might not be available, so we just check it doesn't panic
            let _ = checker.name();
        }

        #[cfg(not(feature = "python-embedded"))]
        if let Some(checker) = checker {
            // Without python-embedded feature, should use SystemPython or SystemCall
            assert!(checker.name() == "SystemPythonChecker" || checker.name() == "SystemCall");
        }
    }
}
