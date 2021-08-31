use crate::{core, server};
use dashmap::{
    mapref::one::{Ref, RefMut},
    DashMap,
};
use lspower::lsp;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::read_to_string, path::Path};
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
    pub client_capabilities: RwLock<Option<lsp::ClientCapabilities>>,
    client: Option<lspower::Client>,
    documents: DashMap<lsp::Url, core::Document>,
    parser: Mutex<tree_sitter::Parser>,
    forest: DashMap<lsp::Url, tree_sitter::Tree>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BeancountLspOptions {
    pub journal_file: String,
}

impl Session {
    /// Create a new [`Session`].
    pub fn new(client: Option<lspower::Client>) -> anyhow::Result<Self> {
        let server_capabilities = RwLock::new(server::capabilities());
        let client_capabilities = RwLock::new(Default::default());
        let documents = DashMap::new();
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser.set_language(tree_sitter_beancount::language())?;
        let parser = Mutex::new(ts_parser);
        let forest = DashMap::new();
        Ok(Session {
            server_capabilities,
            client_capabilities,
            client,
            documents,
            parser,
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

    // Issus to look at if running into issues with this
    // https://github.com/silvanshade/lspower/issues/8
    async fn find_includes(&self, file_str: String, seen_files: HashSet<String>) -> anyhow::Result<HashSet<String>> {
        let text = read_to_string(&file_str).unwrap();
        let bytes = text.as_bytes();

        let mut parser = self.parser.lock().await;
        let tree = parser.parse(&text, None).unwrap();
        let mut cursor = tree.root_node().walk();

        let file_url = lsp::Url::from_file_path(file_str).unwrap();
        self.forest.insert(file_url.clone(), tree.clone());

        let include_nodes = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "include")
            .collect::<Vec<_>>();

        let _include_filenames = include_nodes.into_iter().filter_map(|include_node| {
            let node = include_node.children(&mut cursor).find(|c| c.kind() == "string")?;

            let filename = node
                .utf8_text(bytes)
                .unwrap()
                .trim_start_matches('"')
                .trim_end_matches('"');

            let path = Path::new(filename);

            let file_path = file_url
                .to_file_path()
                .map_err(|_| core::Error::UriToPathConversion)
                .ok()
                .unwrap();
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else if file_path.is_absolute() {
                file_path.parent().unwrap().join(path)
            } else {
                path.to_path_buf()
            };
            let path_str = path.into_os_string().into_string().unwrap();

            if !seen_files.contains(&path_str) {
                Some(self.find_includes(path_str, seen_files.clone()))
            } else {
                None
            }
        });

        Ok(seen_files)
    }

    pub async fn parse_initial_forest(&self, root_journal: String) -> anyhow::Result<bool, anyhow::Error> {
        let seen_files = HashSet::new();
        let journal_files = self.find_includes(root_journal, seen_files);

        Ok(true)
    }
}
