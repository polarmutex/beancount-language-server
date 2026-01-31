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
use anyhow::{Context, Result, anyhow};
use crossbeam_channel::Sender;
use glob::glob;
use lsp_types::notification::Notification;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, warn};
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

    // Find all include directives in this file using tree-sitter query
    let text = fs::read_to_string(file_path)?;
    let bytes = text.as_bytes();

    let include_query_string = r#"
    (include (string) @string)
    "#;
    let include_query =
        tree_sitter::Query::new(&tree_sitter_beancount::language(), include_query_string)
            .unwrap_or_else(|_| panic!("Invalid query for includes: {include_query_string}"));
    let mut cursor_qry = tree_sitter::QueryCursor::new();
    let mut include_matches = cursor_qry.matches(&include_query, tree.root_node(), bytes);

    let include_paths: Vec<PathBuf> = {
        use tree_sitter::StreamingIterator;
        let mut paths = Vec::new();

        while let Some(qmatch) = include_matches.next() {
            for capture in qmatch.captures {
                let filename = capture
                    .node
                    .utf8_text(bytes)
                    .ok()
                    .map(|s| s.trim_start_matches('"').trim_end_matches('"'));

                if let Some(filename) = filename {
                    let path = std::path::Path::new(filename);
                    let resolved_path = if path.is_absolute() {
                        path.to_path_buf()
                    } else if let Some(parent) = file_path.parent() {
                        parent.join(path)
                    } else {
                        continue;
                    };

                    paths.push(resolved_path);
                }
            }
        }

        paths
    };

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
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to create glob pattern: {}", e);
                    glob("").expect("empty glob pattern should always work")
                })
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
                    && let Some(tree) = parser.parse(&text, None)
                {
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
            .expect("Failed to set language for tree-sitter parser");
        parser
    });
    let parser = state
        .parsers
        .get_mut(&uri)
        .expect("parser should exist after insertion");

    // Always parse fresh content - the file may have been modified externally
    // between close and reopen, so we can't rely on cached trees
    let tree = Arc::new(
        parser
            .parse(&params.text_document.text, None)
            .expect("Failed to parse document"),
    );
    state.forest.insert(uri.clone(), tree);

    // Always extract fresh beancount data from the newly parsed tree
    let content = ropey::Rope::from_str(&params.text_document.text);
    state.beancount_data.insert(
        uri.clone(),
        Arc::new(BeancountData::new(
            state.forest.get(&uri).unwrap(),
            &content,
        )),
    );

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

    // Lazy extraction: Ensure BeancountData is extracted before diagnostics
    if let Ok(uri) = params.text_document.uri.to_file_path() {
        state.ensure_beancount_data(&uri);
    }

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
    state.open_docs.remove(&uri);
    // Clear cached parse tree and beancount data to ensure fresh parsing on reopen.
    // This handles external modifications made while the file was closed.
    // Note: We keep parsers for reuse as they are stateless.
    state.forest.remove(&uri);
    state.beancount_data.remove(&uri);
    Ok(())
}

