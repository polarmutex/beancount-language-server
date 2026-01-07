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
        tracing::info!("Document opened: {}", params.text_document.uri.as_str());
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
        tracing::info!("Document saved: {}", params.text_document.uri.as_str());
        text_document::did_save(state, params)
    }

    /// handler for `textDocument/didClose`.
    pub(crate) fn did_close(
        state: &mut LspServerState,
        params: lsp_types::DidCloseTextDocumentParams,
    ) -> Result<()> {
        tracing::info!("Document closed: {}", params.text_document.uri.as_str());
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
                tracing::info!("Completion returned {} items", items.len());
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
        tracing::info!(
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
                tracing::info!("Formatting returned {} text edits", edits.len());
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

    /// handler for `textDocument/willSaveWaitUntil`.
    pub(crate) fn will_save_wait_until(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::WillSaveTextDocumentParams,
    ) -> Result<Option<Vec<lsp_types::TextEdit>>> {
        tracing::info!(
            "WillSaveWaitUntil requested for: {}",
            params.text_document.uri.as_str()
        );

        // Convert WillSaveTextDocumentParams to DocumentFormattingParams
        let formatting_params = lsp_types::DocumentFormattingParams {
            text_document: params.text_document,
            options: lsp_types::FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..Default::default()
            },
            work_done_progress_params: Default::default(),
        };

        match formatting::formatting(snapshot, formatting_params) {
            Ok(Some(edits)) => {
                tracing::info!("WillSaveWaitUntil returned {} text edits", edits.len());
                Ok(Some(edits))
            }
            Ok(None) => {
                tracing::debug!("No formatting changes needed before save");
                Ok(None)
            }
            Err(e) => {
                tracing::error!("WillSaveWaitUntil formatting failed: {}", e);
                Err(e)
            }
        }
    }

    pub(crate) fn handle_references(
        snapshot: LspServerStateSnapshot,
        params: lsp_types::ReferenceParams,
    ) -> Result<Option<Vec<lsp_types::Location>>> {
        tracing::info!(
            "References requested for: {} at {}:{}",
            params.text_document_position.text_document.uri.as_str(),
            params.text_document_position.position.line,
            params.text_document_position.position.character
        );

        match references::references(snapshot, params) {
            Ok(Some(locations)) => {
                tracing::info!("Found {} references", locations.len());
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
        tracing::info!(
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
                tracing::info!("Rename will make {} text edits", change_count);
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
}
