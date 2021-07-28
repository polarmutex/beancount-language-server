use crate::core;
use lspower::{jsonrpc, lsp, LanguageServer};
use std::sync::Arc;

#[derive(Debug)]
pub struct Server {
    pub client: lspower::Client,
    pub session: Arc<core::Session>,
}

impl Server {
    /// Create a new [`Server`] instance.
    pub fn new(client: lspower::Client) -> anyhow::Result<Self> {
        let session = Arc::new(core::Session::new(Some(client.clone()))?);
        Ok(Server { client, session })
    }
}

pub fn capabilities() -> lsp::ServerCapabilities {
    let text_document_sync = {
        let options = lsp::TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(lsp::TextDocumentSyncKind::Incremental),
            ..Default::default()
        };
        Some(lsp::TextDocumentSyncCapability::Options(options))
    };

    lsp::ServerCapabilities {
        text_document_sync,
        ..Default::default()
    }
}

#[lspower::async_trait]
impl LanguageServer for Server {
    async fn initialize(&self, _: lsp::InitializeParams) -> jsonrpc::Result<lsp::InitializeResult> {
        Ok(lsp::InitializeResult::default())
    }

    async fn initialized(&self, _: lsp::InitializedParams) {
        self.client
            .log_message(lsp::MessageType::Info, "server initialized!")
            .await;
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }
}
