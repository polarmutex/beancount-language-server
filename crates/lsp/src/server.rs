use crate::beancount_data::BeancountData;
use crate::config::Config;
use crate::dispatcher::NotificationDispatcher;
use crate::dispatcher::RequestDispatcher;
use crate::document::Document;
use crate::forest;
use crate::handlers;
use crate::progress::Progress;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use lsp_types::notification::Notification;
use std::collections::HashMap;
use std::time::Instant;

pub(crate) type RequestHandler = fn(&mut LspServerState, lsp_server::Response);

#[derive(Debug)]
pub(crate) enum ProgressMsg {
    BeanCheck {
        total: usize,
        done: usize,
    },
    ForestInit {
        total: usize,
        done: usize,
        data: Box<Option<(lsp_types::Url, tree_sitter::Tree, BeancountData)>>,
    },
}

#[derive(Debug)]
pub(crate) enum Task {
    Response(lsp_server::Response),
    Notify(lsp_server::Notification),
    Progress(ProgressMsg),
}

#[derive(Debug)]
pub(crate) enum Event {
    Lsp(lsp_server::Message),
    Task(Task),
}

/*
struct LspServer {
    client: tower_lsp::Client,
    session: Session,
}
*/

pub(crate) struct LspServerState {
    pub beancount_data: HashMap<lsp_types::Url, BeancountData>,

    // the lsp server config options
    pub config: Config,

    pub forest: HashMap<lsp_types::Url, tree_sitter::Tree>,

    // Documents that are currently kept in memory from the client
    pub open_docs: HashMap<lsp_types::Url, Document>,

    pub parsers: HashMap<lsp_types::Url, tree_sitter::Parser>,

    // The request queue keeps track of all incoming and outgoing requests.
    pub req_queue: lsp_server::ReqQueue<(String, Instant), RequestHandler>,

    // Channel to send language server messages to the client
    pub sender: Sender<lsp_server::Message>,

    // True if the client requested that we shut down
    pub shutdown_requested: bool,

    // Channel to send tasks to from background operations
    pub task_sender: Sender<Task>,

    // Channel to receive tasks on from background operations
    pub task_receiver: Receiver<Task>,

    // Thread pool for async execution
    pub thread_pool: threadpool::ThreadPool,
}

/// A snapshot of the state of the language server
pub(crate) struct LspServerStateSnapshot {
    pub beancount_data: HashMap<lsp_types::Url, BeancountData>,
    pub config: Config,
    pub forest: HashMap<lsp_types::Url, tree_sitter::Tree>,
    pub open_docs: HashMap<lsp_types::Url, Document>,
}

/*
impl LspServer {
    /// Create a new [`Server`] instance.
    fn new(client: Client) -> Self {
        let session = Session::new(client.clone());
        Self { client, session }
    }
}
*/
impl LspServerState {
    pub fn new(sender: Sender<lsp_server::Message>, config: Config) -> Self {
        let (task_sender, task_receiver) = crossbeam_channel::unbounded();
        //let (event_tx, event_rx) = crossbeam_channel::unbounded();
        Self {
            beancount_data: HashMap::new(),
            config,
            forest: HashMap::new(),
            open_docs: HashMap::new(),
            parsers: HashMap::new(),
            req_queue: lsp_server::ReqQueue::default(),
            sender,
            shutdown_requested: false,
            task_sender,
            task_receiver,
            thread_pool: threadpool::ThreadPool::default(),
        }
    }

    pub fn run(&mut self, receiver: Receiver<lsp_server::Message>) -> Result<()> {
        // init forest
        if self.config.journal_root.is_some() {
            let file = self.config.journal_root.as_ref().unwrap();
            let journal_root = lsp_types::Url::from_file_path(file)
                .unwrap_or_else(|()| panic!("Cannot parse URL for file '{file:?}'"));

            tracing::info!("initializing forest...");
            let snapshot = self.snapshot();
            let sender = self.task_sender.clone();
            self.thread_pool.execute(move || {
                forest::parse_initial_forest(snapshot, journal_root, sender).unwrap();
            });
            /*forest::parse_initial_forest(
                &self.session,
                lsp_types::Url::from_file_path(
                    self.session.root_journal_path.read().await.clone().unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            */
        }

        while let Some(event) = self.next_event(&receiver) {
            if let Event::Lsp(lsp_server::Message::Notification(notification)) = &event {
                if notification.method == lsp_types::notification::Exit::METHOD {
                    return Ok(());
                }
            }
            self.handle_event(event)?;
        }
        Ok(())
    }

