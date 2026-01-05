use crate::checkers::{BeancountCheckConfig, BeancountCheckMethod};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub root_file: PathBuf,
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
    pub fn new(root_file: PathBuf) -> Self {
        Self {
            root_file,
            journal_root: None,
            formatting: FormattingConfig::default(),
            bean_check: BeancountCheckConfig::default(),
        }
    }
    pub fn update(&mut self, json: serde_json::Value) -> Result<()> {
        // Check explicitly for Ok() here to avoid panicking on invalid input.
        // Gracefully ignore non-BeancountLspOptions inputs here.
        // Example: "[]" is sent by nvim-lspconfig if no initialization options are specified in
        // Lua.
        if let Ok(beancount_lsp_settings) = serde_json::from_value::<BeancountLspOptions>(json) {
            // Only set journal_root if journal_file is present and non-empty
            if let Some(journal_file) = beancount_lsp_settings.journal_file {
                if !journal_file.trim().is_empty() {
                    self.journal_root = Some(PathBuf::from(
                        shellexpand::tilde(&journal_file).as_ref(),
                    ));
                } else {
                    tracing::debug!("Journal file is empty string, treating as None");
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
                    self.bean_check.method = method;
                }
                if let Some(bean_check_cmd) = bean_check.bean_check_cmd {
                    self.bean_check.bean_check_cmd = PathBuf::from(bean_check_cmd);
                }
                if let Some(python_cmd) = bean_check.python_cmd {
                    self.bean_check.python_cmd = PathBuf::from(python_cmd);
                }
                if let Some(python_script_path) = bean_check.python_script_path {
                    self.bean_check.python_script_path = PathBuf::from(python_script_path);
                }
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountCheckOptions {
    /// Method for bean-check execution: "system" or "python"
    #[serde(with = "bean_check_method_serde")]
    pub method: Option<BeancountCheckMethod>,
    /// Path to bean-check executable (for system method)
    pub bean_check_cmd: Option<String>,
    /// Path to Python executable (for python method)
    pub python_cmd: Option<String>,
    /// Path to Python script (for python method)
    pub python_script_path: Option<String>,
}

// Custom serde module for BeancountCheckMethod
mod bean_check_method_serde {
    use super::BeancountCheckMethod;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        value: &Option<BeancountCheckMethod>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(BeancountCheckMethod::SystemCall) => "system".serialize(serializer),
            Some(BeancountCheckMethod::PythonEmbedded) => "python-embedded".serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<BeancountCheckMethod>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: Option<String> = Option::deserialize(deserializer)?;
        match value.as_deref() {
            Some("system") => Ok(Some(BeancountCheckMethod::SystemCall)),
            Some("python-embedded") | Some("pyo3") => {
                Ok(Some(BeancountCheckMethod::PythonEmbedded))
            }
            Some(_) => Ok(None), // Invalid method, ignore gracefully
            None => Ok(None),
        }
    }
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
}
