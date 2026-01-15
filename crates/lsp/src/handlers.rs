pub mod text_document {
    use crate::providers::completion;
    use crate::providers::formatting;
    use crate::providers::references;
    use crate::providers::semantic_tokens;
    use crate::providers::text_document;
    use crate::server::LspServerState;
    use crate::server::LspServerStateSnapshot;
    use anyhow::Result;

    /// handler for `textDocument/didOpen`.
    pub(crate) fn did_open(
        state: &mut LspServerState,
        params: lsp_types::DidOpenTextDocumentParams,
    ) -> Result<()> {
        tracing::trace!("Document opened: {}", params.text_document.uri.as_str());
        tracing::debug!(
            "Document language: {}, version: {}",
            params.text_document.language_id,
            params.text_document.version
        );
        text_document::did_open(state, params)
    }

    /// handler for `textDocument/didSave`.
    pub(crate) fn did_save(
        state: &mut LspServerState,
        params: lsp_types::DidSaveTextDocumentParams,
    ) -> Result<()> {
        tracing::trace!("Document saved: {}", params.text_document.uri.as_str());
        text_document::did_save(state, params)
    }

    /// handler for `textDocument/didClose`.
    pub(crate) fn did_close(
        state: &mut LspServerState,
        params: lsp_types::DidCloseTextDocumentParams,
    ) -> Result<()> {
        tracing::trace!("Document closed: {}", params.text_document.uri.as_str());
        text_document::did_close(state, params)
    }

    /// handler for `textDocument/didChange`.
    pub(crate) fn did_change(
        state: &mut LspServerState,
        params: lsp_types::DidChangeTextDocumentParams,
    ) -> Result<()> {
        tracing::debug!(
            "Document changed: {}, version: {}",
            params.text_document.uri.as_str(),
            params.text_document.version
        );
        tracing::debug!(
            "Number of content changes: {}",
            params.content_changes.len()
        );
        text_document::did_change(state, params)
    }

    pub(crate) fn completion(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::CompletionParams,
    ) -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
        tracing::debug!(
            "Completion requested for: {} at {}:{}",
            params.text_document_position.text_document.uri.as_str(),
            params.text_document_position.position.line,
            params.text_document_position.position.character
        );

        let trigger_char = match &params.context {
            Some(context) => match &context.trigger_character {
                Some(trigger_character) => {
                    tracing::debug!("Completion triggered by character: '{}'", trigger_character);
                    if trigger_character == "2" {
                        if params.text_document_position.position.character > 1 {
                            None
                        } else {
                            trigger_character.chars().last()
                        }
                    } else {
                        trigger_character.chars().last()
                    }
                }
                None => {
                    tracing::debug!("Completion triggered manually (no trigger character)");
                    None
                }
            },
            None => {
                tracing::debug!("Completion triggered manually (no context)");
                None
            }
        };

        match completion::completion(snapshot, trigger_char, params.text_document_position) {
            Ok(Some(items)) => {
                tracing::trace!("Completion returned {} items", items.len());
                // Return CompletionList instead of Array to signal that server-side
                // filtering is preferred. Setting `is_incomplete: true` tells clients
                // like Zed to re-query on each keystroke rather than filtering internally.
                Ok(Some(lsp_types::CompletionResponse::List(
                    lsp_types::CompletionList {
                        is_incomplete: true,
                        items,
                    },
                )))
            }
            Ok(None) => {
                tracing::debug!("No completion items available");
                Ok(None)
            }
            Err(e) => {
                tracing::error!("Completion failed: {}", e);
                Err(e)
            }
        }
    }

    pub(crate) fn formatting(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::DocumentFormattingParams,
    ) -> Result<Option<Vec<lsp_types::TextEdit>>> {
        tracing::trace!(
            "Formatting requested for: {}",
            params.text_document.uri.as_str()
        );
        tracing::debug!(
            "Formatting options: tab_size={}, insert_spaces={}",
            params.options.tab_size,
            params.options.insert_spaces
        );

        match formatting::formatting(snapshot, params) {
            Ok(Some(edits)) => {
                tracing::trace!("Formatting returned {} text edits", edits.len());
                Ok(Some(edits))
            }
            Ok(None) => {
                tracing::debug!("No formatting changes needed");
                Ok(None)
            }
            Err(e) => {
                tracing::error!("Formatting failed: {}", e);
                Err(e)
            }
        }
    }

    pub(crate) fn handle_references(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::ReferenceParams,
    ) -> Result<Option<Vec<lsp_types::Location>>> {
        tracing::trace!(
            "References requested for: {} at {}:{}",
            params.text_document_position.text_document.uri.as_str(),
            params.text_document_position.position.line,
            params.text_document_position.position.character
        );

        match references::references(snapshot, params) {
            Ok(Some(locations)) => {
                tracing::trace!("Found {} references", locations.len());
                Ok(Some(locations))
            }
            Ok(None) => {
                tracing::debug!("No references found");
                Ok(None)
            }
            Err(e) => {
                tracing::error!("References lookup failed: {}", e);
                Err(e)
            }
        }
    }

    pub(crate) fn handle_rename(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::RenameParams,
    ) -> Result<Option<lsp_types::WorkspaceEdit>> {
        tracing::trace!(
            "Rename requested for: {} at {}:{} to '{}'",
            params.text_document_position.text_document.uri.as_str(),
            params.text_document_position.position.line,
            params.text_document_position.position.character,
            params.new_name
        );

        match references::rename(snapshot, params) {
            Ok(Some(workspace_edit)) => {
                let change_count = workspace_edit
                    .changes
                    .as_ref()
                    .map(|changes| changes.values().map(|edits| edits.len()).sum::<usize>())
                    .unwrap_or(0);
                tracing::trace!("Rename will make {} text edits", change_count);
                Ok(Some(workspace_edit))
            }
            Ok(None) => {
                tracing::debug!("No rename edits generated");
                Ok(None)
            }
            Err(e) => {
                tracing::error!("Rename failed: {}", e);
                Err(e)
            }
        }
    }

    pub(crate) fn semantic_tokens_full(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::SemanticTokensParams,
    ) -> Result<Option<lsp_types::SemanticTokensResult>> {
        tracing::debug!(
            "Semantic tokens requested for: {}",
            params.text_document.uri.as_str()
        );
        semantic_tokens::semantic_tokens_full(snapshot, params)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::beancount_data::BeancountData;
        use crate::config::Config;
        use crate::document::Document;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::str::FromStr;
        use std::sync::Arc;
        use tree_sitter_beancount::tree_sitter;
        use url::Url;

        struct TestState {
            snapshot: LspServerStateSnapshot,
            path: PathBuf,
        }

        impl TestState {
            fn new(content: &str) -> anyhow::Result<Self> {
                let path = std::env::current_dir()?.join("test.beancount");
                let rope_content = ropey::Rope::from_str(content);

                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&tree_sitter_beancount::language())?;
                let tree = parser.parse(content, None).unwrap();

                let mut forest = HashMap::new();
                forest.insert(path.clone(), Arc::new(tree.clone()));

                let mut open_docs = HashMap::new();
                open_docs.insert(
                    path.clone(),
                    Document {
                        content: rope_content.clone(),
                        version: 0,
                    },
                );

                let mut beancount_data = HashMap::new();
                beancount_data.insert(
                    path.clone(),
                    Arc::new(BeancountData::new(&tree, &rope_content)),
                );

                let config = Config::new(path.clone());

                Ok(Self {
                    snapshot: LspServerStateSnapshot {
                        forest,
                        open_docs,
                        beancount_data,
                        config,
                    },
                    path,
                })
            }
        }

        #[test]
        fn test_formatting_handler() {
            let content = "2024-01-01 open Assets:Checking\n2024-01-02 * \"Test\"\n  Assets:Checking  100.00 USD\n";
            let state = TestState::new(content).unwrap();

            let uri = lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref())
                .unwrap();
            let params = lsp_types::DocumentFormattingParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                options: lsp_types::FormattingOptions {
                    tab_size: 2,
                    insert_spaces: true,
                    ..Default::default()
                },
                work_done_progress_params: Default::default(),
            };

            let result = formatting(state.snapshot, params);
            assert!(result.is_ok());
        }

        #[test]
        fn test_completion_handler() {
            let content =
                "2024-01-01 open Assets:Checking\n2024-01-02 * \"Test\" \"Test\"\n  Assets:Che";
            let state = TestState::new(content).unwrap();

            let uri = lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref())
                .unwrap();
            let params = lsp_types::CompletionParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                    position: lsp_types::Position::new(2, 12),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            };

            let result = completion(state.snapshot, params);
            assert!(result.is_ok());
        }

        #[test]
        fn test_completion_handler_with_trigger() {
            let content = "2024-01-01 open Assets:Checking\n";
            let state = TestState::new(content).unwrap();

            let uri = lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref())
                .unwrap();
            let params = lsp_types::CompletionParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                    position: lsp_types::Position::new(1, 0),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: Some(lsp_types::CompletionContext {
                    trigger_kind: lsp_types::CompletionTriggerKind::TRIGGER_CHARACTER,
                    trigger_character: Some("2".to_string()),
                }),
            };

            let result = completion(state.snapshot, params);
            assert!(result.is_ok());
        }

        #[test]
        fn test_references_handler() {
            let content = "2024-01-01 open Assets:Checking\n2024-01-02 * \"Test\"\n  Assets:Checking  100.00 USD\n";
            let state = TestState::new(content).unwrap();

            let uri = lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref())
                .unwrap();
            let params = lsp_types::ReferenceParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                    position: lsp_types::Position::new(0, 20),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: lsp_types::ReferenceContext {
                    include_declaration: true,
                },
            };

            let result = handle_references(state.snapshot, params);
            assert!(result.is_ok());
            assert!(result.unwrap().is_some());
        }

        #[test]
        fn test_rename_handler() {
            let content = "2024-01-01 open Assets:Checking\n2024-01-02 * \"Test\"\n  Assets:Checking  100.00 USD\n";
            let state = TestState::new(content).unwrap();

            let uri = lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref())
                .unwrap();
            let params = lsp_types::RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                    position: lsp_types::Position::new(0, 20),
                },
                new_name: "Assets:Bank".to_string(),
                work_done_progress_params: Default::default(),
            };

            let result = handle_rename(state.snapshot, params);
            assert!(result.is_ok());
            let edit = result.unwrap();
            assert!(edit.is_some());
            let changes = edit.unwrap().changes;
            assert!(changes.is_some());
        }

        #[test]
        fn test_semantic_tokens_handler() {
            let content = "2024-01-01 open Assets:Checking\n";
            let state = TestState::new(content).unwrap();

            let uri = lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref())
                .unwrap();
            let params = lsp_types::SemanticTokensParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };

            let result = semantic_tokens_full(state.snapshot, params);
            assert!(result.is_ok());
        }
    }
}
