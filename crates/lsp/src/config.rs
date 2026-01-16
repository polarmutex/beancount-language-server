use crate::checkers::{BeancountCheckConfig, BeancountCheckMethod};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    /// root directory of current workspace
    pub root_dir: PathBuf,
    /// path to root journal file
    pub journal_root: Option<PathBuf>,
    pub formatting: FormattingConfig,
    pub bean_check: BeancountCheckConfig,
}

#[derive(Debug, Clone)]
pub struct FormattingConfig {
    /// Use this prefix width instead of determining an optimal value automatically.
    /// Corresponds to bean-format's --prefix-width (-w) option.
    pub prefix_width: Option<usize>,

    /// Use this width to render numbers instead of determining an optimal value.
    /// Corresponds to bean-format's --num-width (-W) option.
    pub num_width: Option<usize>,

    /// Align currencies in this column.
    /// Corresponds to bean-format's --currency-column (-c) option.
    pub currency_column: Option<usize>,

    /// Spacing between account names and amounts (default: 2).
    /// This is the minimum number of spaces between the account and the amount.
    pub account_amount_spacing: usize,

    /// Number of spaces between the number and currency (default: 1).
    /// Controls whitespace like "100.00 USD" vs "100.00  USD".
    pub number_currency_spacing: usize,

    /// Enforce consistent indentation width for postings and directives.
    /// If specified, all indentation will be normalized to this number of spaces.
    /// If None, indentation is left unchanged.
    pub indent_width: Option<usize>,
}

impl FormattingConfig {
    pub fn default() -> Self {
        Self {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,  // Default spacing like bean-format
            number_currency_spacing: 1, // Default 1 space between number and currency
            indent_width: None,         // Default: no indent normalization
        }
    }
}

impl Config {
    pub fn new(root_dir: PathBuf) -> Self {
        Self {
            root_dir,
            journal_root: None,
            formatting: FormattingConfig::default(),
            bean_check: BeancountCheckConfig::new(),
        }
    }
    pub fn update(&mut self, json: serde_json::Value) -> Result<()> {
        let result = serde_json::from_value::<BeancountLspOptions>(json.clone());

        let beancount_lsp_settings = match result {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(
                    "Failed to parse BeancountLspOptions from initialization options: {:?}; errors: {}",
                    json,
                    err,
                );
                return Ok(());
            }
        };

        // Ignore non-BeancountLspOptions inputs here.
        // Example: "[]" is sent by nvim-lspconfig if no initialization options are specified in
        // Lua.
        // Only set journal_root if journal_file is present and non-empty
        if let Some(journal_file) = beancount_lsp_settings.journal_file {
            if !journal_file.trim().is_empty() {
                self.journal_root = Some(PathBuf::from(shellexpand::tilde(&journal_file).as_ref()));
            } else {
                tracing::info!("Journal file is empty string, treating as None");
            }
        }

        // Update formatting configuration
        if let Some(formatting) = beancount_lsp_settings.formatting {
            if let Some(prefix_width) = formatting.prefix_width {
                self.formatting.prefix_width = Some(prefix_width);
            }
            if let Some(num_width) = formatting.num_width {
                self.formatting.num_width = Some(num_width);
            }
            if let Some(currency_column) = formatting.currency_column {
                self.formatting.currency_column = Some(currency_column);
            }
            if let Some(spacing) = formatting.account_amount_spacing {
                self.formatting.account_amount_spacing = spacing;
            }
            if let Some(spacing) = formatting.number_currency_spacing {
                self.formatting.number_currency_spacing = spacing;
            }
            if let Some(indent_width) = formatting.indent_width {
                self.formatting.indent_width = Some(indent_width);
            }
        }

