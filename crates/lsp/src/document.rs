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

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Uri};
    use std::str::FromStr;

    #[test]
    fn test_document_open() {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 1,
                text: "2024-01-01 open Assets:Checking".to_string(),
            },
        };

        let doc = Document::open(params);
        assert_eq!(doc.version, 1);
        assert_eq!(doc.text_string(), "2024-01-01 open Assets:Checking");
    }

    #[test]
    fn test_document_text() {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 2,
                text: "Test content".to_string(),
            },
        };

        let doc = Document::open(params);
        let rope = doc.text();
        assert_eq!(rope.to_string(), "Test content");
    }

    #[test]
    fn test_document_text_string() {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 1,
                text: "Hello\nWorld".to_string(),
            },
        };

        let doc = Document::open(params);
        assert_eq!(doc.text_string(), "Hello\nWorld");
    }

    #[test]
    fn test_document_len_bytes() {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 1,
                text: "12345".to_string(),
            },
        };

        let doc = Document::open(params);
        assert_eq!(doc.len_bytes(), 5);
    }

    #[test]
    fn test_document_is_empty() {
        let params_empty = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 1,
                text: "".to_string(),
            },
        };

        let doc_empty = Document::open(params_empty);
        assert!(doc_empty.is_empty());

        let params_non_empty = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 1,
                text: "content".to_string(),
            },
        };

        let doc_non_empty = Document::open(params_non_empty);
        assert!(!doc_non_empty.is_empty());
    }

    #[test]
    fn test_document_with_multibyte_utf8() {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_str("file:///test.bean").unwrap(),
                language_id: "beancount".to_string(),
                version: 1,
                text: "2024-01-01 * \"Coffee ☕\"".to_string(),
            },
        };

        let doc = Document::open(params);
        assert_eq!(doc.text_string(), "2024-01-01 * \"Coffee ☕\"");
        assert_eq!(doc.len_bytes(), 25); // ☕ is 3 bytes in UTF-8
        assert!(!doc.is_empty());
    }
}
