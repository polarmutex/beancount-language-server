use crate::{core, providers, server};
use dashmap::{
    mapref::one::{Ref, RefMut},
    DashMap,
};
use linked_list::LinkedList;
use log::debug;
use lspower::lsp;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::read_to_string,
    path::{Path, PathBuf},
};
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
    parsers: DashMap<lsp::Url, Mutex<tree_sitter::Parser>>,
    pub forest: DashMap<lsp::Url, Mutex<tree_sitter::Tree>>,
    pub root_journal_path: RwLock<Option<PathBuf>>,
    pub bean_check_path: Option<PathBuf>,
    pub beancount_data: core::BeancountData,
    pub diagnostic_data: providers::DiagnosticData,
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
        let parsers = DashMap::new();
        let forest = DashMap::new();
        let beancount_data = core::BeancountData::new();
        let diagnostic_data = providers::DiagnosticData::new();

        let bean_check_path = env::var_os("PATH").and_then(|paths| {
            env::split_paths(&paths).find_map(|p| {
                let full_path = p.join("bean-check");

                if full_path.is_file() {
                    Some(full_path)
                } else {
                    None
                }
            })
        });

        let root_journal_path = RwLock::new(None);

        Ok(Session {
            server_capabilities,
            client_capabilities,
            client,
            documents,
            parsers,
            forest,
            root_journal_path,
            bean_check_path,
            beancount_data,
            diagnostic_data,
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
        // debug_assert!(result.is_none());
        // let result = self.forest.insert(uri, Mutex::new(document.tree));
        // debug_assert!(result.is_none());
        Ok(())
    }

    /// Remove a [`core::Document`] from the [`Session`].
    pub fn remove_document(&self, uri: &lsp::Url) -> anyhow::Result<()> {
        let result = self.documents.remove(uri);
        debug_assert!(result.is_some());
        // let result = self.parsers.remove(uri);
        // debug_assert!(result.is_some());
        // let result = self.documents.remove(uri);
        // debug_assert!(result.is_some());
        Ok(())
    }

    /// Get a reference to the [`core::Document`] in the [`Session`].
    pub async fn get_document(&self, uri: &lsp::Url) -> anyhow::Result<Ref<'_, lsp::Url, core::Document>> {
        self.documents.get(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Document;
            let uri = uri.clone();
            core::Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    /// Get a mutable reference to the [`core::Document`] in the [`Session`].
    pub async fn get_mut_document(&self, uri: &lsp::Url) -> anyhow::Result<RefMut<'_, lsp::Url, core::Document>> {
        self.documents.get_mut(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Document;
            let uri = uri.clone();
            core::Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    pub async fn get_mut_parser(
        &self,
        uri: &lsp::Url,
    ) -> anyhow::Result<RefMut<'_, lsp::Url, Mutex<tree_sitter::Parser>>> {
        debug!("getting mutable parser {}", uri);
        debug!("parser contains key {}", self.parsers.contains_key(uri));
        self.parsers.get_mut(uri).ok_or_else(|| {
            debug!("Error getting mutable parser");
            let kind = SessionResourceKind::Parser;
            let uri = uri.clone();
            core::Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    /// Get a reference to the [`tree_sitter::Tree`] for a [`core::Document`] in the [`Session`].
    //pub async fn get_tree(&self, uri: &lsp::Url) -> anyhow::Result<Ref<'_, lsp::Url, Mutex<tree_sitter::Tree>>> {
    //    self.forest.get(uri).ok_or_else(|| {
    //        let kind = SessionResourceKind::Tree;
    //        let uri = uri.clone();
    //        core::Error::SessionResourceNotFound { kind, uri }.into()
    //    })
    //}

    /// Get a mutable reference to the [`tree_sitter::Tree`] for a [`core::Document`] in the
    /// [`Session`].
    pub async fn get_mut_tree(&self, uri: &lsp::Url) -> anyhow::Result<RefMut<'_, lsp::Url, Mutex<tree_sitter::Tree>>> {
        self.forest.get_mut(uri).ok_or_else(|| {
            let kind = SessionResourceKind::Tree;
            let uri = uri.clone();
            core::Error::SessionResourceNotFound { kind, uri }.into()
        })
    }

    // Issus to look at if running into issues with this
    // https://github.com/silvanshade/lspower/issues/8
    pub async fn parse_initial_forest(&self, root_url: lsp::Url) -> anyhow::Result<bool, anyhow::Error> {
        let mut seen_files = LinkedList::new();
        // let root_pathbuf: String = self.root_journal_path.into_inner().unwrap().as_ref().as_os_str();
        // let temp = self.root_journal_path.read().await;
        // let root_url = lsp::Url::from_file_path(temp.clone().unwrap()).unwrap();
        seen_files.push_back(root_url);

        // follow this for native cursor support
        // https://github.com/rust-lang/rust/issues/58533
        let mut ll_cursor = seen_files.cursor();
        while ll_cursor.peek_next() != None {
            let file = ll_cursor.next().unwrap();
            debug!("parsing {}", file.as_ref());

            let file_path = file
                .to_file_path()
                .map_err(|_| core::Error::UriToPathConversion)
                .ok()
                .unwrap();

            let text = read_to_string(file_path.clone())?;
            let bytes = text.as_bytes();

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(tree_sitter_beancount::language())?;
            let tree = parser.parse(&text, None).unwrap();
            self.parsers.insert(file.clone(), Mutex::new(parser));
            let mut cursor = tree.root_node().walk();

            debug!("adding to forest {}", file.as_ref());
            self.forest.insert(file.clone(), Mutex::new(tree.clone()));

            debug!("creating rope from text");
            let content = ropey::Rope::from_str(text.as_str());
            debug!("updating beancount data");
            self.beancount_data.update_data(file.clone(), &tree, &content);

            let include_nodes = tree
                .root_node()
                .children(&mut cursor)
                .filter(|c| c.kind() == "include")
                .collect::<Vec<_>>();

            let include_filenames = include_nodes.into_iter().filter_map(|include_node| {
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
            for include_url in include_filenames {
                if !self.forest.contains_key(&include_url) {
                    ll_cursor.insert(include_url);
                }
            }
        }
        Ok(true)
    }
}
