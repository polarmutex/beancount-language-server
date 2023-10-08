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
        let beancount_lsp_settings: BeancountLspOptions = serde_json::from_value(json).unwrap();
        if beancount_lsp_settings.journal_file.is_some() {
            self.journal_root = Some(PathBuf::from(
                shellexpand::tilde(&beancount_lsp_settings.journal_file.unwrap()).as_ref(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountLspOptions {
    pub journal_file: Option<String>,
}
