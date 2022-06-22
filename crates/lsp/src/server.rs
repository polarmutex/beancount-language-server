use crate::{error, handlers, session::BeancountLspOptions, session::Session};
use std::path::PathBuf;
use tokio::io::{Stdin, Stdout};
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct LspServer {
    client: tower_lsp::Client,
    session: Session,
}

impl LspServer {
    /// Create a new [`Server`] instance.
    fn new(client: Client) -> Self {
        let session = Session::new(client.clone());
        Self { client, session }
    }
}

pub fn capabilities() -> lsp_types::ServerCapabilities {
    let text_document_sync = {
        let options = lsp_types::TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(lsp_types::TextDocumentSyncKind::INCREMENTAL),
            will_save: Some(true),
            will_save_wait_until: Some(false),
            save: Some(lsp_types::TextDocumentSyncSaveOptions::SaveOptions(
                lsp_types::SaveOptions {
                    include_text: Some(true),
                },
            )),
        };
        Some(lsp_types::TextDocumentSyncCapability::Options(options))
    };
    let completion_provider = {
        let options = lsp_types::CompletionOptions {
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

    let document_formatting_provider = { Some(lsp_types::OneOf::Left(true)) };

    lsp_types::ServerCapabilities {
        text_document_sync,
        completion_provider,
        document_formatting_provider,
        ..Default::default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LspServer {
    async fn initialize(&self, params: lsp_types::InitializeParams) -> jsonrpc::Result<lsp_types::InitializeResult> {
        self.client
            .log_message(lsp_types::MessageType::ERROR, "Beancount Server initializing")
            .await;

        *self.session.client_capabilities.write().await = Some(params.capabilities);
        let capabilities = capabilities();

        let beancount_lsp_settings: BeancountLspOptions = if let Some(json) = params.initialization_options {
            serde_json::from_value(json).unwrap()
        } else {
            BeancountLspOptions {
                journal_file: String::from(""),
            }
        };
        // TODO need error if it does not exist
        *self.session.root_journal_path.write().await =
            Some(PathBuf::from(beancount_lsp_settings.journal_file.clone()));

        let journal_file = lsp_types::Url::from_file_path(beancount_lsp_settings.journal_file).unwrap();

        if (self.session.parse_initial_forest(journal_file).await).is_ok() {};

        Ok(lsp_types::InitializeResult {
            capabilities,
            ..lsp_types::InitializeResult::default()
        })
    }

    async fn initialized(&self, _: lsp_types::InitializedParams) {
        //let typ = lsp_types::MessageType::INFO;
        //let message = "beancount language server initialized!";
        //self.client.log_message(typ, message).await;
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: lsp_types::DidOpenTextDocumentParams) {
        handlers::text_document::did_open(&self.session, params).await.unwrap()
    }

    async fn did_save(&self, params: lsp_types::DidSaveTextDocumentParams) {
        handlers::text_document::did_save(&self.session, params).await.unwrap()
    }

    async fn did_change(&self, params: lsp_types::DidChangeTextDocumentParams) {
        handlers::text_document::did_change(&self.session, params)
            .await
            .unwrap()
    }

    async fn did_close(&self, params: lsp_types::DidCloseTextDocumentParams) {
        handlers::text_document::did_close(&self.session, params).await.unwrap()
    }

    async fn completion(
        &self,
        params: lsp_types::CompletionParams,
    ) -> jsonrpc::Result<Option<lsp_types::CompletionResponse>> {
        let result = handlers::text_document::completion(&self.session, params).await;
        Ok(result.map_err(error::IntoJsonRpcError)?)
    }

    async fn formatting(
        &self,
        params: lsp_types::DocumentFormattingParams,
    ) -> jsonrpc::Result<Option<Vec<lsp_types::TextEdit>>> {
        let result = handlers::text_document::formatting(&self.session, params).await;
        Ok(result.map_err(error::IntoJsonRpcError)?)
    }
}

pub async fn run_server(stdin: Stdin, stdout: Stdout) {
    let (service, messages) = LspService::build(LspServer::new).finish();
    Server::new(stdin, stdout, messages).serve(service).await;
}
