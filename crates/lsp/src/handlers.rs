pub mod text_document {
    use crate::{document::Document, progress, providers, session::Session};
    use log::debug;
    use providers::completion;
    use providers::diagnostics;
    use providers::formatting;
    use std::path::PathBuf;
    use tokio::sync::Mutex;
    use tower_lsp::lsp_types;
    use tree_sitter_utils::lsp_utils::lsp_textdocchange_to_ts_inputedit;

    /// handler for `textDocument/didOpen`.
    pub(crate) async fn did_open(
        session: &Session,
        params: lsp_types::DidOpenTextDocumentParams,
    ) -> anyhow::Result<()> {
        debug!("handlers::did_open");
        let uri = params.text_document.uri.clone();

        let document = Document::open(params);
        // let tree = document.tree.clone();
        debug!("handlers::did_open - adding {}", uri);
        session.insert_document(uri.clone(), document)?;

        if let Err(err) = check_beancont(session).await {
            debug!(
                "handlers::did_open -- Error finding diagnostics {}",
                err.to_string()
            );
            session
                .client
                .log_message(lsp_types::MessageType::ERROR, err.to_string())
                .await;
        }

        Ok(())
    }

    /// handler for `textDocument/didSave`.
    pub(crate) async fn did_save(
        session: &Session,
        _params: lsp_types::DidSaveTextDocumentParams,
    ) -> anyhow::Result<()> {
        debug!("handlers::did_save");

        if let Err(err) = check_beancont(session).await {
            debug!(
                "handlers::did_save -- Error finding diagnostics {}",
                err.to_string()
            );
            session
                .client
                .log_message(lsp_types::MessageType::ERROR, err.to_string())
                .await;
        }

        Ok(())
    }

    // handler for `textDocument/didClose`.
    pub(crate) async fn did_close(
        session: &Session,
        params: lsp_types::DidCloseTextDocumentParams,
    ) -> anyhow::Result<()> {
        debug!("handlers::did_close");
        let uri = params.text_document.uri;
        session.remove_document(&uri)?;
        // let version = Default::default();
        Ok(())
    }

    // handler for `textDocument/didChange`.
    pub(crate) async fn did_change(
        session: &Session,
        params: lsp_types::DidChangeTextDocumentParams,
    ) -> anyhow::Result<()> {
        debug!("handlers::did_change");
        let uri = &params.text_document.uri;
        debug!("handlers::did_change - requesting {}", uri);
        let mut doc = session.get_mut_document(uri).await?;

        debug!("handlers::did_change - convert edits");
        let edits = params
            .content_changes
            .iter()
            .map(|change| lsp_textdocchange_to_ts_inputedit(&doc.content, change))
            .collect::<Result<Vec<_>, _>>()?;

        debug!("handlers::did_change - apply edits - document");
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

        debug!("handlers::did_change - apply edits - tree");
        let result = {
            let parser = session.get_mut_parser(uri).await?;
            let mut parser = parser.lock().await;

            let old_tree = session.get_mut_tree(uri).await?;
            let mut old_tree = old_tree.lock().await;

            for edit in &edits {
                old_tree.edit(edit);
            }

            parser.parse(doc.text().to_string(), Some(&old_tree))
        };

        debug!("handlers::did_change - save tree");
        if let Some(tree) = result {
            *session.get_mut_tree(uri).await?.value_mut() = Mutex::new(tree.clone());
            session
                .beancount_data
                .update_data(uri.clone(), &tree, &doc.content);
        }

        debug!("handlers::did_close - done");
        Ok(())
    }

    pub(crate) async fn completion(
        session: &Session,
        params: lsp_types::CompletionParams,
    ) -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
        completion::completion(session, params).await
    }

    pub(crate) async fn formatting(
        session: &Session,
        params: lsp_types::DocumentFormattingParams,
    ) -> anyhow::Result<Option<Vec<lsp_types::TextEdit>>> {
        formatting::formatting(session, params).await
    }

    async fn check_beancont(session: &Session) -> anyhow::Result<()> {
        let progress_token = progress::progress_begin(&session.client, "bean-check").await;
        debug!("handlers::check_beancount");
        let bean_check_cmd = &PathBuf::from("bean-check");
        // session
        //.bean_check_path
        //.as_ref()
        //.ok_or_else(|| core::Error::InvalidState)?;
        let root_journal_path = &session.root_journal_path.read().await.clone().unwrap();

        let diags = diagnostics::diagnostics(
            &session.diagnostic_data,
            &session.beancount_data,
            bean_check_cmd,
            root_journal_path,
        )
        .await;
        session.diagnostic_data.update(diags.clone());
        for (key, value) in diags {
            session.client.publish_diagnostics(key, value, None).await;
        }
        progress::progress_end(&session.client, progress_token).await;
        Ok(())
    }
}
