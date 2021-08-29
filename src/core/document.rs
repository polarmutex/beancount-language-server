use lspower::lsp;

#[derive(Clone)]
pub struct Document {
    /// The textual content of the document.
    pub content: ropey::Rope,
}

impl Document {
    pub fn open(params: lsp::DidOpenTextDocumentParams) -> Self {
        let content = ropey::Rope::from(params.text_document.text);
        let content = content.clone();
        Self { content }
    }

    pub fn text(&self) -> ropey::Rope {
        self.content.clone()
    }
}
