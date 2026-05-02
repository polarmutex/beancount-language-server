use crate::forest;
use crate::providers::diagnostics;
use crate::server::LspServerState;
use crate::server::LspServerStateSnapshot;
use crate::server::ProgressMsg;
use crate::server::Task;
use crate::to_json;
use anyhow::{Context, Result, anyhow};
use crossbeam_channel::Sender;
use lsp_types::Notification;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::{debug, warn};

/// Process included files recursively from a given beancount file.
///
/// Uses `forest::parse_reachable_includes` (shared logic with the background
/// forest initialiser) and `doc_store.insert_parsed` for consistent state updates.
fn process_includes(
    state: &mut LspServerState,
    file_path: &PathBuf,
    processed: &mut HashSet<PathBuf>,
) -> Result<()> {
    if processed.contains(file_path) {
        return Ok(());
    }
    processed.insert(file_path.clone());

    let tree = match state.doc_store.get_tree(file_path) {
        Some(tree) => tree.clone(),
        None => return Ok(()),
    };

    // Pre-populate already_seen with files already in the forest to skip them.
    let known: Vec<PathBuf> = state.doc_store.forest_keys().cloned().collect();
    processed.extend(known);

    forest::parse_reachable_includes(
        &tree,
        file_path,
        processed,
        &mut |path, new_tree, content| {
            state.doc_store.insert_parsed(path, new_tree, content);
            Ok(())
        },
    )
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

    tracing::debug!("text_document::did_open - adding {:#?}", &uri);
    state.doc_store.open(
        uri.clone(),
        &params.text_document.text,
        params.text_document.version,
    );

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

    if let Ok(uri) = params.text_document.uri.to_file_path() {
        state.doc_store.ensure_beancount_data(&uri);
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
    state.doc_store.close(&uri);
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

        match change.kind {
            lsp_types::FileChangeType::Created | lsp_types::FileChangeType::Changed => {
                tracing::debug!(
                    "External file change detected: {:?} (type: {:?})",
                    uri,
                    change.kind
                );

                // Skip if file is currently open in editor (editor manages its own state)
                if state.doc_store.has_open_doc(&uri) {
                    tracing::debug!("Skipping {:?} - file is open in editor", uri);
                    continue;
                }

                state.doc_store.invalidate_external(&uri);
                tracing::debug!("Cleared stale cache for {:?}", uri);

                if let Ok(content) = fs::read_to_string(&uri)
                    && let Some(tree) = crate::treesitter_utils::parse_beancount(&content)
                {
                    state.doc_store.insert_parsed(uri.clone(), tree, &content);
                    tracing::debug!("Re-parsed external file: {:?}", uri);
                }
            }
            lsp_types::FileChangeType::Deleted => {
                tracing::debug!("External file deleted: {:?}", uri);
                state.doc_store.remove_external(&uri);
            }
        }
    }

    // Trigger diagnostics refresh for open documents
    if state.config.journal_root.is_some() {
        let snapshot = state.snapshot();
        let task_sender = state.task_sender.clone();

        // Find an open document to use for diagnostics URI
        if let Some(open_uri) = state.doc_store.open_doc_keys().next().cloned() {
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
    let uri = match params
        .text_document
        .text_document_identifier
        .uri
        .to_file_path()
    {
        Ok(path) => path,
        Err(_) => {
            debug!(
                "Failed to convert URI to file path: {:?}",
                params.text_document.text_document_identifier.uri
            );
            return Ok(());
        }
    };
    tracing::debug!("text_document::did_change - requesting {:#?}", uri);
    state
        .doc_store
        .apply_change(&uri, &params.content_changes, params.text_document.version)?;
    debug!("text_document::did_change - done");
    Ok(())
}

fn handle_diagnostics(
    snapshot: LspServerStateSnapshot,
    sender: Sender<Task>,
    uri: lsp_types::Uri,
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
        Some(path) => {
            tracing::debug!("Using configured journal_root: {}", path.display());
            path
        }
        None => {
            // Fallback to using the current file as the root journal
            let path = uri
                .to_file_path()
                .map_err(|_| anyhow!("Failed to convert URI to file path: {}", uri.as_str()))?;
            tracing::debug!(
                "No journal_root configured; using current file as root: {}",
                path.display()
            );
            path
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
        &snapshot.beancount_data,
        checker.as_ref(),
        &root_journal_path,
        &snapshot.config.diagnostic_flags,
    );

    sender.send(Task::Progress(ProgressMsg::BeanCheck {
        done: 1,
        total: 1,
        checker_name: checker.name().to_string(),
        run_id,
    }))?;

    publish_diagnostics(diags, snapshot.forest.keys().cloned(), &sender)
}

fn publish_diagnostics(
    diags: HashMap<PathBuf, Vec<lsp_types::Diagnostic>>,
    forest_keys: impl Iterator<Item = PathBuf>,
    sender: &Sender<Task>,
) -> Result<()> {
    let mut normalized: HashMap<PathBuf, Vec<lsp_types::Diagnostic>> = HashMap::new();
    for (path, diagnostics) in diags {
        let key = normalize_path_for_diagnostics(&path);
        normalized.entry(key).or_default().extend(diagnostics);
    }

    for file in forest_keys {
        let lookup = normalize_path_for_diagnostics(&file);
        let diagnostics = normalized.remove(&lookup).unwrap_or_default();
        sender
            .send(Task::Notify(lsp_server::Notification {
                method: lsp_types::PublishDiagnosticsNotification::METHOD.to_string(),
                params: to_json(lsp_types::PublishDiagnosticsParams {
                    uri: {
                        let url = url::Url::from_file_path(&file).map_err(|()| {
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
    for (file, diagnostics) in normalized {
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
            method: lsp_types::PublishDiagnosticsNotification::METHOD.to_string(),
            params,
        })) {
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
    use lsp_types::{Position, Range, TextDocumentContentChangePartial};

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
        let change = TextDocumentContentChangePartial {
            range: Range {
                start: Position {
                    line: 0,
                    character: 15, // Position after quote before C
                },
                end: Position {
                    line: 0,
                    character: 19, // Position after é (4 UTF-16 code units: C, a, f, é)
                },
            },
            text: "Restaurant".to_string(),
            ..Default::default()
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

        // Calculate range for full document replacement
        let end_line = (doc.content.len_lines().saturating_sub(1)) as u32;
        let end_line_len = if doc.content.len_lines() > 0 {
            let last_line = doc.content.line(end_line as usize);
            last_line.len_chars().saturating_sub(1) as u32
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
        let change = TextDocumentContentChangePartial {
            range: Range {
                start: Position {
                    line: 0,
                    character: 22, // UTF-16 position where "After" starts (after the emoji)
                },
                end: Position {
                    line: 0,
                    character: 27, // UTF-16 position where "After" ends
                },
            },
            text: "Modified".to_string(),
            ..Default::default()
        };

        // Calculate positions with fixed logic
        let start_row_char_idx = doc.content.line_to_char(0);
        let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);
        let start_col_char_idx = doc
            .content
            .utf16_cu_to_char(start_line_utf16_cu + change.range.start.character as usize)
            - start_row_char_idx;

        let end_col_char_idx = doc
            .content
            .utf16_cu_to_char(start_line_utf16_cu + change.range.end.character as usize)
            - start_row_char_idx;

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
        // Replace "日本語" with "にほんご"
        doc.content.insert(start_char_idx, "にほんご");

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

    #[test]
    fn test_out_of_bounds_utf16_position() {
        // Test that out-of-bounds UTF-16 positions are clamped instead of panicking
        // This reproduces issue #820 where neovim sends positions beyond document bounds
        let content = "2023-01-01 * \"Test\"\n";
        let doc = create_test_document(content);
        let total_utf16_len = doc.content.len_utf16_cu();

        // Simulate a change with end position beyond document bounds
        let start_row_char_idx = doc.content.line_to_char(0);
        let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);

        // This should not panic - it should clamp to document end
        let out_of_bounds_utf16 = total_utf16_len + 100;
        let clamped_utf16 = out_of_bounds_utf16.min(doc.content.len_utf16_cu());
        let result = doc
            .content
            .utf16_cu_to_char(start_line_utf16_cu + clamped_utf16);

        // Should succeed and clamp to valid position
        assert!(
            result <= doc.content.len_chars(),
            "Should clamp to valid char position"
        );
    }

    #[test]
    fn test_handle_diagnostics_without_journal_root() {
        // Regression test for issue #822
        // Verify that diagnostics work even when journal_root is not configured
        use super::handle_diagnostics;
        use crate::checkers::SystemCallChecker;
        use crate::config::Config;
        use crate::server::LspServerStateSnapshot;
        use crossbeam_channel;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::str::FromStr;
        use std::sync::Arc;

        // Create a temporary test file
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("test.beancount");
        std::fs::write(&test_file, "2023-01-01 open Assets:Cash\n").unwrap();

        // Create URI from the test file
        let url = url::Url::from_file_path(&test_file).unwrap();
        let uri = lsp_types::Uri::from_str(url.as_ref()).unwrap();

        // Create config WITHOUT journal_root (this is the bug scenario)
        let mut config = Config::new(temp_dir.path().to_path_buf());
        config.journal_root = None; // Explicitly set to None

        // Create a mock checker that succeeds
        let checker = SystemCallChecker::new(PathBuf::from("/bin/true"));

        // Create snapshot
        let snapshot = LspServerStateSnapshot {
            beancount_data: Arc::new(HashMap::new()),
            config,
            forest: Arc::new(HashMap::new()),
            forest_content: Arc::new(HashMap::new()),
            open_docs: Arc::new(HashMap::new()),
            checker: Some(Arc::new(checker)),
        };

        // Create channel for task communication using crossbeam_channel
        let (sender, receiver) = crossbeam_channel::unbounded();

        // Call handle_diagnostics - this should NOT skip diagnostics
        let result = handle_diagnostics(snapshot, sender, uri.clone());

        // The function should succeed (not return error about missing journal_root)
        assert!(
            result.is_ok(),
            "handle_diagnostics should succeed without journal_root configured"
        );

        // Verify that tasks were sent (diagnostics were run, not skipped)
        let mut task_count = 0;
        while let Ok(_task) = receiver.try_recv() {
            task_count += 1;
        }

        // Should have at least progress messages (start + end)
        assert!(
            task_count > 0,
            "Should send tasks when running diagnostics (got {} tasks)",
            task_count
        );
    }

    #[test]
    fn test_handle_diagnostics_with_journal_root() {
        // Verify that diagnostics still work correctly when journal_root IS configured
        use super::handle_diagnostics;
        use crate::checkers::SystemCallChecker;
        use crate::config::Config;
        use crate::server::LspServerStateSnapshot;
        use crossbeam_channel;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::str::FromStr;
        use std::sync::Arc;

        // Create temporary test files
        let temp_dir = tempfile::tempdir().unwrap();
        let root_journal = temp_dir.path().join("main.beancount");
        let other_file = temp_dir.path().join("accounts.beancount");
        std::fs::write(&root_journal, "2023-01-01 open Assets:Cash\n").unwrap();
        std::fs::write(&other_file, "2023-01-01 open Assets:Bank\n").unwrap();

        // Create URI from the other file (not the root)
        let url = url::Url::from_file_path(&other_file).unwrap();
        let uri = lsp_types::Uri::from_str(url.as_ref()).unwrap();

        // Create config WITH journal_root (traditional multi-file setup)
        let mut config = Config::new(temp_dir.path().to_path_buf());
        config.journal_root = Some(root_journal.clone());

        // Create a mock checker
        let checker = SystemCallChecker::new(PathBuf::from("/bin/true"));

        // Create snapshot
        let snapshot = LspServerStateSnapshot {
            beancount_data: Arc::new(HashMap::new()),
            config,
            forest: Arc::new(HashMap::new()),
            forest_content: Arc::new(HashMap::new()),
            open_docs: Arc::new(HashMap::new()),
            checker: Some(Arc::new(checker)),
        };

        // Create channel for task communication using crossbeam_channel
        let (sender, receiver) = crossbeam_channel::unbounded();

        // Call handle_diagnostics with a different file than journal_root
        let result = handle_diagnostics(snapshot, sender, uri.clone());

        // Should succeed
        assert!(
            result.is_ok(),
            "handle_diagnostics should succeed with journal_root configured"
        );

        // Verify that tasks were sent
        let mut task_count = 0;
        while let Ok(_task) = receiver.try_recv() {
            task_count += 1;
        }

        assert!(
            task_count > 0,
            "Should send tasks when running diagnostics with journal_root"
        );
    }

    #[test]
    fn test_handle_diagnostics_without_checker() {
        // Verify that diagnostics gracefully handle missing checker
        use super::handle_diagnostics;
        use crate::config::Config;
        use crate::server::LspServerStateSnapshot;
        use crossbeam_channel;
        use std::collections::HashMap;
        use std::str::FromStr;
        use std::sync::Arc;

        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("test.beancount");
        std::fs::write(&test_file, "2023-01-01 open Assets:Cash\n").unwrap();

        let url = url::Url::from_file_path(&test_file).unwrap();
        let uri = lsp_types::Uri::from_str(url.as_ref()).unwrap();

        let config = Config::new(temp_dir.path().to_path_buf());

        // Create snapshot WITHOUT checker
        let snapshot = LspServerStateSnapshot {
            beancount_data: Arc::new(HashMap::new()),
            config,
            forest: Arc::new(HashMap::new()),
            forest_content: Arc::new(HashMap::new()),
            open_docs: Arc::new(HashMap::new()),
            checker: None,
        };

        let (sender, _receiver) = crossbeam_channel::unbounded();

        // Should succeed but skip diagnostics
        let result = handle_diagnostics(snapshot, sender, uri);

        assert!(result.is_ok(), "Should handle missing checker gracefully");
    }
}
