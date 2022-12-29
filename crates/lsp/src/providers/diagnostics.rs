use crate::beancount_data::BeancountData;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use tracing::debug;

pub struct DiagnosticData {
    current_diagnostics: HashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>>,
}
impl DiagnosticData {
    pub fn new() -> Self {
        Self {
            current_diagnostics: HashMap::new(),
        }
    }

    /*pub fn update(&self, data: HashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>>) {
        self.current_diagnostics.clear();
        for it in data.iter() {
            self.current_diagnostics.insert(it.0.clone(), it.1.clone());
        }
    }*/
}

impl Default for DiagnosticData {
    fn default() -> Self {
        Self::new()
    }
}

/// Provider function for LSP `textDocument/publishDiagnostics`.
pub fn diagnostics(
    //previous_diagnostics: &DiagnosticData,
    beancount_data: HashMap<lsp_types::Url, BeancountData>,
    bean_check_cmd: &Path,
    root_journal_file: &Path,
) -> HashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>> {
    let error_line_regexp = regex::Regex::new(r"^([^:]+):(\d+):\s*(.*)$").unwrap();

    debug!("providers::diagnostics");
    let output = Command::new(bean_check_cmd)
        .arg(root_journal_file)
        .output()
        .unwrap();
    debug!("bean-check outupt {:?}", output);

    let diags = if !output.status.success() {
        debug!("bean-check generating diags");
        let output = std::str::from_utf8(&output.stderr);

        let mut map: HashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>> = HashMap::new();

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
        HashMap::new()
    };

    let mut ret: HashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>> = HashMap::new();

    // Add previous urls to clear out if neccessary
    //for it in previous_diagnostics.current_diagnostics.iter() {
    //    ret.insert(it.key().clone(), vec![]);
    //}
    // add bean-check errors
    for url in diags.iter() {
        for diag in url.1.iter() {
            if ret.contains_key(url.0) {
                ret.get_mut(url.0).unwrap().push(diag.clone());
            } else {
                ret.insert(url.0.clone(), vec![diag.clone()]);
            }
        }
    }
    // add flagged entries
    for data in beancount_data.iter() {
        for entry in data.1.flagged_entries.iter() {
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
            if ret.contains_key(data.0) {
                ret.get_mut(data.0).unwrap().push(diag);
            } else {
                ret.insert(data.0.clone(), vec![diag]);
            }
        }
    }
    ret
}