/// Provider function for `workspace/didChangeWatchedFiles`.
/// Handles external file changes detected by the client's file watcher.
pub(crate) fn did_change_watched_files(
    state: &mut LspServerState,
    params: lsp_types::DidChangeWatchedFilesParams,
) -> Result<()> {
    tracing::debug!(
        "workspace::did_change_watched_files: {} changes",
        params.changes.len()
    );

    for change in params.changes {
        let uri = match change.uri.to_file_path() {
            Ok(path) => path,
            Err(_) => {
                debug!("Failed to convert URI to file path: {:?}", change.uri);
                continue;
            }
        };

        match change.typ {
            lsp_types::FileChangeType::CREATED | lsp_types::FileChangeType::CHANGED => {
                tracing::debug!(
                    "External file change detected: {:?} (type: {:?})",
                    uri,
                    change.typ
                );

                // Skip if file is currently open in editor (editor manages its own state)
                if state.open_docs.contains_key(&uri) {
                    tracing::debug!("Skipping {:?} - file is open in editor", uri);
                    continue;
                }

                // Clear stale cache so next access will re-parse
                if state.forest.remove(&uri).is_some() {
                    tracing::debug!("Cleared stale tree for {:?}", uri);
                }
                if state.beancount_data.remove(&uri).is_some() {
                    tracing::debug!("Cleared stale beancount_data for {:?}", uri);
                }

                // If this file is part of our forest (included files), re-parse it
                if let Ok(content) = fs::read_to_string(&uri) {
                    let mut parser = tree_sitter::Parser::new();
                    if parser
                        .set_language(&tree_sitter_beancount::language())
                        .is_ok()
                        && let Some(tree) = parser.parse(&content, None)
                    {
                        let rope_content = ropey::Rope::from_str(&content);
                        let beancount_data = BeancountData::new(&tree, &rope_content);

                        state.forest.insert(uri.clone(), Arc::new(tree));
                        state
                            .beancount_data
                            .insert(uri.clone(), Arc::new(beancount_data));

                        tracing::debug!("Re-parsed external file: {:?}", uri);
                    }
                }
            }
            lsp_types::FileChangeType::DELETED => {
                tracing::debug!("External file deleted: {:?}", uri);

                // Remove from all caches
                state.forest.remove(&uri);
                state.beancount_data.remove(&uri);
                state.parsers.remove(&uri);
            }
            _ => {
                tracing::debug!("Unknown file change type: {:?}", change.typ);
            }
        }
    }

    // Trigger diagnostics refresh for open documents
    if state.config.journal_root.is_some() {
        let snapshot = state.snapshot();
        let task_sender = state.task_sender.clone();

        // Find an open document to use for diagnostics URI
        if let Some(open_uri) = state.open_docs.keys().next().cloned() {
            let url = match url::Url::from_file_path(&open_uri) {
                Ok(url) => url,
                Err(_) => {
                    tracing::warn!("Failed to convert path to URL: {:?}", open_uri);
                    return Ok(());
                }
            };
            let lsp_uri = match lsp_types::Uri::from_str(url.as_str()) {
                Ok(uri) => uri,
                Err(e) => {
                    tracing::warn!("Failed to create LSP URI: {}", e);
                    return Ok(());
                }
            };

            state.thread_pool.execute(move || {
                let _result = handle_diagnostics(snapshot, task_sender, lsp_uri);
            });
        } else {
            tracing::debug!(
                "No open documents, skipping diagnostics refresh after external change"
            );
        }
    }

    Ok(())
}

