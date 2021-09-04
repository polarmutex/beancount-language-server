use crate::{core, handlers};
use lspower::{jsonrpc, lsp, LanguageServer};
use std::{path::PathBuf, sync::Arc};

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
    async fn initialize(&self, params: lsp::InitializeParams) -> jsonrpc::Result<lsp::InitializeResult> {
        self.client
            .log_message(lsp::MessageType::Error, "Beancount Server initializing")
            .await;

        *self.session.client_capabilities.write().await = Some(params.capabilities);
        let capabilities = capabilities();

        let beancount_lsp_settings: core::BeancountLspOptions;
        if let Some(json) = params.initialization_options {
            beancount_lsp_settings = serde_json::from_value(json).unwrap();
        } else {
            beancount_lsp_settings = core::BeancountLspOptions {
                journal_file: String::from(""),
            };
        }
        // TODO need error if it does not exist
        *self.session.root_journal_path.write().await =
            Some(PathBuf::from(beancount_lsp_settings.journal_file.clone()));

        self.session
            .parse_initial_forest(lsp::Url::from_file_path(beancount_lsp_settings.journal_file).unwrap())
            .await;
        Ok(lsp::InitializeResult {
            capabilities,
            ..lsp::InitializeResult::default()
        })
    }

    async fn initialized(&self, _: lsp::InitializedParams) {
        let typ = lsp::MessageType::Info;
        let message = "beancount language server initialized!";
        self.client.log_message(typ, message).await;
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: lsp::DidOpenTextDocumentParams) {
        let session = self.session.clone();
        handlers::text_document::did_open(session, params).await.unwrap()
    }

    async fn did_change(&self, params: lsp::DidChangeTextDocumentParams) {
        let session = self.session.clone();
        handlers::text_document::did_change(session, params).await.unwrap()
    }

    async fn did_close(&self, params: lsp::DidCloseTextDocumentParams) {
        let session = self.session.clone();
        handlers::text_document::did_close(session, params).await.unwrap()
    }
}
