use crate::beancount_data::BeancountData;
use crate::capabilities;
use crate::config::Config;
use crate::document::Document;
use crate::forest;
use crate::handlers;
use crate::treesitter_utils::lsp_textdocchange_to_ts_inputedit;
use anyhow::Result;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use async_lsp::ErrorCode;
use async_lsp::ResponseError;
use lsp_types::notification as notif;
use lsp_types::request as req;
use lsp_types::request::Request;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidCloseTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::DidSaveTextDocumentParams;
use lsp_types::InitializeParams;
use lsp_types::InitializeResult;
use lsp_types::InitializedParams;
use lsp_types::ServerInfo;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::future::ready;
use std::future::Future;
use std::ops::ControlFlow;
use std::panic;
use std::panic::UnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::task;
use tokio::task::JoinHandle;

type NotifyResult = ControlFlow<async_lsp::Result<()>>;

pub struct LspServerState {
    client: ClientSocket,

    pub beancount_data: Arc<RwLock<HashMap<lsp_types::Url, BeancountData>>>,

    // the lsp server config options
    pub config: Arc<Config>,

    pub forest: Arc<RwLock<HashMap<lsp_types::Url, tree_sitter::Tree>>>,

    // Documents that are currently kept in memory from the client
    pub open_docs: Arc<RwLock<HashMap<lsp_types::Url, Document>>>,

    pub parsers: HashMap<lsp_types::Url, tree_sitter::Parser>,

    load_forest_future: Option<JoinHandle<()>>,
    load_diagnostics_future: Option<JoinHandle<()>>,
}

impl LspServerState {
    pub fn new_router(client: ClientSocket) -> Router<Self> {
        let this = Self::new(client);
        let mut router = Router::new(this);
        router
            .request::<req::Initialize, _>(Self::on_initialize)
            .notification::<notif::Initialized>(Self::on_initialized)
            .notification::<notif::DidOpenTextDocument>(Self::on_did_open)
            .notification::<notif::DidCloseTextDocument>(Self::on_did_close)
            .notification::<notif::DidChangeTextDocument>(Self::on_did_change)
            .notification::<notif::DidSaveTextDocument>(Self::on_did_save)
            .request_snap::<req::Completion>(handlers::completion)
            .request_snap::<req::Formatting>(handlers::formatting)
            .request::<req::Shutdown, _>(|_, _| ready(Ok(())))
            .notification::<notif::Exit>(|_, _| ControlFlow::Break(Ok(())));
        router
    }

    pub fn new(client: ClientSocket) -> Self {
        Self {
            client,
            beancount_data: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(Config::new(PathBuf::new())),
            forest: Arc::new(RwLock::new(HashMap::new())),
            open_docs: Arc::new(RwLock::new(HashMap::new())),
            parsers: HashMap::new(),

            load_forest_future: None,
            load_diagnostics_future: None,
        }
    }

    fn on_initialize(
        &mut self,
        params: InitializeParams,
    ) -> impl Future<Output = Result<InitializeResult, ResponseError>> {
        tracing::info!("Init params: {params:?}");

        let config = {
            let root_file = match params.root_uri.and_then(|it| it.to_file_path().ok()) {
                Some(it) => it,
                None => std::env::current_dir().expect("to have a current dir"),
            };
            let mut config = Config::new(root_file);
            if let Some(json) = params.initialization_options {
                config.update(json).unwrap();
            }
            config
        };
        *Arc::get_mut(&mut self.config).expect("No concurrent access yet") = config;

        let server_capabilities = capabilities::server_capabilities();
        ready(Ok(InitializeResult {
            capabilities: server_capabilities,
            server_info: Some(ServerInfo {
                name: String::from("beancount-language-server"),
                version: option_env!("CFG_RELEASE").map(Into::into),
            }),
        }))
    }

    fn spawn_load_forest(&mut self) {
        let file = self.config.journal_root.as_ref().unwrap();
        let journal_root = lsp_types::Url::from_file_path(file)
            .unwrap_or_else(|()| panic!("Cannot parse URL for file '{file:?}'"));
        let client = self.client.clone();
        let forest = Arc::clone(&self.forest);
        let data = Arc::clone(&self.beancount_data);
        let fut = tokio::spawn(async {
            forest::parse_initial_forest(client, forest, data, journal_root).await
        });
        if let Some(prev_fut) = self.load_forest_future.replace(fut) {
            prev_fut.abort();
        }
        // tokio::spawn(async { forest::parse_initial_forest(forest, data, journal_root).await });
    }