/// Provider function for `textDocument/didChange`.
pub(crate) fn did_change(
    state: &mut LspServerState,
    params: lsp_types::DidChangeTextDocumentParams,
) -> Result<()> {
    tracing::debug!("text_document::did_change");
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
    tracing::debug!("text_document::did_change - requesting {:#?}", uri);
    let doc = match state.open_docs.get_mut(&uri) {
        Some(doc) => doc,
        None => {
            tracing::warn!("Document not found in open_docs: {:?}", uri);
            return Ok(());
        }
    };

    // Version tracking for synchronization validation
    let new_version = params.text_document.version;
    if new_version <= doc.version {
        tracing::warn!(
            "Received out-of-order or duplicate change: current version={}, received version={}",
            doc.version,
            new_version
        );
    }
    tracing::trace!("Document version: {} -> {}", doc.version, new_version);

    tracing::debug!("text_document::did_change - convert edits and apply changes");

    // Calculate tree-sitter edits before modifying the document
    // This must be done first since edits are based on the old content
    let ts_edits = params
        .content_changes
        .iter()
        .map(|change| lsp_textdocchange_to_ts_inputedit(&doc.content, change))
        .collect::<Result<Vec<_>, _>>()?;

    // Apply changes to document content
    // We reuse position calculations to avoid redundant UTF-16 conversions
    for change in &params.content_changes {
        let text = change.text.as_str();

        let range = if let Some(range) = change.range {
            range
        } else {
            // Full document replacement: range should cover entire current document
            let end_line = (doc.content.len_lines().saturating_sub(1)) as u32;
            let end_line_len = if doc.content.len_lines() > 0 {
                // Get the character length of the last line (excluding newline)
                let last_line = doc.content.line(end_line as usize);
                last_line.len_chars().saturating_sub(1).max(0) as u32
            } else {
                0
            };
            lsp_types::Range {
                start: lsp_types::Position::new(0, 0),
                end: lsp_types::Position::new(end_line, end_line_len),
            }
        };

        // Convert LSP positions (line, UTF-16 column) to rope character indices
        // LSP positions use UTF-16 code units for columns, rope uses UTF-8 characters
        let start_row_char_idx = doc.content.line_to_char(range.start.line as usize);
        let end_row_char_idx = doc.content.line_to_char(range.end.line as usize);

        // Convert UTF-16 column offsets to character offsets within the line
        // CRITICAL: range.start.character is UTF-16 offset *within the line*, not document-wide
        let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);
        let start_col_char_idx = doc
            .content
            .utf16_cu_to_char(start_line_utf16_cu + range.start.character as usize)
            - start_row_char_idx;

        let end_line_utf16_cu = doc.content.char_to_utf16_cu(end_row_char_idx);
        let end_col_char_idx = doc
            .content
            .utf16_cu_to_char(end_line_utf16_cu + range.end.character as usize)
            - end_row_char_idx;

        let start_char_idx = start_row_char_idx + start_col_char_idx;
        let end_char_idx = end_row_char_idx + end_col_char_idx;

        tracing::trace!(
            "Applying change: range={}:{}-{}:{}, char_idx={}-{}, text_len={}",
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character,
            start_char_idx,
            end_char_idx,
            text.len()
        );

        doc.content.remove(start_char_idx..end_char_idx);

        if !change.text.is_empty() {
            doc.content.insert(start_char_idx, text);
        }
    }

    debug!("text_document::did_change - incremental tree parse");
    let result = {
        let parser = match state.parsers.get_mut(&uri) {
            Some(p) => p,
            None => {
                tracing::warn!("Parser not found for document: {:?}", uri);
                return Ok(());
            }
        };
        let old_tree_arc = match state.forest.get(&uri) {
            Some(t) => t,
            None => {
                tracing::warn!("Tree not found in forest: {:?}", uri);
                return Ok(());
            }
        };

        // Avoid cloning the tree when possible - tree-sitter's edit() takes &mut
        // We clone here because we need to preserve the old tree in the Arc
        // until we've successfully parsed the new tree
        let mut old_tree = (**old_tree_arc).clone();

        // Apply all edits to the tree to prepare for incremental parsing
        for edit in &ts_edits {
            old_tree.edit(edit);
        }

        // Parse with incremental tree
        // Note: We could avoid the string allocation by implementing a custom TextProvider
        // that yields rope chunks, but the current tree-sitter bindings make this complex
        parser.parse(doc.text_string(), Some(&old_tree))
    };

    debug!("text_document::did_change - save tree");
    if let Some(tree) = result {
        let tree_arc = Arc::new(tree);
        *state
            .forest
            .get_mut(&uri)
            .expect("tree should exist in forest") = tree_arc.clone();
        // Lazy extraction: Don't extract BeancountData on every keystroke
        // It will be extracted on-demand when needed (e.g., for completion)
        state.beancount_data.remove(&uri);
    }

    // Update document version after successfully applying changes
    doc.version = new_version;

    debug!("text_document::did_change - done");
    Ok(())
}

fn handle_diagnostics(
    snapshot: LspServerStateSnapshot,
    sender: Sender<Task>,
    _uri: lsp_types::Uri,
) -> Result<()> {
    tracing::debug!("text_document::handle_diagnostics");

    let checker = match snapshot.checker.clone() {
        Some(checker) => checker,
        None => {
            tracing::warn!("No checker available; skipping diagnostics");
            return Ok(());
        }
    };

    tracing::debug!(
        "Using checker: {}, available: {}",
        checker.name(),
        checker.is_available()
    );

    let root_journal_path = match snapshot.config.journal_root.clone() {
        Some(path) => path,
        None => {
            tracing::warn!("No journal_root configured; skipping diagnostics");
            return Ok(());
        }
    };

    // Generate a unique run id for this diagnostics execution to avoid
    // progress token collisions if multiple runs overlap.
    let run_id: u64 = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };

    sender.send(Task::Progress(ProgressMsg::BeanCheck {
        done: 0,
        total: 1,
        checker_name: checker.name().to_string(),
        run_id,
    }))?;

    let diags = diagnostics::diagnostics(
        snapshot.beancount_data,
        checker.as_ref(),
        &root_journal_path,
    );

    sender.send(Task::Progress(ProgressMsg::BeanCheck {
        done: 1,
        total: 1,
        checker_name: checker.name().to_string(),
        run_id,
    }))?;

    let mut normalized_diags: HashMap<PathBuf, Vec<lsp_types::Diagnostic>> = HashMap::new();
    for (path, diagnostics) in diags {
        let key = normalize_path_for_diagnostics(&path);
        normalized_diags.entry(key).or_default().extend(diagnostics);
    }

    for file in snapshot.forest.keys() {
        let lookup = normalize_path_for_diagnostics(file);
        let diagnostics = normalized_diags.remove(&lookup).unwrap_or_default();
        sender
            .send(Task::Notify(lsp_server::Notification {
                method: lsp_types::notification::PublishDiagnostics::METHOD.to_owned(),
                params: to_json(lsp_types::PublishDiagnosticsParams {
                    uri: {
                        let url = url::Url::from_file_path(file).map_err(|()| {
                            anyhow!("Failed to convert file path to URI: {}", file.display())
                        })?;
                        lsp_types::Uri::from_str(url.as_str())
                            .with_context(|| format!("Failed to parse URL as LSP URI: {}", url))?
                    },
                    diagnostics,
                    version: None,
                })
                .unwrap(),
            }))
            .unwrap()
    }

    // ignore the broken file paths
    for (file, diagnostics) in normalized_diags {
        let url = match url::Url::from_file_path(&file) {
            Ok(url) => url,
            Err(_) => {
                warn!("Failed to convert file path to URI: {}", file.display());
                continue;
            }
        };

        let uri = match lsp_types::Uri::from_str(url.as_str()) {
            Ok(uri) => uri,
            Err(e) => {
                warn!("Failed to parse URL as LSP URI ({}): {}", url, e);
                continue;
            }
        };

        let params = match to_json(lsp_types::PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        }) {
            Ok(params) => params,
            Err(e) => {
                warn!(
                    "Failed to serialize diagnostics for {}: {}",
                    file.display(),
                    e
                );
                continue;
            }
        };

        if let Err(e) = sender.send(Task::Notify(lsp_server::Notification {
            method: lsp_types::notification::PublishDiagnostics::METHOD.to_owned(),
            params,
        })) {
            // Sending back to the main loop failed; propagate error to abort the function
            return Err(e.into());
        }
    }
    Ok(())
}

