use crate::core;
use log::debug;
use lspower::lsp;
use std::path::PathBuf;
use tokio::process::Command;

/// Provider function for LSP `textDocument/publishDiagnostics`.
pub async fn diagnostics(bean_check_cmd: &PathBuf, root_journal_file: &PathBuf) -> Vec<lsp::Diagnostic> {
    debug!("providers::diagnostics");
    let output = Command::new(bean_check_cmd)
        .arg(root_journal_file)
        .output()
        .await
        .map_err(core::Error::from);
    debug!("bean-check outupt {:?}", output);
    vec![]
}
