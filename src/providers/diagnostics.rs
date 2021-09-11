use crate::core;
use dashmap::DashMap;
use log::debug;
use lspower::lsp;
use std::{path::PathBuf, sync::Arc};
use tokio::process::Command;

pub struct DiagnosticData {
    current_diagnostics: DashMap<lsp::Url, Vec<lsp::Diagnostic>>,
}
impl DiagnosticData {
    pub fn new() -> Self {
        Self {
            current_diagnostics: DashMap::new(),
        }
    }

    pub fn update(&self, data: DashMap<lsp::Url, Vec<lsp::Diagnostic>>) {
        self.current_diagnostics.clear();
        for it in data.iter() {
            self.current_diagnostics.insert(it.key().clone(), it.value().clone());
        }
    }
}

/// Provider function for LSP `textDocument/publishDiagnostics`.
pub async fn diagnostics(
    previous_diagnostics: &DiagnosticData,
    beancount_data: &core::BeancountData,
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
        debug!("bean-check generating diags");
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
                    severity: Some(lsp::DiagnosticSeverity::Error),
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
        debug!("bean-check return empty");
        DashMap::new()
    };

    let ret: DashMap<lsp::Url, Vec<lsp::Diagnostic>> = DashMap::new();

    // Add previous urls to clear out if neccessary
    for it in previous_diagnostics.current_diagnostics.iter() {
        ret.insert(it.key().clone(), vec![]);
    }
    // add bean-check errors
    for url in diags.iter() {
        for diag in url.value().iter() {
            if ret.contains_key(&url.key()) {
                ret.get_mut(&url.key()).unwrap().push(diag.clone());
            } else {
                ret.insert(url.key().clone(), vec![diag.clone()]);
            }
        }
    }
    // add flagged entries
    for uri in beancount_data.flagged_entries.iter() {
        for entry in uri.value().iter() {
            let position = lsp::Position {
                line: entry.line,
                character: 0,
            };
            let diag = lsp::Diagnostic {
                range: lsp::Range {
                    start: position,
                    end: position,
                },
                message: "Flagged".to_string(),
                severity: Some(lsp::DiagnosticSeverity::Warning),
                ..lsp::Diagnostic::default()
            };
            if ret.contains_key(&uri.key()) {
                ret.get_mut(&uri.key()).unwrap().push(diag);
            } else {
                ret.insert(uri.key().clone(), vec![diag]);
            }
        }
    }
    ret
}
