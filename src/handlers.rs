pub mod text_document {
    use crate::core;
    use lsp_text::RopeExt;
    use lspower::lsp;
    use std::sync::Arc;

    /// handler for `textDocument/didOpen`.
    pub async fn did_open(session: Arc<core::Session>, params: lsp::DidOpenTextDocumentParams) -> anyhow::Result<()> {
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

        Ok(())
    }
}