    fn spawn_update_diagnostics(&mut self, uri: String) {
        let path = if self.config.journal_root.is_some() {
            self.config.journal_root.as_ref().unwrap().clone()
        } else {
            PathBuf::from(uri.to_string().replace("file://", ""))
        };
        let client = self.client.clone();
        let forest = self.forest.read().unwrap().clone();
        let data = self.beancount_data.read().unwrap().clone();
        let fut =
            task::spawn(async { handlers::handle_diagnostics(client, forest, data, path).await });
        // let fut = task::spawn(Self::load_flake_workspace(self.client.clone()));
        if let Some(prev_fut) = self.load_diagnostics_future.replace(fut) {
            prev_fut.abort();
        }
    }

    fn on_initialized(&mut self, _params: InitializedParams) -> NotifyResult {
        self.spawn_load_forest();
        ControlFlow::Continue(())
    }

    /// handler for `textDocument/didOpen`.
    fn on_did_open(&mut self, params: DidOpenTextDocumentParams) -> NotifyResult {
        tracing::debug!("handlers::did_open");
        let uri = params.text_document.uri.clone();

        let document = Document::open(params.clone());
        //let tree = document.tree.clone();
        tracing::debug!("handlers::did_open - adding {}", uri);
        self.open_docs
            .write()
            .unwrap()
            .insert(uri.clone(), document);

        self.parsers.entry(uri.clone()).or_insert_with(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(tree_sitter_beancount::language())
                .unwrap();
            parser
        });
        let parser = self.parsers.get_mut(&uri).unwrap();

        self.forest
            .write()
            .unwrap()
            .entry(uri.clone())
            .or_insert_with(|| parser.parse(&params.text_document.text, None).unwrap());

        self.beancount_data
            .write()
            .unwrap()
            .entry(uri.clone())
            .or_insert_with(|| {
                let content = ropey::Rope::from_str(&params.text_document.text);
                BeancountData::new(self.forest.read().unwrap().get(&uri).unwrap(), &content)
            });

        // let snapshot = self.snapshot();
        // let _result = handle_diagnostics(snapshot, task_sender, params.text_document.uri);
        self.spawn_update_diagnostics(params.text_document.uri.to_string());

