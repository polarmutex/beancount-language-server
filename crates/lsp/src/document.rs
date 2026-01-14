#[derive(Clone)]
pub struct Document {
    /// The textual content of the document.
    pub content: ropey::Rope,
    /// The document version from the LSP client.
    /// Used to detect out-of-order changes and synchronization issues.
    pub version: i32,
}

impl Document {
    pub fn open(params: lsp_types::DidOpenTextDocumentParams) -> Self {
        let content = ropey::Rope::from(params.text_document.text);
        let version = params.text_document.version;
        Self { content, version }
    }

    pub fn text(&self) -> ropey::Rope {
        self.content.clone()
    }

    /// Get the document text as a single string.
    /// This allocates - use sparingly. Prefer working with the rope directly when possible.
    pub fn text_string(&self) -> String {
        self.content.to_string()
    }

    /// Get the byte length of the document.
    #[inline]
    pub fn len_bytes(&self) -> usize {
        self.content.len_bytes()
    }

    /// Check if the document is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.content.len_bytes() == 0
    }
}
