use crate::beancount_data::BeancountData;
use crate::checkers::BeancountChecker;
use crate::checkers::create_checker;
use crate::config::Config;
use crate::dispatcher::NotificationDispatcher;
use crate::dispatcher::RequestRouter;
use crate::document::Document;
use crate::forest;
use crate::handlers;
use crate::progress::Progress;
use crate::utils::ToFilePath;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use lsp_types::notification::Notification;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tree_sitter_beancount::tree_sitter;

pub(crate) type RequestHandler = fn(&mut LspServerState, lsp_server::Response);
pub(crate) type ForestData = Box<Option<(PathBuf, Arc<tree_sitter::Tree>, Arc<BeancountData>)>>;

#[derive(Debug)]
pub(crate) enum ProgressMsg {
    BeanCheck {
        total: usize,
        done: usize,
        checker_name: String,
    },
    ForestInit {
        total: usize,
        done: usize,
        data: ForestData,
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
    pub beancount_data: HashMap<PathBuf, Arc<BeancountData>>,

    // the lsp server config options
    pub config: Config,

    pub forest: HashMap<PathBuf, Arc<tree_sitter::Tree>>,

    // Documents that are currently kept in memory from the client
    pub open_docs: HashMap<PathBuf, Document>,

    pub parsers: HashMap<PathBuf, tree_sitter::Parser>,

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

    // Cached checker instance (created once and reused)
    pub checker: Option<Arc<dyn BeancountChecker>>,

    // Request router with registered handlers
    pub request_router: Arc<RequestRouter>,
}

/// A snapshot of the state of the language server
pub(crate) struct LspServerStateSnapshot {
    pub beancount_data: HashMap<PathBuf, Arc<BeancountData>>,
    pub config: Config,
    pub forest: HashMap<PathBuf, Arc<tree_sitter::Tree>>,
    pub open_docs: HashMap<PathBuf, Document>,
    pub checker: Option<Arc<dyn BeancountChecker>>,
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
        let request_router = Arc::new(Self::build_request_router());
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
            checker: None,
            request_router,
        }
    }

    pub fn run(&mut self, receiver: Receiver<lsp_server::Message>) -> Result<()> {
        tracing::info!("LSP server starting main event loop");

        // Initialize checker once (can be slow); report progress to users.
        self.ensure_checker();

        // init forest
        if let Some(file) = self.config.journal_root.as_ref() {
            let journal_root = if file.is_relative() {
                self.config.root_dir.join(file)
            } else {
                file.clone()
            };

            // Check if exists
            if !journal_root.exists() {
                let error_msg = format!("Journal root does not exist: {}", journal_root.display());
                tracing::error!("{}", error_msg);

                // Send error message to client
                self.send_notification::<lsp_types::notification::ShowMessage>(
                    lsp_types::ShowMessageParams {
                        typ: lsp_types::MessageType::ERROR,
                        message: error_msg.clone(),
                    },
                );

                // Log warning and continue without forest initialization instead of returning error
                // This allows the language server to continue functioning for open documents
                tracing::warn!(
                    "Continuing without forest initialization due to invalid journal root"
                );
            } else {
                tracing::info!(
                    "Initializing forest for journal root: {}",
                    journal_root.display()
                );
                let snapshot = self.snapshot();
                let sender = self.task_sender.clone();
                self.thread_pool.execute(move || {
                    match forest::parse_initial_forest(snapshot, journal_root, sender) {
                        Ok(_) => tracing::info!("Forest initialization completed successfully"),
                        Err(e) => tracing::error!("Forest initialization failed: {}", e),
                    }
                });
            }
        } else {
            tracing::warn!("No journal_root configured, skipping forest initialization");
        }

        tracing::debug!("Entering main event loop");
        while let Some(event) = self.next_event(&receiver) {
            if let Event::Lsp(lsp_server::Message::Notification(notification)) = &event
                && notification.method == lsp_types::notification::Exit::METHOD
            {
                tracing::info!("Received exit notification, shutting down");
                return Ok(());
            }
            self.handle_event(event)?;
        }
        tracing::info!("Main event loop completed");
        Ok(())
    }

    // Blocks until new event is received
    pub fn next_event(&self, receiver: &Receiver<lsp_server::Message>) -> Option<Event> {
        crossbeam_channel::select! {
            recv(receiver) -> msg => msg.ok().map(Event::Lsp),
            recv(self.task_receiver) -> task => task.ok().map(Event::Task),
        }
    }

    // handles an event
    fn handle_event(&mut self, event: Event) -> Result<()> {
        let start_time = Instant::now();

        match event {
            Event::Task(task) => {
                tracing::debug!("Handling task: {:?}", task);
                self.handle_task(task)?;
            }
            Event::Lsp(msg) => match msg {
                lsp_server::Message::Request(req) => {
                    tracing::debug!("Handling LSP request: method={}, id={}", req.method, req.id);
                    self.on_request(req, start_time)?;
                }
                lsp_server::Message::Response(resp) => {
                    tracing::debug!("Handling LSP response: id={}", resp.id);
                    self.complete_request(resp);
                }
                lsp_server::Message::Notification(notif) => {
                    tracing::debug!("Handling LSP notification: method={}", notif.method);
                    self.on_notification(notif)?;
                }
            },
        };

        let duration = start_time.elapsed();
        if duration.as_millis() > 100 {
            tracing::warn!("Event handling took longer than expected: {:?}", duration);
        }

        Ok(())
    }

    // Handles a task sent by another async task
    fn handle_task(&mut self, task: Task) -> anyhow::Result<()> {
        match task {
            Task::Notify(notification) => {
                tracing::debug!("Sending notification: {}", notification.method);
                self.send(notification.into());
            }
            Task::Response(response) => {
                tracing::debug!("Sending response for request: {}", response.id);
                self.respond(response);
            }
            Task::Progress(progress_task) => {
                tracing::debug!("Handling progress task: {:?}", progress_task);
                self.handle_progress_task(progress_task)?;
            }
        }
        Ok(())
    }

    fn handle_progress_task(&mut self, task: ProgressMsg) -> Result<()> {
        match task {
            ProgressMsg::BeanCheck {
                total,
                done,
                checker_name,
            } => {
                let progress_state = if done == 0 {
                    Progress::Begin
                } else if done < total {
                    Progress::Report
                } else {
                    Progress::End
                };
                self.report_progress(
                    &format!("bean check ({})", checker_name),
                    progress_state,
                    Some(format!("{done}/{total}")),
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
                    Some(format!("{done}/{total}")),
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
            tracing::warn!("Request {} received after shutdown was requested", req.id);
            self.respond(lsp_server::Response::new_err(
                req.id,
                lsp_server::ErrorCode::InvalidRequest as i32,
                "shutdown was requested".to_string(),
            ));
            return Ok(());
        }

        tracing::debug!("Processing request: method={}, id={}", req.method, req.id);

        self.request_router.clone().dispatch(self, req);

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
        if let Some((method, start)) = self.req_queue.incoming.complete(&response.id) {
            let duration = start.elapsed();
            let is_error = response.error.is_some();

            if is_error {
                tracing::warn!(
                    "Request {} ({}) completed with error in {:?}: {:?}",
                    response.id,
                    method,
                    duration,
                    response.error
                );
            } else {
                tracing::trace!(
                    "Request {} ({}) completed successfully in {:?}",
                    response.id,
                    method,
                    duration
                );
            }

            if duration.as_millis() > 1000 {
                tracing::warn!("Slow request detected: {} took {:?}", method, duration);
            }

            self.send(response.into());
        } else {
            tracing::warn!("Received response for unknown request: {}", response.id);
        }
    }

    /// Sends a message to the client
    pub(crate) fn send(&mut self, message: lsp_server::Message) {
        match &message {
            lsp_server::Message::Request(req) => {
                tracing::debug!(
                    "Sending request to client: method={}, id={}",
                    req.method,
                    req.id
                );
            }
            lsp_server::Message::Response(resp) => {
                tracing::debug!(
                    "Sending response to client: id={}, has_error={}",
                    resp.id,
                    resp.error.is_some()
                );
            }
            lsp_server::Message::Notification(notif) => {
                tracing::debug!("Sending notification to client: method={}", notif.method);
            }
        }

        if let Err(e) = self.sender.send(message) {
            tracing::error!("Failed to send LSP message to client: {}", e);
        }
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
            checker: self.checker.clone(),
        }
    }

    fn build_request_router() -> RequestRouter {
        let mut router = RequestRouter::new();
        router
            .on_sync::<lsp_types::request::Shutdown>(|state, _request| {
                tracing::info!("Received shutdown request");
                state.shutdown_requested = true;
                Ok(())
            })
            .expect("Failed to register Shutdown handler")
            .on_with::<lsp_types::request::Completion>(
                |r, params| {
                    r.ensure_beancount_data_for_position(&params.text_document_position);
                },
                handlers::text_document::completion,
            )
            .expect("Failed to register Completion handler")
            .on::<lsp_types::request::Formatting>(handlers::text_document::formatting)
            .expect("Failed to register Formatting handler")
            .on_with::<lsp_types::request::Rename>(
                |r, params| {
                    r.ensure_beancount_data_for_position(&params.text_document_position);
                },
                handlers::text_document::handle_rename,
            )
            .expect("Failed to register Rename handler")
            .on_with::<lsp_types::request::References>(
                |r, params| {
                    r.ensure_beancount_data_for_position(&params.text_document_position);
                },
                handlers::text_document::handle_references,
            )
            .expect("Failed to register References handler")
            .on_with::<lsp_types::request::GotoDefinition>(
                |r, params| {
                    r.ensure_beancount_data_for_position(&params.text_document_position_params);
                },
                handlers::text_document::handle_definition,
            )
            .expect("Failed to register GotoDefinition handler")
            .on_with::<lsp_types::request::SemanticTokensFullRequest>(
                |r, params| {
                    r.ensure_beancount_data_for_text_document(&params.text_document);
                },
                handlers::text_document::semantic_tokens_full,
            )
            .expect("Failed to register SemanticTokens handler")
            .on::<lsp_types::request::InlayHintRequest>(handlers::text_document::inlay_hint)
            .expect("Failed to register InlayHint handler");

        router
    }

    fn ensure_checker(&mut self) -> Option<Arc<dyn BeancountChecker>> {
        if let Some(checker) = &self.checker {
            return Some(checker.clone());
        }

        self.report_progress(
            "checker auto",
            Progress::Begin,
            Some("discovering available checkers".to_string()),
            None,
        );

        let checker = create_checker(&self.config.bean_check, &self.config.root_dir);
        let checker = checker.map(|checker| {
            let checker_name = checker.name().to_string();
            let checker: Arc<dyn BeancountChecker> = Arc::from(checker);
            self.checker = Some(checker.clone());

            self.report_progress(
                "checker auto",
                Progress::End,
                Some(format!("using {checker_name}")),
                None,
            );

            checker
        });

        if checker.is_none() {
            self.report_progress(
                "checker auto",
                Progress::End,
                Some("no checker available".to_string()),
                None,
            );
        }

        checker
    }

    /// Ensure BeancountData is extracted for the given URI.
    /// Lazily extracts on first access after tree changes (lazy extraction for #757).
    pub(crate) fn ensure_beancount_data(&mut self, uri: &PathBuf) {
        // If data already exists, no need to extract
        if self.beancount_data.contains_key(uri) {
            return;
        }

        // Extract on-demand
        if let (Some(tree), Some(doc)) = (self.forest.get(uri), self.open_docs.get(uri)) {
            let beancount_data = BeancountData::new(tree, &doc.content);
            self.beancount_data
                .insert(uri.clone(), Arc::new(beancount_data));
            tracing::debug!("Lazy extraction: BeancountData extracted for {:?}", uri);
        }
    }

    fn ensure_beancount_data_for_text_document(
        &mut self,
        text_document: &lsp_types::TextDocumentIdentifier,
    ) {
        if let Ok(path) = text_document.uri.to_file_path() {
            self.ensure_beancount_data(&path);
        }
    }

    fn ensure_beancount_data_for_position(
        &mut self,
        params: &lsp_types::TextDocumentPositionParams,
    ) {
        self.ensure_beancount_data_for_text_document(&params.text_document);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::document::Document;
    use ropey::Rope;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    fn create_test_state() -> LspServerState {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let config = Config::new(PathBuf::from("/test"));
        LspServerState::new(sender, config)
    }

    fn create_test_tree(content: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .expect("Failed to set language");
        parser.parse(content, None).expect("Failed to parse")
    }

    #[test]
    fn test_lazy_extraction_skips_if_data_exists() {
        let mut state = create_test_state();
        let uri = PathBuf::from("/test/file.beancount");

        // Create test data
        let content = "2024-01-01 open Assets:Checking USD\n";
        let tree = create_test_tree(content);
        let doc = Document {
            content: Rope::from_str(content),
            version: 1,
        };

        // Setup state
        state.forest.insert(uri.clone(), Arc::new(tree));
        state.open_docs.insert(uri.clone(), doc);

        // Extract once
        state.ensure_beancount_data(&uri);
        assert!(state.beancount_data.contains_key(&uri));

        // Get pointer to the Arc
        let first_ptr = Arc::as_ptr(state.beancount_data.get(&uri).unwrap());

        // Call again - should skip extraction
        state.ensure_beancount_data(&uri);

        // Verify same Arc (pointer equality means no re-extraction)
        let second_ptr = Arc::as_ptr(state.beancount_data.get(&uri).unwrap());
        assert_eq!(first_ptr, second_ptr, "Data should not be re-extracted");
    }

    #[test]
    fn test_lazy_extraction_extracts_if_missing() {
        let mut state = create_test_state();
        let uri = PathBuf::from("/test/file.beancount");

        // Create test data
        let content = "2024-01-01 open Assets:Checking USD\n";
        let tree = create_test_tree(content);
        let doc = Document {
            content: Rope::from_str(content),
            version: 1,
        };

        // Setup state without data
        state.forest.insert(uri.clone(), Arc::new(tree));
        state.open_docs.insert(uri.clone(), doc);

        // Verify data doesn't exist yet
        assert!(!state.beancount_data.contains_key(&uri));

        // Extract on-demand
        state.ensure_beancount_data(&uri);

        // Verify data was extracted
        assert!(state.beancount_data.contains_key(&uri));
        let data = state.beancount_data.get(&uri).unwrap();
        let accounts = data.get_accounts();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], "Assets:Checking");
    }

    #[test]
    fn test_lazy_extraction_handles_missing_tree() {
        let mut state = create_test_state();
        let uri = PathBuf::from("/test/file.beancount");

        // Create doc but no tree
        let content = "2024-01-01 open Assets:Checking USD\n";
        let doc = Document {
            content: Rope::from_str(content),
            version: 1,
        };
        state.open_docs.insert(uri.clone(), doc);

        // Try to extract - should not panic
        state.ensure_beancount_data(&uri);

        // Data should not be extracted
        assert!(!state.beancount_data.contains_key(&uri));
    }

    #[test]
    fn test_lazy_extraction_handles_missing_doc() {
        let mut state = create_test_state();
        let uri = PathBuf::from("/test/file.beancount");

        // Create tree but no doc
        let content = "2024-01-01 open Assets:Checking USD\n";
        let tree = create_test_tree(content);
        state.forest.insert(uri.clone(), Arc::new(tree));

        // Try to extract - should not panic
        state.ensure_beancount_data(&uri);

        // Data should not be extracted
        assert!(!state.beancount_data.contains_key(&uri));
    }
}