    // Blocks until new event is received
    pub fn next_event(&self, receiver: &Receiver<lsp_server::Message>) -> Option<Event> {
        crossbeam_channel::select! {
            recv(receiver) -> msg => msg.ok().map(Event::Lsp),
            recv(self.task_receiver) -> task => Some(Event::Task(task.unwrap())),
        }
    }

    // handles an event
    fn handle_event(&mut self, event: Event) -> Result<()> {
        tracing::info!("handling event {:?}", event);
        let start_time = Instant::now();

        match event {
            Event::Task(task) => self.handle_task(task)?,
            Event::Lsp(msg) => match msg {
                lsp_server::Message::Request(req) => self.on_request(req, start_time)?,
                lsp_server::Message::Response(resp) => self.complete_request(resp),
                lsp_server::Message::Notification(notif) => self.on_notification(notif)?,
            },
        };
        Ok(())
    }

    // Handles a task sent by another async task
    fn handle_task(&mut self, task: Task) -> anyhow::Result<()> {
        match task {
            Task::Notify(notification) => {
                self.send(notification.into());
            }
            Task::Response(response) => self.respond(response),
            Task::Progress(task) => self.handle_progress_task(task)?,
        }
        Ok(())
    }

    fn handle_progress_task(&mut self, task: ProgressMsg) -> Result<()> {
        match task {
            ProgressMsg::BeanCheck { total, done } => {
                let progress_state = if done == 0 {
                    Progress::Begin
                } else if done < total {
                    Progress::Report
                } else {
                    Progress::End
                };
                self.report_progress(
                    "bean check",
                    progress_state,
                    Some(format!("{}/{}", done, total)),
                    Some(Progress::fraction(done, total)),
                )
            }
            ProgressMsg::ForestInit { total, done, data } => {
                if let Some(data) = *data {
                    self.forest.insert(data.0.clone(), data.1);
                    self.beancount_data.insert(data.0, data.2);
                }
                let progress_state = if done == 0 {
                    Progress::Begin
                } else if done < total {
                    Progress::Report
                } else {
                    Progress::End
                };
                self.report_progress(
                    "generating forest",
                    progress_state,
                    Some(format!("{}/{}", done, total)),
                    Some(Progress::fraction(done, total)),
                )
            }
        }
        Ok(())
    }

    // Registers a request with the server. We register all these request to make
    // sure they all get handled and so we can measure the time it takes for them
    // to complete from the point of view of the client.
    fn register_request(&mut self, request: &lsp_server::Request, start_time: Instant) {
        self.req_queue
            .incoming
            .register(request.id.clone(), (request.method.clone(), start_time))
    }

    // Handles a language server protocol request
    fn on_request(&mut self, req: lsp_server::Request, start_time: Instant) -> Result<()> {
        self.register_request(&req, start_time);
        if self.shutdown_requested {
            self.respond(lsp_server::Response::new_err(
                req.id,
                lsp_server::ErrorCode::InvalidRequest as i32,
                "shutdown was requested".to_string(),
            ));
            return Ok(());
        }

        RequestDispatcher::new(self, req)
            .on_sync::<lsp_types::request::Shutdown>(|state, _request| {
                state.shutdown_requested = true;
                Ok(())
            })?
            .on::<lsp_types::request::Completion>(handlers::text_document::completion)?
            .on::<lsp_types::request::Formatting>(handlers::text_document::formatting)?
            .finish();
        Ok(())
    }

    // Handles a response to a request we made. The response gets forwarded to where we made the request from.
    fn complete_request(&mut self, resp: lsp_server::Response) {
        let handler = self
            .req_queue
            .outgoing
            .complete(resp.id.clone())
            .expect("received response for unknown request");
        handler(self, resp)
    }

