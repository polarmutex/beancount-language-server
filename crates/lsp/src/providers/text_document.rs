use crate::beancount_data::BeancountData;
use crate::checkers::create_checker;
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
use glob::glob;
use lsp_types::notification::Notification;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tracing::debug;
use tree_sitter_beancount::tree_sitter;

/// Process included files recursively from a given beancount file
fn process_includes(
    state: &mut LspServerState,
    file_path: &PathBuf,
    processed: &mut HashSet<PathBuf>,
) -> Result<()> {
    // Avoid infinite loops in case of circular includes
    if processed.contains(file_path) {
        return Ok(());
    }
    processed.insert(file_path.clone());

    // Get the tree for this file (should already be parsed)
    let tree = match state.forest.get(file_path) {
        Some(tree) => tree.clone(),
        None => return Ok(()), // File not parsed yet, skip
    };

    // Find all include directives in this file
    let text = fs::read_to_string(file_path)?;
    let bytes = text.as_bytes();
    let mut cursor = tree.root_node().walk();

    let include_paths: Vec<PathBuf> = tree
        .root_node()
        .children(&mut cursor)
        .filter(|c| c.kind() == "include")
        .filter_map(|include_node| {
            let mut node_cursor = include_node.walk();
            let string_node = include_node
                .children(&mut node_cursor)
                .find(|c| c.kind() == "string")?;

            let filename = string_node
                .utf8_text(bytes)
                .ok()?
                .trim_start_matches('"')
                .trim_end_matches('"');

            let path = std::path::Path::new(filename);
            let resolved_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                file_path.parent()?.join(path)
            };

            Some(resolved_path)
        })
        .collect();

    // Process each included file
    for include_path in include_paths {
        // Handle glob patterns
        for path in glob(include_path.to_str().unwrap_or(""))
            .unwrap_or_else(|_| {
                // If glob fails, try the path as-is
                glob::glob_with(
                    &include_path.to_string_lossy(),
                    glob::MatchOptions::default(),
                )
                .unwrap_or(glob("").unwrap())
            })
            .flatten()
        {
            // Skip if already processed
            if state.forest.contains_key(&path) {
                continue;
            }

            // Parse the included file
            if let Ok(text) = fs::read_to_string(&path) {
                let mut parser = tree_sitter::Parser::new();
                if parser
                    .set_language(&tree_sitter_beancount::language())
                    .is_ok()
                {
                    if let Some(tree) = parser.parse(&text, None) {
                        let content = ropey::Rope::from_str(&text);
                        let beancount_data = BeancountData::new(&tree, &content);

                        // Add to state
                        state.forest.insert(path.clone(), Arc::new(tree));
                        state
                            .beancount_data
                            .insert(path.clone(), Arc::new(beancount_data));

                        debug!("Processed included file: {:?}", path);

                        // Recursively process includes in this file
                        process_includes(state, &path, processed)?;
                    }
                }
            }
        }
    }

    Ok(())
}

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
        .or_insert_with(|| Arc::new(parser.parse(&params.text_document.text, None).unwrap()));

    state.beancount_data.entry(uri.clone()).or_insert_with(|| {
        let content = ropey::Rope::from_str(&params.text_document.text);
        Arc::new(BeancountData::new(
            state.forest.get(&uri).unwrap(),
            &content,
        ))
    });

    // Process any included files from this document
    let mut processed = HashSet::new();
    if let Err(e) = process_includes(state, &uri, &mut processed) {
        debug!("Error processing includes for {:?}: {}", uri, e);
    }

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
        let old_tree_arc = state.forest.get(uri).unwrap();
        let mut old_tree = (**old_tree_arc).clone();

        for edit in &edits {
            old_tree.edit(edit);
        }

        parser.parse(doc.text().to_string(), Some(&old_tree))
    };

    debug!("text_document::did_change - save tree");
    if let Some(tree) = result {
        let tree_arc = Arc::new(tree);
        *state.forest.get_mut(uri).unwrap() = tree_arc.clone();
        *state.beancount_data.get_mut(uri).unwrap() =
            Arc::new(BeancountData::new(&tree_arc, &doc.content));
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

    // Create the appropriate checker based on configuration
    tracing::debug!(
        "Bean check configuration: method={:?}, bean_check_cmd={}, python_cmd={}, python_script={}",
        snapshot.config.bean_check.method,
        snapshot.config.bean_check.bean_check_cmd.display(),
        snapshot.config.bean_check.python_cmd.display(),
        snapshot.config.bean_check.python_script_path.display()
    );

    let checker = create_checker(&snapshot.config.bean_check);
    tracing::debug!(
        "Using checker: {}, available: {}",
        checker.name(),
        checker.is_available()
    );

    sender
        .send(Task::Progress(ProgressMsg::BeanCheck { done: 0, total: 1 }))
        .unwrap();

    let root_journal_path = if snapshot.config.journal_root.is_some() {
        snapshot.config.journal_root.unwrap()
    } else {
        // Use proper URI to file path conversion instead of string replacement
        uri.to_file_path().unwrap_or_default()
    };

    let diags = diagnostics::diagnostics(
        snapshot.beancount_data,
        checker.as_ref(),
        &root_journal_path,
    );

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
