use crate::beancount_data::BeancountData;
use crate::document::Document;
use crate::providers::diagnostics;
use crate::server::LspServerState;
use crate::server::LspServerStateSnapshot;
use crate::server::ProgressMsg;
use crate::server::Task;
use crate::to_json;
use crate::treesitter_utils::lsp_textdocchange_to_ts_inputedit;
use crate::utils::ToFilePath;
use anyhow::Result;
use crossbeam_channel::Sender;
use lsp_types::notification::Notification;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::debug;
use tree_sitter_beancount::tree_sitter;

/// Provider function for `textDocument/didOpen`.
pub(crate) fn did_open(
    state: &mut LspServerState,
    params: lsp_types::DidOpenTextDocumentParams,
) -> Result<()> {
    debug!("text_document::did_open");
    let uri = match params.text_document.uri.to_file_path() {
        Ok(path) => path,
        Err(_) => {
            debug!(
                "Failed to convert URI to file path: {:?}",
                params.text_document.uri
            );
            return Ok(());
        }
    };

    let document = Document::open(params.clone());
    tracing::debug!("text_document::did_open - adding {:#?}", &uri);
    state.open_docs.insert(uri.clone(), document);

    state.parsers.entry(uri.clone()).or_insert_with(|| {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        parser
    });
    let parser = state.parsers.get_mut(&uri).unwrap();

    state
        .forest
        .entry(uri.clone())
        .or_insert_with(|| parser.parse(&params.text_document.text, None).unwrap());

    state.beancount_data.entry(uri.clone()).or_insert_with(|| {
        let content = ropey::Rope::from_str(&params.text_document.text);
        BeancountData::new(state.forest.get(&uri).unwrap(), &content)
    });

    let snapshot = state.snapshot();
    let task_sender = state.task_sender.clone();
    state.thread_pool.execute(move || {
        let _result = handle_diagnostics(snapshot, task_sender, params.text_document.uri);
    });

    Ok(())
}

/// Provider function for `textDocument/didSave`.
pub(crate) fn did_save(
    state: &mut LspServerState,
    params: lsp_types::DidSaveTextDocumentParams,
) -> Result<()> {
    tracing::debug!("text_document::did_save");

    let snapshot = state.snapshot();
    let task_sender = state.task_sender.clone();
    state.thread_pool.execute(move || {
        let _result = handle_diagnostics(snapshot, task_sender, params.text_document.uri);
    });

    Ok(())
}

/// Provider function for `textDocument/didClose`.
pub(crate) fn did_close(
    state: &mut LspServerState,
    params: lsp_types::DidCloseTextDocumentParams,
) -> Result<()> {
    tracing::debug!("text_document::did_close");
    let uri = params.text_document.uri.to_file_path().unwrap();
    state.open_docs.remove(&uri);
    Ok(())
}

/// Provider function for `textDocument/didChange`.
pub(crate) fn did_change(
    state: &mut LspServerState,
    params: lsp_types::DidChangeTextDocumentParams,
) -> Result<()> {
    tracing::debug!("text_document::did_change");
    let uri = &params.text_document.uri.to_file_path().unwrap();
    tracing::debug!("text_document::did_change - requesting {:#?}", uri);
    let doc = state.open_docs.get_mut(uri).unwrap();

    tracing::debug!("text_document::did_change - convert edits");
    let edits = params
        .content_changes
        .iter()
        .map(|change| lsp_textdocchange_to_ts_inputedit(&doc.content, change))
        .collect::<Result<Vec<_>, _>>()?;

    tracing::debug!("text_document::did_change - apply edits - document");
    for change in &params.content_changes {
        let text = change.text.as_str();
        let text_bytes = text.as_bytes();
        let text_end_byte_idx = text_bytes.len();

        let range = if let Some(range) = change.range {
            range
        } else {
            let start_line_idx = doc.content.byte_to_line(0);
            let end_line_idx = doc.content.byte_to_line(text_end_byte_idx);

            let start = lsp_types::Position::new(start_line_idx as u32, 0);
            let end = lsp_types::Position::new(end_line_idx as u32, 0);
            lsp_types::Range { start, end }
        };

        let start_row_char_idx = doc.content.line_to_char(range.start.line as usize);
        let start_col_char_idx = doc.content.utf16_cu_to_char(range.start.character as usize);
        let end_row_char_idx = doc.content.line_to_char(range.end.line as usize);
        let end_col_char_idx = doc.content.utf16_cu_to_char(range.end.character as usize);

        let start_char_idx = start_row_char_idx + start_col_char_idx;
        let end_char_idx = end_row_char_idx + end_col_char_idx;
        doc.content.remove(start_char_idx..end_char_idx);

        if !change.text.is_empty() {
            doc.content.insert(start_char_idx, text);
        }
    }

    debug!("text_document::did_change - apply edits - tree");
    let result = {
        let parser = state.parsers.get_mut(uri).unwrap();
        let old_tree = state.forest.get_mut(uri).unwrap();

        for edit in &edits {
            old_tree.edit(edit);
        }

        parser.parse(doc.text().to_string(), Some(old_tree))
    };

    debug!("text_document::did_change - save tree");
    if let Some(tree) = result {
        *state.forest.get_mut(uri).unwrap() = tree.clone();
        *state.beancount_data.get_mut(uri).unwrap() = BeancountData::new(&tree, &doc.content);
    }

    debug!("text_document::did_change - done");
    Ok(())
}

fn handle_diagnostics(
    snapshot: LspServerStateSnapshot,
    sender: Sender<Task>,
    uri: lsp_types::Uri,
) -> Result<()> {
    tracing::debug!("text_document::handle_diagnostics");
    let bean_check_cmd = &PathBuf::from("bean-check");

    sender
        .send(Task::Progress(ProgressMsg::BeanCheck { done: 0, total: 1 }))
        .unwrap();

    let root_journal_path = if snapshot.config.journal_root.is_some() {
        snapshot.config.journal_root.unwrap()
    } else {
        // Use proper URI to file path conversion instead of string replacement
        uri.to_file_path().unwrap_or_default()
    };

    let diags =
        diagnostics::diagnostics(snapshot.beancount_data, bean_check_cmd, &root_journal_path);

    sender
        .send(Task::Progress(ProgressMsg::BeanCheck { done: 1, total: 1 }))
        .unwrap();

    for file in snapshot.forest.keys() {
        let diagnostics = if diags.contains_key(file) {
            diags.get(file).unwrap().clone()
        } else {
            vec![]
        };
        sender
            .send(Task::Notify(lsp_server::Notification {
                method: lsp_types::notification::PublishDiagnostics::METHOD.to_owned(),
                params: to_json(lsp_types::PublishDiagnosticsParams {
                    uri: {
                        // Handle cross-platform file URI creation
                        let file_path_str = file.to_str().unwrap();
                        let uri_str = if cfg!(windows)
                            && file_path_str.len() > 1
                            && file_path_str.chars().nth(1) == Some(':')
                        {
                            // Windows absolute path like "C:\path"
                            format!("file:///{}", file_path_str.replace('\\', "/"))
                        } else if cfg!(windows) && file_path_str.starts_with('/') {
                            // Unix-style path on Windows, convert to Windows style
                            format!("file:///C:{}", file_path_str.replace('\\', "/"))
                        } else {
                            // Unix path or other platforms
                            format!("file://{file_path_str}")
                        };
                        lsp_types::Uri::from_str(&uri_str).unwrap()
                    },
                    diagnostics,
                    version: None,
                })
                .unwrap(),
            }))
            .unwrap()
    }
    Ok(())
}