    // Handles a notification from the language server client
    fn on_notification(&mut self, notif: lsp_server::Notification) -> Result<()> {
        NotificationDispatcher::new(self, notif)
            .on::<lsp_types::notification::DidOpenTextDocument>(handlers::text_document::did_open)?
            .on::<lsp_types::notification::DidCloseTextDocument>(
                handlers::text_document::did_close,
            )?
            .on::<lsp_types::notification::DidSaveTextDocument>(handlers::text_document::did_save)?
            .on::<lsp_types::notification::DidChangeTextDocument>(
                handlers::text_document::did_change,
            )?
            .finish();
        Ok(())
    }

    // Sends a response to the client. This method logs the time it took us to reply to a request from the client.
    pub(crate) fn respond(&mut self, response: lsp_server::Response) {
        if let Some((_method, start)) = self.req_queue.incoming.complete(response.id.clone()) {
            let duration = start.elapsed();
            tracing::info!("handled req#{} in {:?}", response.id, duration);
            self.send(response.into());
        }
    }

    /// Sends a message to the client
    pub(crate) fn send(&mut self, message: lsp_server::Message) {
        self.sender
            .send(message)
            .expect("error sending lsp message to the outgoing channel")
    }

    // Sends a request to the client and registers the request so that we can handle the response.
    pub(crate) fn send_request<R: lsp_types::request::Request>(
        &mut self,
        params: R::Params,
        handler: RequestHandler,
    ) {
        let request = self
            .req_queue
            .outgoing
            .register(R::METHOD.to_string(), params, handler);
        self.send(request.into());
    }

    // Sends a notification to the client
    pub(crate) fn send_notification<N: lsp_types::notification::Notification>(
        &mut self,
        params: N::Params,
    ) {
        let not = lsp_server::Notification::new(N::METHOD.to_string(), params);
        self.send(not.into());
    }

    pub(crate) fn snapshot(&self) -> LspServerStateSnapshot {
        LspServerStateSnapshot {
            beancount_data: self.beancount_data.clone(),
            config: self.config.clone(),
            forest: self.forest.clone(),
            open_docs: self.open_docs.clone(),
        }
    }
}

/*
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
}*/

/*
#[tower_lsp::async_trait]
impl LanguageServer for LspServer {
    async fn initialize(
        &self,
        params: lsp_types::InitializeParams,
    ) -> jsonrpc::Result<lsp_types::InitializeResult> {
        self.client
            .log_message(
                lsp_types::MessageType::ERROR,
                "Beancount Server initializing",
            )
            .await;

        *self.session.client_capabilities.write().await = Some(params.capabilities);
        let capabilities = capabilities();

        let beancount_lsp_settings: BeancountLspOptions =
            if let Some(json) = params.initialization_options {
                serde_json::from_value(json).unwrap()
            } else {
                BeancountLspOptions {
                    journal_file: String::from(""),
                }?
            };
        // TODO need error if it does not exist
        *self.session.root_journal_path.write().await =
            Some(PathBuf::from(beancount_lsp_settings.journal_file.clone()));

        Ok(lsp_types::InitializeResult {
            capabilities,
            ..lsp_types::InitializeResult::default()
        })
    }

    async fn initialized(&self, _: lsp_types::InitializedParams) {
        if self.session.root_journal_path.read().await.is_some() {
            forest::parse_initial_forest(
                &self.session,
                lsp_types::Url::from_file_path(
                    self.session.root_journal_path.read().await.clone().unwrap(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: lsp_types::DidOpenTextDocumentParams) {
        handlers::text_document::did_open(&self.session, params)
            .await
            .unwrap()
    }

    async fn did_save(&self, params: lsp_types::DidSaveTextDocumentParams) {
        handlers::text_document::did_save(&self.session, params)
            .await
            .unwrap()
    }

    async fn did_change(&self, params: lsp_types::DidChangeTextDocumentParams) {
        handlers::text_document::did_change(&self.session, params)
            .await
            .unwrap()
    }

    async fn did_close(&self, params: lsp_types::DidCloseTextDocumentParams) {
        handlers::text_document::did_close(&self.session, params)
            .await
            .unwrap()
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
*/
