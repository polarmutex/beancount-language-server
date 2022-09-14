use crate::beancount_data::BeancountData;
use dashmap::DashMap;
use log::debug;
use std::path::Path;
use tokio::process::Command;
use tower_lsp::lsp_types;

pub struct DiagnosticData {
    current_diagnostics: DashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>>,
}
impl DiagnosticData {
    pub fn new() -> Self {
        Self {
            current_diagnostics: DashMap::new(),
        }
    }

    pub fn update(&self, data: DashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>>) {
        self.current_diagnostics.clear();
        for it in data.iter() {
            self.current_diagnostics
                .insert(it.key().clone(), it.value().clone());
        }
    }
}

impl Default for DiagnosticData {
    fn default() -> Self {
        Self::new()
    }
}

/// Provider function for LSP `textDocument/publishDiagnostics`.
pub async fn diagnostics(
    previous_diagnostics: &DiagnosticData,
    beancount_data: &BeancountData,
    bean_check_cmd: &Path,
    root_journal_file: &Path,
) -> DashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>> {
    let error_line_regexp = regex::Regex::new(r"^([^:]+):(\d+):\s*(.*)$").unwrap();

    debug!("providers::diagnostics");
    let output = Command::new(bean_check_cmd)
        .arg(root_journal_file)
        .output()
        .await
        .unwrap();
    debug!("bean-check outupt {:?}", output);

    let diags = if !output.status.success() {
        debug!("bean-check generating diags");
        let output = std::str::from_utf8(&output.stderr);

        let map: DashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>> = DashMap::new();

        for line in output.unwrap().lines() {
            debug!("line: {}", line);
            if let Some(caps) = error_line_regexp.captures(line) {
                debug!("caps: {:?}", caps);
                let position = lsp_types::Position {
                    line: caps[2].parse::<u32>().unwrap().saturating_sub(1),
                    character: 0,
                };

                let file_url = lsp_types::Url::from_file_path(&caps[1]).unwrap();
                let diag = lsp_types::Diagnostic {
                    range: lsp_types::Range {
                        start: position,
                        end: position,
                    },
                    message: caps[3].trim().to_string(),
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    ..lsp_types::Diagnostic::default()
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

    let ret: DashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>> = DashMap::new();

    // Add previous urls to clear out if neccessary
    for it in previous_diagnostics.current_diagnostics.iter() {
        ret.insert(it.key().clone(), vec![]);
    }
    // add bean-check errors
    for url in diags.iter() {
        for diag in url.value().iter() {
            if ret.contains_key(url.key()) {
                ret.get_mut(url.key()).unwrap().push(diag.clone());
            } else {
                ret.insert(url.key().clone(), vec![diag.clone()]);
            }
        }
    }
    // add flagged entries
    for uri in beancount_data.flagged_entries.iter() {
        for entry in uri.value().iter() {
            let position = lsp_types::Position {
                line: entry.line,
                character: 0,
            };
            let diag = lsp_types::Diagnostic {
                range: lsp_types::Range {
                    start: position,
                    end: position,
                },
                message: "Flagged".to_string(),
                severity: Some(lsp_types::DiagnosticSeverity::WARNING),
                ..lsp_types::Diagnostic::default()
            };
            if ret.contains_key(uri.key()) {
                ret.get_mut(uri.key()).unwrap().push(diag);
            } else {
                ret.insert(uri.key().clone(), vec![diag]);
            }
        }
    }
    ret
}
