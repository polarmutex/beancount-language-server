use crate::{core, server};
use dashmap::{
    mapref::one::{Ref, RefMut},
    DashMap,
};
use linked_list::LinkedList;
use log::{debug, error, info, log_enabled, Level};
use lspower::lsp;
use serde::{Deserialize, Serialize};
use std::{fs::read_to_string, path::Path};
use tokio::sync::RwLock;

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
        let forest = DashMap::new();
        Ok(Session {
            server_capabilities,
            client_capabilities,
            client,
            documents,
            forest,
        })
    }

    /// Retrieve the handle for the LSP client.
    pub fn client(&self) -> anyhow::Result<&lspower::Client> {
        self.client
            .as_ref()
            .ok_or_else(|| core::Error::ClientNotInitialized.into())
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
    pub async fn parse_initial_forest(&self, root_journal: lsp::Url) -> anyhow::Result<bool, anyhow::Error> {
        let mut seen_files = LinkedList::new();
        seen_files.push_back(root_journal);

        // follow this for native cursor support
        // https://github.com/rust-lang/rust/issues/58533
        let mut ll_cursor = seen_files.cursor();
        let mut file = ll_cursor.next();
        while file != None {
            debug!("parsing {}", file.as_ref().unwrap());

            let file_path = file
                .as_ref()
                .unwrap()
                .to_file_path()
                .map_err(|_| core::Error::UriToPathConversion)
                .ok()
                .unwrap();

            let text = read_to_string(&file_path)?;
            let bytes = text.as_bytes();

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(tree_sitter_beancount::language())?;
            let tree = parser.parse(&text, None).unwrap();
            let mut cursor = tree.root_node().walk();

            debug!("adding to forest {}", file.as_ref().unwrap());
            self.forest.insert(file.cloned().unwrap(), tree.clone());

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

                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else if file_path.is_absolute() {
                    file_path.parent().unwrap().join(path)
                } else {
                    path.to_path_buf()
                };
                let path_url = lsp::Url::from_file_path(path).unwrap();

                Some(path_url)
            });

            // This could get in an infinite loop if there is a loop wtth the include files
            // TODO see if I can prevent this
            for include_url in _include_filenames {
                if !self.forest.contains_key(&include_url) {
                    ll_cursor.insert(include_url);
                }
            }

            file = ll_cursor.next();
        }
        Ok(true)
    }
}
