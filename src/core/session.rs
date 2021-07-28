use dashmap::DashMap;
use lspower::lsp;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Session {
    client: Option<lspower::Client>,
    forest: DashMap<lsp::Url, Mutex<tree_sitter::Tree>>,
}

impl Session {
    /// Create a new [`Session`].
    pub fn new(client: Option<lspower::Client>) -> anyhow::Result<Self> {
        let forest = DashMap::new();
        Ok(Session { client, forest })
    }
}
