use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub root_file: PathBuf,
    pub journal_root: Option<PathBuf>,
    pub formatting: FormattingConfig,
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
}

impl FormattingConfig {
    pub fn default() -> Self {
        Self {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2, // Default spacing like bean-format
        }
    }
}

impl Config {
    pub fn new(root_file: PathBuf) -> Self {
        Self {
            root_file,
            journal_root: None,
            formatting: FormattingConfig::default(),
        }
    }
    pub fn update(&mut self, json: serde_json::Value) -> Result<()> {
        // Check explicitly for Ok() here to avoid panicking on invalid input.
        // Gracefully ignore non-BeancountLspOptions inputs here.
        // Example: "[]" is sent by nvim-lspconfig if no initialization options are specified in
        // Lua.
        if let Ok(beancount_lsp_settings) = serde_json::from_value::<BeancountLspOptions>(json) {
            if beancount_lsp_settings.journal_file.is_some() {
                self.journal_root = Some(PathBuf::from(
                    shellexpand::tilde(&beancount_lsp_settings.journal_file.unwrap()).as_ref(),
                ));
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
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountLspOptions {
    pub journal_file: Option<String>,
    pub formatting: Option<FormattingOptions>,
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
}