        // Update bean-check configuration
        if let Some(bean_check) = beancount_lsp_settings.bean_check {
            if let Some(method) = bean_check.method {
                self.bean_check.method = Some(method);
            }
            if let Some(bean_check_cmd) = bean_check.bean_check_cmd {
                self.bean_check.bean_check_cmd = Some(PathBuf::from(bean_check_cmd));
            }
            if let Some(python_cmd) = bean_check.python_cmd {
                self.bean_check.python_cmd = Some(PathBuf::from(python_cmd));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountLspOptions {
    pub journal_file: Option<String>,
    pub formatting: Option<FormattingOptions>,
    pub bean_check: Option<BeancountCheckOptions>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FormattingOptions {
    /// Use this prefix width instead of determining an optimal value automatically.
    pub prefix_width: Option<usize>,

    /// Use this width to render numbers instead of determining an optimal value.
    pub num_width: Option<usize>,

    /// Align currencies in this column.
    pub currency_column: Option<usize>,

    /// Spacing between account names and amounts.
    pub account_amount_spacing: Option<usize>,

    /// Number of spaces between the number and currency.
    pub number_currency_spacing: Option<usize>,

    /// Enforce consistent indentation width for postings and directives.
    pub indent_width: Option<usize>,
}

#[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountCheckOptions {
    /// Method for bean-check execution: "system", "python-system", or "python-embedded"
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub method: Option<BeancountCheckMethod>,
    /// Path to bean-check executable (for system method)
    pub bean_check_cmd: Option<String>,
    /// Path to Python executable (for python method)
    pub python_cmd: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_journal() {
        let mut config = Config::new(PathBuf::new());
        config.update(serde_json::from_str("[]").unwrap()).unwrap();
        assert_eq!(config.journal_root, None);
    }

    #[test]
    fn test_null_journal() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"journal_file\": null}").unwrap())
            .unwrap();
        assert_eq!(config.journal_root, None);
    }

