use crate::core;
use dashmap::DashMap;
use log::debug;
use lspower::lsp;
use std::path::PathBuf;
use tokio::process::Command;

/// Provider function for LSP `textDocument/publishDiagnostics`.
pub async fn diagnostics(
    bean_check_cmd: &PathBuf,
    root_journal_file: &PathBuf,
) -> DashMap<lsp::Url, Vec<lsp::Diagnostic>> {
    let error_line_regexp = regex::Regex::new(r"^([^:]+):(\d+):\s*(.*)$").unwrap();

    debug!("providers::diagnostics");
    let output = Command::new(bean_check_cmd)
        .arg(root_journal_file)
        .output()
        .await
        .map_err(core::Error::from)
        .unwrap();
    debug!("bean-check outupt {:?}", output);

    let diags = if !output.status.success() {
        let output = std::str::from_utf8(&output.stderr).map_err(core::Error::from);

        let map: DashMap<lsp::Url, Vec<lsp::Diagnostic>> = DashMap::new();
        for line in output.unwrap().lines() {
            debug!("line: {}", line);
            if let Some(caps) = error_line_regexp.captures(line) {
                debug!("caps: {:?}", caps);
                let position = lsp::Position {
                    line: caps[2].parse::<u32>().unwrap().saturating_sub(1),
                    character: 0,
                };

                let file_url = lsp::Url::from_file_path(caps[1].to_string()).unwrap();
                let diag = lsp::Diagnostic {
                    range: lsp::Range {
                        start: position,
                        end: position,
                    },
                    message: caps[3].trim().to_string(),
                    ..lsp::Diagnostic::default()
                };
                if map.contains_key(&file_url) {
                    map.get_mut(&file_url).unwrap().push(diag);
                } else {
                    map.insert(file_url, vec![diag]);
                }
            }
        }
        map
    } else {
        DashMap::new()
    };
    diags
}
