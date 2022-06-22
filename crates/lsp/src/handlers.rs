pub mod text_document {
    use crate::{core, core::RopeExt, providers, session::Session};
    use log::debug;
    use providers::completion;
    use providers::diagnostics;
    use providers::formatting;
    use std::path::PathBuf;
    use tokio::sync::Mutex;
    use tower_lsp::lsp_types;

    /// handler for `textDocument/didOpen`.
    pub(crate) async fn did_open(
        session: &Session,
        params: lsp_types::DidOpenTextDocumentParams,
    ) -> anyhow::Result<()> {
        debug!("handlers::did_open");
        let uri = params.text_document.uri.clone();

        let document = core::Document::open(params);
        // let tree = document.tree.clone();
        debug!("handlers::did_open - adding {}", uri);
        session.insert_document(uri.clone(), document)?;

        if let Err(err) = check_beancont(&session).await {
            debug!("handlers::did_open -- Error finding diagnostics {}", err.to_string());
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

        if let Err(err) = check_beancont(&session).await {
            debug!("handlers::did_save -- Error finding diagnostics {}", err.to_string());
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
            .map(|change| doc.content.build_edit(change))
            .collect::<Result<Vec<_>, _>>()?;

        debug!("handlers::did_change - apply edits - document");
        for edit in &edits {
            doc.content.apply_edit(edit);
        }

        debug!("handlers::did_change - apply edits - tree");
        let result = {
            let parser = session.get_mut_parser(uri).await?;
            let mut parser = parser.lock().await;

            let old_tree = session.get_mut_tree(uri).await?;
            let mut old_tree = old_tree.lock().await;

            for edit in &edits {
                old_tree.edit(&edit.input_edit);
            }

            let mut callback = {
                let mut content = doc.content.clone();
                content.shrink_to_fit();
                let byte_idx = 0;
                content.chunk_walker(byte_idx).callback_adapter_for_tree_sitter()
            };
            parser.parse_with(&mut callback, Some(&*old_tree))
        };

        debug!("handlers::did_change - save tree");
        if let Some(tree) = result {
            *session.get_mut_tree(uri).await?.value_mut() = Mutex::new(tree.clone());
            session.beancount_data.update_data(uri.clone(), &tree, &doc.content);
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
        debug!("handlers::check_beancount");
        let bean_check_cmd = &PathBuf::from("bean-check");
        // session
        //.bean_check_path
        //.as_ref()
        //.ok_or_else(|| core::Error::InvalidState)?;
        let temp = session.root_journal_path.read().await;
        let root_journal_path = temp.clone().unwrap();

        let diags = diagnostics::diagnostics(
            &session.diagnostic_data,
            &session.beancount_data,
            bean_check_cmd,
            &root_journal_path,
        )
        .await;
        session.diagnostic_data.update(diags.clone());
        for (key, value) in diags {
            session.client.publish_diagnostics(key, value, None).await;
        }
        Ok(())
    }
}
