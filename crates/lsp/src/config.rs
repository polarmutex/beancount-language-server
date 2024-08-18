use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub root_file: PathBuf,
    pub journal_root: Option<PathBuf>,
}

impl Config {
    pub fn new(root_file: PathBuf) -> Self {
        Self {
            root_file,
            journal_root: None,
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
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountLspOptions {
    pub journal_file: Option<String>,
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
