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
            change: Some(lsp::TextDocumentSyncKind::INCREMENTAL),
            will_save: Some(true),
            will_save_wait_until: Some(false),
            save: Some(lsp::TextDocumentSyncSaveOptions::SaveOptions(lsp::SaveOptions {
                include_text: Some(true),
            })),
            ..Default::default()
        };
        Some(lsp::TextDocumentSyncCapability::Options(options))
    };
    let completion_provider = {
        let options = lsp::CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![
                "2".to_string(),
                ":".to_string(),
                "#".to_string(),
                "\"".to_string(),
            ]),
            ..Default::default()
        };
        Some(options)
    };

    let document_formatting_provider = {
        Some(lsp::OneOf::Left(true))
    };

    lsp::ServerCapabilities {
        text_document_sync,
        completion_provider,
        document_formatting_provider,
        ..Default::default()
    }
}

#[lspower::async_trait]
impl LanguageServer for Server {
    async fn initialize(&self, params: lsp::InitializeParams) -> jsonrpc::Result<lsp::InitializeResult> {
        self.client
            .log_message(lsp::MessageType::ERROR, "Beancount Server initializing")
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
        let typ = lsp::MessageType::INFO;
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

    async fn did_save(&self, params: lsp::DidSaveTextDocumentParams) {
        let session = self.session.clone();
        handlers::text_document::did_save(session, params).await.unwrap()
    }

    async fn did_change(&self, params: lsp::DidChangeTextDocumentParams) {
        let session = self.session.clone();
        handlers::text_document::did_change(session, params).await.unwrap()
    }

    async fn did_close(&self, params: lsp::DidCloseTextDocumentParams) {
        let session = self.session.clone();
        handlers::text_document::did_close(session, params).await.unwrap()
    }

    async fn completion(&self, params: lsp::CompletionParams) -> jsonrpc::Result<Option<lsp::CompletionResponse>> {
        let session = self.session.clone();
        let result = handlers::text_document::completion(session, params).await;
        Ok(result.map_err(core::IntoJsonRpcError)?)
    }

    async fn formatting(&self, params: lsp::DocumentFormattingParams) -> jsonrpc::Result<Option<Vec<lsp::TextEdit>>> {
        let session = self.session.clone();
        let result = handlers::text_document::formatting(session, params).await;
        Ok(result.map_err(core::IntoJsonRpcError)?)
    }
}