        ControlFlow::Continue(())
    }

    // handler for `textDocument/didClose`.
    fn on_did_close(&mut self, params: DidCloseTextDocumentParams) -> NotifyResult {
        tracing::debug!("handlers::did_close");
        let uri = params.text_document.uri;
        self.open_docs.write().unwrap().remove(&uri);
        // let version = Default::default();
        ControlFlow::Continue(())
    }

    // handler for `textDocument/didChange`.
    fn on_did_change(&mut self, params: DidChangeTextDocumentParams) -> NotifyResult {
        tracing::debug!("handlers::did_change");
        let uri = &params.text_document.uri;
        tracing::debug!("handlers::did_change - requesting {}", uri);
        let mut binding = self.open_docs.write().unwrap();
        let doc = binding.get_mut(uri).unwrap();

        tracing::debug!("handlers::did_change - convert edits");
        let edits = params
            .content_changes
            .iter()
            .map(|change| lsp_textdocchange_to_ts_inputedit(&doc.content, change))
            .collect::<Result<Vec<_>, _>>()
            .expect("");

        tracing::debug!("handlers::did_change - apply edits - document");
        for change in &params.content_changes {
            let text = change.text.as_str();
            let text_bytes = text.as_bytes();
            let text_end_byte_idx = text_bytes.len();

            let range = if let Some(range) = change.range {
                range
            } else {
                let start_line_idx = doc.content.byte_to_line(0);
                let end_line_idx = doc.content.byte_to_line(text_end_byte_idx);

                let start = lsp_types::Position::new(start_line_idx as u32, 0);
                let end = lsp_types::Position::new(end_line_idx as u32, 0);
                lsp_types::Range { start, end }
            };

            let start_row_char_idx = doc.content.line_to_char(range.start.line as usize);
            let start_col_char_idx = doc.content.utf16_cu_to_char(range.start.character as usize);
            let end_row_char_idx = doc.content.line_to_char(range.end.line as usize);
            let end_col_char_idx = doc.content.utf16_cu_to_char(range.end.character as usize);

            let start_char_idx = start_row_char_idx + start_col_char_idx;
            let end_char_idx = end_row_char_idx + end_col_char_idx;
            doc.content.remove(start_char_idx..end_char_idx);

            if !change.text.is_empty() {
                doc.content.insert(start_char_idx, text);
            }
        }

        tracing::debug!("handlers::did_change - apply edits - tree");
        let result = {
            let parser = self.parsers.get_mut(uri).unwrap();
            //let mut parser = parser.lock();

            let mut binding = self.forest.write().unwrap();
            let old_tree = binding.get_mut(uri).unwrap();
            //let mut old_tree = old_tree.lock().await;

            for edit in &edits {
                old_tree.edit(edit);
            }

            parser.parse(doc.text().to_string(), Some(old_tree))
        };

        tracing::debug!("handlers::did_change - save tree");
        if let Some(tree) = result {
            *self.forest.write().unwrap().get_mut(uri).unwrap() = tree.clone();
            *self.beancount_data.write().unwrap().get_mut(uri).unwrap() =
                BeancountData::new(&tree, &doc.content);
            /*.unwrap().update_data(
                uri.clone(),
                &tree,
                &doc.content,
            );*/
        }

        tracing::debug!("handlers::did_close - done");

        ControlFlow::Continue(())
    }

    // handler for `textDocument/didClose`.
    fn on_did_save(&mut self, params: DidSaveTextDocumentParams) -> NotifyResult {
        tracing::debug!("handlers::did_save");
        self.spawn_update_diagnostics(params.text_document.uri.to_string());
        ControlFlow::Continue(())
    }

    fn spawn_with_snapshot<T: Send + 'static>(
        &self,
        f: impl FnOnce(LspServerStateSnapshot) -> T + Send + 'static,
    ) -> JoinHandle<T> {
        let snap = LspServerStateSnapshot {
            beancount_data: Arc::clone(&self.beancount_data),
            forest: Arc::clone(&self.forest),
            open_docs: Arc::clone(&self.open_docs),
        };
        task::spawn_blocking(move || f(snap))
    }
}

/// A snapshot of the state of the language server
pub(crate) struct LspServerStateSnapshot {
    pub(crate) beancount_data: Arc<RwLock<HashMap<lsp_types::Url, BeancountData>>>,
    pub(crate) forest: Arc<RwLock<HashMap<lsp_types::Url, tree_sitter::Tree>>>,
    pub(crate) open_docs: Arc<RwLock<HashMap<lsp_types::Url, Document>>>,
}

impl LspServerStateSnapshot {
    pub(crate) fn beancount_data(
        &self,
    ) -> impl std::ops::Deref<Target = HashMap<lsp_types::Url, BeancountData>> + '_ {
        self.beancount_data.read().unwrap()
    }
    pub(crate) fn forest(
        &self,
    ) -> impl std::ops::Deref<Target = HashMap<lsp_types::Url, tree_sitter::Tree>> + '_ {
        self.forest.read().unwrap()
    }
    pub(crate) fn open_docs(
        &self,
    ) -> impl std::ops::Deref<Target = HashMap<lsp_types::Url, Document>> + '_ {
        self.open_docs.read().unwrap()
    }
}

trait RouterExt: BorrowMut<Router<LspServerState>> {
    fn request_snap<R: Request>(
        &mut self,
        f: impl Fn(LspServerStateSnapshot, R::Params) -> Result<R::Result>
            + Send
            + Copy
            + UnwindSafe
            + 'static,
    ) -> &mut Self
    where
        R::Params: Send + UnwindSafe + 'static,
        R::Result: Send + 'static,
    {
        self.borrow_mut().request::<R, _>(move |this, params| {
            let task = this.spawn_with_snapshot(move |snap| {
                // with_catch_unwind(R::METHOD, move || f(snap, params))
                f(snap, params)
            });
            async move {
                task.await
                    .expect("Already catch_unwind")
                    .map_err(error_to_response)
            }
        });
        self
    }
}

impl RouterExt for Router<LspServerState> {}

fn error_to_response(err: anyhow::Error) -> ResponseError {
    match err.downcast::<ResponseError>() {
        Ok(resp) => resp,
        Err(err) => ResponseError::new(ErrorCode::INTERNAL_ERROR, err),
    }
}