    #[test]
    fn test_journal() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"journal_file\": \"mypath\"}").unwrap())
            .unwrap();
        assert_eq!(config.journal_root, Some("mypath".into()));
    }

    #[test]
    fn test_empty_journal() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"journal_file\": \"\"}").unwrap())
            .unwrap();
        assert_eq!(config.journal_root, None);
    }

    #[test]
    fn test_whitespace_journal() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"journal_file\": \"   \"}").unwrap())
            .unwrap();
        assert_eq!(config.journal_root, None);
    }

    #[test]
    fn test_formatting_config_defaults() {
        let config = FormattingConfig::default();
        assert_eq!(config.prefix_width, None);
        assert_eq!(config.num_width, None);
        assert_eq!(config.currency_column, None);
        assert_eq!(config.account_amount_spacing, 2);
        assert_eq!(config.number_currency_spacing, 1);
        assert_eq!(config.indent_width, None);
    }

    #[test]
    fn test_formatting_prefix_width() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"formatting\": {\"prefix_width\": 60}}").unwrap())
            .unwrap();
        assert_eq!(config.formatting.prefix_width, Some(60));
    }

    #[test]
    fn test_formatting_num_width() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"formatting\": {\"num_width\": 15}}").unwrap())
            .unwrap();
        assert_eq!(config.formatting.num_width, Some(15));
    }

    #[test]
    fn test_formatting_currency_column() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"formatting\": {\"currency_column\": 80}}").unwrap())
            .unwrap();
        assert_eq!(config.formatting.currency_column, Some(80));
    }

    #[test]
    fn test_formatting_account_amount_spacing() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(
                serde_json::from_str("{\"formatting\": {\"account_amount_spacing\": 4}}").unwrap(),
            )
            .unwrap();
        assert_eq!(config.formatting.account_amount_spacing, 4);
    }

    #[test]
    fn test_formatting_number_currency_spacing() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(
                serde_json::from_str("{\"formatting\": {\"number_currency_spacing\": 2}}").unwrap(),
            )
            .unwrap();
        assert_eq!(config.formatting.number_currency_spacing, 2);
    }

    #[test]
    fn test_formatting_indent_width() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str("{\"formatting\": {\"indent_width\": 4}}").unwrap())
            .unwrap();
        assert_eq!(config.formatting.indent_width, Some(4));
    }

    #[test]
    fn test_formatting_multiple_options() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(
                serde_json::from_str(
                    r#"{
                        "formatting": {
                            "prefix_width": 50,
                            "num_width": 12,
                            "currency_column": 70,
                            "account_amount_spacing": 3,
                            "number_currency_spacing": 1,
                            "indent_width": 2
                        }
                    }"#,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(config.formatting.prefix_width, Some(50));
        assert_eq!(config.formatting.num_width, Some(12));
        assert_eq!(config.formatting.currency_column, Some(70));
        assert_eq!(config.formatting.account_amount_spacing, 3);
        assert_eq!(config.formatting.number_currency_spacing, 1);
        assert_eq!(config.formatting.indent_width, Some(2));
    }

    #[test]
    fn test_bean_check_method_system() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str(r#"{"bean_check": {"method": "system"}}"#).unwrap())
            .unwrap();
        assert_eq!(
            config.bean_check.method,
            Some(BeancountCheckMethod::SystemCall)
        );
    }

    #[test]
    fn test_bean_check_method_python_embedded() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(
                serde_json::from_str(r#"{"bean_check": {"method": "python-embedded"}}"#).unwrap(),
            )
            .unwrap();
        assert_eq!(
            config.bean_check.method,
            Some(BeancountCheckMethod::PythonEmbedded)
        );
    }

    #[test]
    fn test_bean_check_method_pyo3_alias() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str(r#"{"bean_check": {"method": "pyo3"}}"#).unwrap())
            .unwrap();
        assert_eq!(
            config.bean_check.method,
            Some(BeancountCheckMethod::PythonEmbedded)
        );
    }

    #[test]
    fn test_bean_check_cmd_path() {
        let config = Config::new(PathBuf::new());
        // Check default value
        assert_eq!(config.bean_check.bean_check_cmd, None);
    }

    #[test]
    fn test_bean_check_python_cmd() {
        let config = Config::new(PathBuf::new());
        // Check default value
        assert_eq!(config.bean_check.python_cmd, None);
    }

    #[test]
    fn test_config_new() {
        let config = Config::new(PathBuf::from("/path/to/file.bean"));
        assert_eq!(config.root_dir, PathBuf::from("/path/to/file.bean"));
        assert_eq!(config.journal_root, None);
        assert_eq!(config.formatting.prefix_width, None);
        assert_eq!(config.bean_check.method, None);
    }

    #[test]
    fn test_update_with_invalid_json() {
        let mut config = Config::new(PathBuf::new());
        // Invalid JSON should be ignored gracefully
        let result = config.update(serde_json::from_str("[]").unwrap());
        assert!(result.is_ok());
        assert_eq!(config.journal_root, None);
    }

    #[test]
    fn test_update_with_invalid_method() {
        let mut config = Config::new(PathBuf::new());
        // Invalid bean_check method should be ignored
        config
            .update(
                serde_json::from_str(r#"{"bean_check": {"method": "invalid-method"}}"#).unwrap(),
            )
            .unwrap();
        // Should keep default method (None)
        assert_eq!(config.bean_check.method, None);
    }

    #[test]
    fn test_update_with_valid_method() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(serde_json::from_str(r#"{"bean_check": {"method": "python-system"}}"#).unwrap())
            .unwrap();
        assert_eq!(
            config.bean_check.method,
            Some(BeancountCheckMethod::PythonSystem)
        );
    }

    #[test]
    fn test_init_options_with_empty_objects() {
        let mut config = Config::new(PathBuf::from("/workspace"));
        config
            .update(
                serde_json::from_str(
                    r#"{
                        "bean_check": {},
                        "formatting": {},
                        "journal_file": "./main.bean"
                    }"#,
                )
                .unwrap(),
            )
            .unwrap();

        assert_eq!(config.journal_root, Some(PathBuf::from("./main.bean")));
        assert_eq!(config.bean_check.method, None);
        assert_eq!(config.formatting.prefix_width, None);
    }

    #[test]
    fn test_combined_journal_and_formatting() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(
                serde_json::from_str(
                    r#"{
                        "journal_file": "/path/to/journal.bean",
                        "formatting": {
                            "prefix_width": 60,
                            "indent_width": 4
                        }
                    }"#,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            config.journal_root,
            Some(PathBuf::from("/path/to/journal.bean"))
        );
        assert_eq!(config.formatting.prefix_width, Some(60));
        assert_eq!(config.formatting.indent_width, Some(4));
    }

    #[test]
    fn test_combined_all_options() {
        let mut config = Config::new(PathBuf::new());
        config
            .update(
                serde_json::from_str(
                    r#"{
                        "journal_file": "/path/to/journal.bean",
                        "formatting": {
                            "prefix_width": 60
                        },
                        "bean_check": {
                            "method": "python-embedded",
                            "python_cmd": "/usr/bin/python3"
                        }
                    }"#,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            config.journal_root,
            Some(PathBuf::from("/path/to/journal.bean"))
        );
        assert_eq!(config.formatting.prefix_width, Some(60));
        assert_eq!(
            config.bean_check.method,
            Some(BeancountCheckMethod::PythonEmbedded)
        );
        assert_eq!(
            config.bean_check.python_cmd,
            Some(PathBuf::from("/usr/bin/python3"))
        );
    }
}
