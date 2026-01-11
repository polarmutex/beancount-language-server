use crate::from_json;
use crate::server::LspServerState;
use crate::server::LspServerStateSnapshot;
use crate::server::Task;
use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn result_to_response<R>(
    id: lsp_server::RequestId,
    result: Result<R::Result>,
) -> lsp_server::Response
where
    R: lsp_types::request::Request + 'static,
    R::Params: DeserializeOwned + 'static,
    R::Result: Serialize + 'static,
{
    match result {
        Ok(resp) => lsp_server::Response::new_ok(id, &resp),
        Err(e) => lsp_server::Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

// A helper struct to  dispatch LSP requests to functions.
#[must_use = "RequestDispatcher::finish not called"]
pub(crate) struct RequestDispatcher<'a> {
    state: &'a mut LspServerState,
    request: Option<lsp_server::Request>,
}

impl<'a> RequestDispatcher<'a> {
    pub fn new(state: &'a mut LspServerState, request: lsp_server::Request) -> Self {
        RequestDispatcher {
            state,
            request: Some(request),
        }
    }

    // Tries to parse the request as the specified type.
    fn parse<R>(&mut self) -> Option<(lsp_server::RequestId, R::Params)>
    where
        R: lsp_types::request::Request + 'static,
        R::Params: DeserializeOwned + 'static,
    {
        let req = match &self.request {
            Some(req) if req.method == R::METHOD => self.request.take().unwrap(),
            _ => return None,
        };

        match from_json(R::METHOD, req.params) {
            Ok(params) => Some((req.id, params)),
            Err(err) => {
                let response = lsp_server::Response::new_err(
                    req.id,
                    lsp_server::ErrorCode::InvalidParams as i32,
                    err.to_string(),
                );
                self.state.respond(response);
                None
            }
        }
    }

    // Try to dispatch the event as the given Request type on the current thread.
    pub fn on_sync<R>(
        &mut self,
        f: fn(&mut LspServerState, R::Params) -> Result<R::Result>,
    ) -> Result<&mut Self>
    where
        R: lsp_types::request::Request + 'static,
        R::Params: DeserializeOwned + 'static,
        R::Result: Serialize + 'static,
    {
        let (id, params) = match self.parse::<R>() {
            Some(it) => it,
            None => return Ok(self),
        };
        let result = f(self.state, params);
        let response = result_to_response::<R>(id, result);
        self.state.respond(response);
        Ok(self)
    }

    // Try to dispatch the event as the given Request type on the thread pool.
    pub fn on<R>(
        &mut self,
        f: fn(LspServerStateSnapshot, R::Params) -> Result<R::Result>,
    ) -> Result<&mut Self>
    where
        R: lsp_types::request::Request + 'static,
        R::Params: DeserializeOwned + 'static + Send,
        R::Result: Serialize + 'static,
    {
        let (id, params) = match self.parse::<R>() {
            Some(it) => it,
            None => return Ok(self),
        };

        self.state.thread_pool.execute({
            let snapshot = self.state.snapshot();
            let sender = self.state.task_sender.clone();

            move || {
                let result = f(snapshot, params);
                sender
                    .send(Task::Response(result_to_response::<R>(id, result)))
                    .unwrap();
            }
        });

        Ok(self)
    }

    // If the request was not handled, report back that this is an unknown request.
    pub fn finish(&mut self) {
        if let Some(req) = self.request.take() {
            tracing::error!("unknown request: {:?}", req);
            let response = lsp_server::Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                "unknown request".to_string(),
            );
            self.state.respond(response);
        }
    }
}

// A helper struct to  dispatch LSP requests to functions.
#[must_use = "NotificationDispatcher::finish not called"]
pub(crate) struct NotificationDispatcher<'a> {
    state: &'a mut LspServerState,
    notification: Option<lsp_server::Notification>,
}

impl<'a> NotificationDispatcher<'a> {
    /// Constructs a new dispatcher for the specified request
    pub fn new(state: &'a mut LspServerState, notification: lsp_server::Notification) -> Self {
        NotificationDispatcher {
            state,
            notification: Some(notification),
        }
    }

    /// Try to dispatch the event as the given Notification type.
    pub fn on<N>(
        &mut self,
        handle_notification_fn: fn(&mut LspServerState, N::Params) -> Result<()>,
    ) -> anyhow::Result<&mut Self>
    where
        N: lsp_types::notification::Notification + 'static,
        N::Params: DeserializeOwned + Send + 'static,
    {
        let notification = match self.notification.take() {
            Some(it) => it,
            None => return Ok(self),
        };
        let params = match notification.extract::<N::Params>(N::METHOD) {
            Ok(it) => it,
            Err(lsp_server::ExtractError::JsonError { method, error }) => {
                panic!("Invalid request\nMethod: {method}\n error: {error}",)
            }
            Err(lsp_server::ExtractError::MethodMismatch(notification)) => {
                self.notification = Some(notification);
                return Ok(self);
            }
        };
        handle_notification_fn(self.state, params)?;
        Ok(self)
    }

    /// Wraps-up the dispatcher. If the notification was not handled, log an error.
    pub fn finish(&mut self) {
        if let Some(notification) = &self.notification
            && !notification.method.starts_with("$/")
        {
            tracing::error!("unhandled notification: {:?}", notification);
        }
    }
}
