use crate::{beancount_data::BeancountData, document::Document, error::Error, providers};
use dashmap::{
    mapref::one::{Ref, RefMut},
    DashMap,
};
use log::debug;
use providers::diagnostics;
use serde::{Deserialize, Serialize};
use std::path;
use tokio::sync::{Mutex, RwLock};
use tower_lsp::lsp_types;

/// A tag representing of the kinds of session resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionResourceKind {
    /// A tag representing a [`Document`].
    Document,
    /// A tag representing a [`tree_sitter::Parser`].
    Parser,
    /// A tag representing a [`tree_sitter::Tree`].
    Tree,
}

pub(crate) struct Session {
    //pub(crate) server_capabilities: RwLock<lsp_types::ServerCapabilities>,
    pub(crate) client_capabilities: RwLock<Option<lsp_types::ClientCapabilities>>,
    pub(crate) client: tower_lsp::Client,
    pub(crate) documents: DashMap<lsp_types::Url, Document>,
    pub(crate) parsers: DashMap<lsp_types::Url, Mutex<tree_sitter::Parser>>,
    pub(crate) forest: DashMap<lsp_types::Url, Mutex<tree_sitter::Tree>>,
    pub(crate) root_journal_path: RwLock<Option<path::PathBuf>>,
    //pub(crate) bean_check_path: Option<PathBuf>,
    pub(crate) beancount_data: BeancountData,
    pub(crate) diagnostic_data: diagnostics::DiagnosticData,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountLspOptions {
    pub journal_file: String,
}

impl Session {
    /// Create a new [`Session`].
    pub fn new(client: tower_lsp::Client) -> Self {
        //let server_capabilities = RwLock::new(server::capabilities());
        let client_capabilities = RwLock::new(Default::default());
        let documents = DashMap::new();
        let parsers = DashMap::new();
        let forest = DashMap::new();
        let beancount_data = BeancountData::new();
        let diagnostic_data = diagnostics::DiagnosticData::new();

        //let bean_check_path = env::var_os("PATH").and_then(|paths| {
        //    env::split_paths(&paths).find_map(|p| {
        //        let full_path = p.join("bean-check");

        //        if full_path.is_file() {
        //            Some(full_path)
        //        } else {
        //            None
        //        }
        //    })
        //});

        let root_journal_path = RwLock::new(None);

        Self {
            //server_capabilities,
            client_capabilities,
            client,
            documents,
            parsers,
            forest,
            root_journal_path,
            //bean_check_path,
            beancount_data,
            diagnostic_data,
        }
    }

    /// Insert a [`Document`] into the [`Session`].
    pub fn insert_document(&self, uri: lsp_types::Url, document: Document) -> anyhow::Result<()> {
        let result = self.documents.insert(uri, document);
        debug_assert!(result.is_none());
        // let result = self.parsers.insert(uri.clone(), Mutex::new(document.parser));
        // debug_assert!(result.is_none());
        // let result = self.forest.insert(uri, Mutex::new(document.tree));
        // debug_assert!(result.is_none());
        Ok(())
    }

    /// Remove a [`core::Document`] from the [`Session`].
    pub fn remove_document(&self, uri: &lsp_types::Url) -> anyhow::Result<()> {
        let result = self.documents.remove(uri);
        debug_assert!(result.is_some());
        // let result = self.parsers.remove(uri);
        // debug_assert!(result.is_some());
        // let result = self.documents.remove(uri);
        // debug_assert!(result.is_some());
        Ok(())
    }

    /// Get a reference to the [`core::Document`] in the [`Session`].
    pub async fn get_document(&self, uri: &lsp_types::Url) -> anyhow::Result<Ref<'_, lsp_types::Url, Document>> {
        self.documents.get(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Document;
            let uri = uri.clone();
            Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    /// Get a mutable reference to the [`core::Document`] in the [`Session`].
    pub async fn get_mut_document(&self, uri: &lsp_types::Url) -> anyhow::Result<RefMut<'_, lsp_types::Url, Document>> {
        self.documents.get_mut(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Document;
            let uri = uri.clone();
            Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    pub async fn get_mut_parser(
        &self,
        uri: &lsp_types::Url,
    ) -> anyhow::Result<RefMut<'_, lsp_types::Url, Mutex<tree_sitter::Parser>>> {
        debug!("getting mutable parser {}", uri);
        debug!("parser contains key {}", self.parsers.contains_key(uri));
        self.parsers.get_mut(uri).ok_or_else(|| {
            debug!("Error getting mutable parser");
            let kind = SessionResourceKind::Parser;
            let uri = uri.clone();
            Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    /// Get a reference to the [`tree_sitter::Tree`] for a [`Document`] in the [`Session`].
    //pub async fn get_tree(&self, uri: &lsp::Url) -> anyhow::Result<Ref<'_, lsp::Url, Mutex<tree_sitter::Tree>>> {
    //    self.forest.get(uri).ok_or_else(|| {
    //        let kind = SessionResourceKind::Tree;
    //        let uri = uri.clone();
    //        Error::SessionResourceNotFound { kind, uri }.into()
    //    })
    //}

    /// Get a mutable reference to the [`tree_sitter::Tree`] for a [`Document`] in the
    /// [`Session`].
    pub async fn get_mut_tree(
        &self,
        uri: &lsp_types::Url,
    ) -> anyhow::Result<RefMut<'_, lsp_types::Url, Mutex<tree_sitter::Tree>>> {
        self.forest.get_mut(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Tree;
            let uri = uri.clone();
            Error::SessionResourceNotFound { kind, uri }.into()
        })
    }
}