fn normalize_path_for_diagnostics(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        strip_verbatim_prefix(normalized)
    }

    #[cfg(not(windows))]
    {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }
}

#[cfg(windows)]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use lsp_types::{Position, Range, TextDocumentContentChangeEvent};

    /// Helper to create a test document with UTF-8 content
    fn create_test_document(content: &str) -> Document {
        Document {
            content: ropey::Rope::from_str(content),
            version: 0,
        }
    }

    #[test]
    fn test_utf8_multibyte_character_handling() {
        // Test content with various multi-byte UTF-8 characters
        let content = "2023-01-01 * \"Café ☕\" \"Description with émojis 🎉\"\n  Assets:Cash  -100.00 USD\n  Expenses:Food  100.00 USD\n";
        let mut doc = create_test_document(content);

        // Simulate a change that replaces "Café" with "Restaurant"
        // "Café" has é which is 2 bytes in UTF-8 but 1 UTF-16 code unit
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 15, // Position after quote before C
                },
                end: Position {
                    line: 0,
                    character: 19, // Position after é (4 UTF-16 code units: C, a, f, é)
                },
            }),
            range_length: None,
            text: "Restaurant".to_string(),
        };

        // Calculate the rope positions using the fixed logic
        let start_row_char_idx = doc.content.line_to_char(0);
        let end_row_char_idx = doc.content.line_to_char(0);

        let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);
        let start_col_char_idx =
            doc.content.utf16_cu_to_char(start_line_utf16_cu + 15) - start_row_char_idx;

        let end_line_utf16_cu = doc.content.char_to_utf16_cu(end_row_char_idx);
        let end_col_char_idx =
            doc.content.utf16_cu_to_char(end_line_utf16_cu + 19) - end_row_char_idx;

        let start_char_idx = start_row_char_idx + start_col_char_idx;
        let end_char_idx = end_row_char_idx + end_col_char_idx;

        // Apply the change
        doc.content.remove(start_char_idx..end_char_idx);
        doc.content.insert(start_char_idx, &change.text);

        // Verify the result
        let result = doc.content.to_string();
        assert!(
            result.contains("Restaurant"),
            "Should contain 'Restaurant', got: {}",
            result
        );
        assert!(
            result.contains("☕"),
            "Should preserve emoji ☕, got: {}",
            result
        );
        assert!(
            result.contains("🎉"),
            "Should preserve emoji 🎉, got: {}",
            result
        );
        assert!(
            !result.contains("Café"),
            "Should not contain 'Café' anymore, got: {}",
            result
        );
    }

    #[test]
    fn test_full_document_replacement_with_utf8() {
        let initial_content = "2023-01-01 * \"Test\" \"Test\"\n  Assets:Cash  100.00 USD\n";
        let mut doc = create_test_document(initial_content);

        // Full document replacement (no range specified)
        let new_content =
            "2024-01-01 * \"New café ☕\" \"With émojis 🎉\"\n  Assets:Bank  200.00 EUR\n";
        let _change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: new_content.to_string(),
        };

        // Calculate range for full document replacement
        let end_line = (doc.content.len_lines().saturating_sub(1)) as u32;
        let end_line_len = if doc.content.len_lines() > 0 {
            let last_line = doc.content.line(end_line as usize);
            last_line.len_chars().saturating_sub(1).max(0) as u32
        } else {
            0
        };

        let _range = Range {
            start: Position::new(0, 0),
            end: Position::new(end_line, end_line_len),
        };

        // Apply using the calculated range
        let start_char_idx = 0;
        let end_char_idx = doc.content.len_chars();

        doc.content.remove(start_char_idx..end_char_idx);
        doc.content.insert(start_char_idx, new_content);

        // Verify the result
        let result = doc.content.to_string();
        assert_eq!(result, new_content, "Full document replacement failed");
        assert!(result.contains("café"), "Should contain 'café'");
        assert!(result.contains("☕"), "Should contain emoji ☕");
        assert!(result.contains("🎉"), "Should contain emoji 🎉");
    }

    #[test]
    fn test_emoji_at_edit_boundary() {
        // Test editing near emoji boundaries (common source of bugs)
        let content = "2023-01-01 * \"Before🎉After\" \"Test\"\n  Assets:Cash  100.00 USD\n";
        let mut doc = create_test_document(content);

        // Replace "After" which comes right after an emoji
        // String: 2023-01-01 * "Before🎉After" "Test"
        // The emoji 🎉 is 4 bytes in UTF-8, 2 UTF-16 code units (surrogate pair)
        // UTF-16 positions: After starts at 22, ends at 27
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 22, // UTF-16 position where "After" starts (after the emoji)
                },
                end: Position {
                    line: 0,
                    character: 27, // UTF-16 position where "After" ends
                },
            }),
            range_length: None,
            text: "Modified".to_string(),
        };

        // Calculate positions with fixed logic
        let start_row_char_idx = doc.content.line_to_char(0);
        let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);
        let start_col_char_idx = doc.content.utf16_cu_to_char(
            start_line_utf16_cu + change.range.as_ref().unwrap().start.character as usize,
        ) - start_row_char_idx;

        let end_col_char_idx = doc.content.utf16_cu_to_char(
            start_line_utf16_cu + change.range.as_ref().unwrap().end.character as usize,
        ) - start_row_char_idx;

        let start_char_idx = start_row_char_idx + start_col_char_idx;
        let end_char_idx = start_row_char_idx + end_col_char_idx;

        // Apply the change
        doc.content.remove(start_char_idx..end_char_idx);
        doc.content.insert(start_char_idx, &change.text);

        // Verify the result
        let result = doc.content.to_string();
        assert!(
            result.contains("Before🎉Modified"),
            "Should contain 'Before🎉Modified', got: {}",
            result
        );
        assert!(
            !result.contains("After"),
            "Should not contain 'After', got: {}",
            result
        );
    }

    #[test]
    fn test_asian_characters_handling() {
        // Test with CJK characters (3 bytes in UTF-8, 1 UTF-16 code unit each)
        let content = "2023-01-01 * \"日本語テスト\" \"中文测试\"\n  Assets:Cash  100.00 USD\n";
        let mut doc = create_test_document(content);

        // Replace "日本語" with "にほんご"
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 15, // After opening quote
                },
                end: Position {
                    line: 0,
                    character: 18, // After 3 characters (each is 1 UTF-16 CU)
                },
            }),
            range_length: None,
            text: "にほんご".to_string(),
        };

        // Calculate positions
        let start_row_char_idx = doc.content.line_to_char(0);
        let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);

        let start_col_char_idx =
            doc.content.utf16_cu_to_char(start_line_utf16_cu + 15) - start_row_char_idx;
        let end_col_char_idx =
            doc.content.utf16_cu_to_char(start_line_utf16_cu + 18) - start_row_char_idx;

        let start_char_idx = start_row_char_idx + start_col_char_idx;
        let end_char_idx = start_row_char_idx + end_col_char_idx;

        // Apply change
        doc.content.remove(start_char_idx..end_char_idx);
        doc.content.insert(start_char_idx, &change.text);

        // Verify
        let result = doc.content.to_string();
        assert!(
            result.contains("にほんご"),
            "Should contain 'にほんご', got: {}",
            result
        );
        assert!(
            result.contains("中文测试"),
            "Should preserve '中文测试', got: {}",
            result
        );
    }
}
