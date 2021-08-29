use crate::{core, server};
use dashmap::{
    mapref::one::{Ref, RefMut},
    DashMap,
};
use lspower::lsp;
use tokio::sync::{Mutex, RwLock};

/// A tag representing of the kinds of session resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionResourceKind {
    /// A tag representing a [`core::Document`].
    Document,
    /// A tag representing a [`tree_sitter::Parser`].
    Parser,
    /// A tag representing a [`tree_sitter::Tree`].
    Tree,
}

pub struct Session {
    pub server_capabilities: RwLock<lsp::ServerCapabilities>,
    pub client_capabilities: RwLock<lsp::ClientCapabilities>,
    client: Option<lspower::Client>,
    documents: DashMap<lsp::Url, core::Document>,
    // parsers: DashMap<lsp::Url, Mutex<tree_sitter::Parser>>,
    forest: DashMap<lsp::Url, Mutex<tree_sitter::Tree>>,
}

impl Session {
    /// Create a new [`Session`].
    pub fn new(client: Option<lspower::Client>) -> anyhow::Result<Self> {
        let server_capabilities = RwLock::new(server::capabilities());
        let client_capabilities = RwLock::new(Default::default());
        let documents = DashMap::new();
        // let parsers = DashMap::new();
        let forest = DashMap::new();
        Ok(Session {
            server_capabilities,
            client_capabilities,
            client,
            documents,
            // parsers,
            forest,
        })
    }

    /// Insert a [`core::Document`] into the [`Session`].
    pub fn insert_document(&self, uri: lsp::Url, document: core::Document) -> anyhow::Result<()> {
        let result = self.documents.insert(uri.clone(), document);
        debug_assert!(result.is_none());
        // let result = self.parsers.insert(uri.clone(), Mutex::new(document.parser));
        debug_assert!(result.is_none());
        // let result = self.forest.insert(uri, Mutex::new(document.tree));
        debug_assert!(result.is_none());
        Ok(())
    }

    /// Remove a [`core::Document`] from the [`Session`].
    pub fn remove_document(&self, uri: &lsp::Url) -> anyhow::Result<()> {
        let result = self.documents.remove(uri);
        debug_assert!(result.is_some());
        // let result = self.parsers.remove(uri);
        debug_assert!(result.is_some());
        let result = self.documents.remove(uri);
        debug_assert!(result.is_some());
        Ok(())
    }

    /// Get a reference to the [`core::Text`] for a [`core::Document`] in the [`Session`].
    pub async fn get_document(&self, uri: &lsp::Url) -> anyhow::Result<Ref<'_, lsp::Url, core::Document>> {
        self.documents.get(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Document;
            let uri = uri.clone();
            core::Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    /// Get a mutable reference to the [`core::Text`] for a [`core::Document`] in the [`Session`].
    pub async fn get_mut_document(&self, uri: &lsp::Url) -> anyhow::Result<RefMut<'_, lsp::Url, core::Document>> {
        self.documents.get_mut(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Document;
            let uri = uri.clone();
            core::Error::SessionResourceNotFound { kind, uri }.into()
        })
    }
}
