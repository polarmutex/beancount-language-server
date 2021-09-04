pub mod text_document {
    use crate::{core, core::RopeExt};
    use log::debug;
    use lspower::lsp;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// handler for `textDocument/didOpen`.
    pub async fn did_open(session: Arc<core::Session>, params: lsp::DidOpenTextDocumentParams) -> anyhow::Result<()> {
        debug!("handlers::did_open");
        let uri = params.text_document.uri.clone();

        let document = core::Document::open(params);
        // let tree = document.tree.clone();
        let text = document.text();
        session.insert_document(uri.clone(), document)?;
        // let diagnostics = provider::diagnostics(&tree, &text);
        // let version = Default::default();
        // session.client()?.publish_diagnostics(uri, diagnostics, version).await;

        Ok(())
    }

    // handler for `textDocument/didClose`.
    pub async fn did_close(session: Arc<core::Session>, params: lsp::DidCloseTextDocumentParams) -> anyhow::Result<()> {
        debug!("handlers::did_close");
        let uri = params.text_document.uri;
        session.remove_document(&uri)?;
        // let version = Default::default();
        Ok(())
    }

    // handler for `textDocument/didChange`.
    pub async fn did_change(
        session: Arc<core::Session>,
        params: lsp::DidChangeTextDocumentParams,
    ) -> anyhow::Result<()> {
        debug!("handlers::did_change");
        let uri = &params.text_document.uri;
        let mut doc = session.get_mut_document(uri).await?;

        let edits = params
            .content_changes
            .iter()
            .map(|change| doc.content.build_edit(change))
            .collect::<Result<Vec<_>, _>>()?;

        for edit in &edits {
            doc.content.apply_edit(edit);
        }

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

        debug!("handlers::did_close - save tree");
        if let Some(tree) = result {
            *session.get_mut_tree(uri).await?.value_mut() = Mutex::new(tree.clone());
        }

        debug!("handlers::did_close - done");
        Ok(())
    }
}
