use crate::from_json;
use crate::server::LspServerState;
use crate::server::LspServerStateSnapshot;
use crate::server::Task;
use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

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

// A helper struct to dispatch LSP requests to functions.
pub(crate) struct RequestRouter {
    handlers: HashMap<String, DispatchHandler>,
}

type DispatchHandler =
    Box<dyn Fn(&mut LspServerState, lsp_server::Request) + Send + Sync + 'static>;

impl RequestRouter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    fn insert_handler(&mut self, method: &'static str, handler: DispatchHandler) -> Result<()> {
        if self.handlers.contains_key(method) {
            anyhow::bail!("duplicate handler registered for method: {method}");
        }
        self.handlers.insert(method.to_string(), handler);
        Ok(())
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
        self.insert_handler(
            R::METHOD,
            Box::new(
                move |state, req| match from_json::<R::Params>(R::METHOD, req.params) {
                    Ok(params) => {
                        let result = f(state, params);
                        let response = result_to_response::<R>(req.id, result);
                        state.respond(response);
                    }
                    Err(err) => {
                        let response = lsp_server::Response::new_err(
                            req.id,
                            lsp_server::ErrorCode::InvalidParams as i32,
                            err.to_string(),
                        );
                        state.respond(response);
                    }
                },
            ),
        )?;
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
        self.insert_handler(
            R::METHOD,
            Box::new(
                move |state, req| match from_json::<R::Params>(R::METHOD, req.params) {
                    Ok(params) => {
                        let id = req.id;
                        let snapshot = state.snapshot();
                        let sender = state.task_sender.clone();
                        state.thread_pool.execute(move || {
                            let result = f(snapshot, params);
                            if let Err(e) =
                                sender.send(Task::Response(result_to_response::<R>(id, result)))
                            {
                                tracing::error!("Failed to send response: {}", e);
                            }
                        });
                    }
                    Err(err) => {
                        let response = lsp_server::Response::new_err(
                            req.id,
                            lsp_server::ErrorCode::InvalidParams as i32,
                            err.to_string(),
                        );
                        state.respond(response);
                    }
                },
            ),
        )?;
        Ok(self)
    }

    // Try to dispatch the event as the given Request type on the thread pool, with a pre-hook
    // that can use the parsed params on the main thread before dispatch.
    pub fn on_with<R>(
        &mut self,
        pre: fn(&mut LspServerState, &R::Params),
        f: fn(LspServerStateSnapshot, R::Params) -> Result<R::Result>,
    ) -> Result<&mut Self>
    where
        R: lsp_types::request::Request + 'static,
        R::Params: DeserializeOwned + Send + 'static,
        R::Result: Serialize + 'static,
    {
        self.insert_handler(
            R::METHOD,
            Box::new(
                move |state, req| match from_json::<R::Params>(R::METHOD, req.params) {
                    Ok(params) => {
                        pre(state, &params);

                        let id = req.id;
                        let snapshot = state.snapshot();
                        let sender = state.task_sender.clone();
                        state.thread_pool.execute(move || {
                            let result = f(snapshot, params);
                            if let Err(e) =
                                sender.send(Task::Response(result_to_response::<R>(id, result)))
                            {
                                tracing::error!("Failed to send response: {}", e);
                            }
                        });
                    }
                    Err(err) => {
                        let response = lsp_server::Response::new_err(
                            req.id,
                            lsp_server::ErrorCode::InvalidParams as i32,
                            err.to_string(),
                        );
                        state.respond(response);
                    }
                },
            ),
        )?;
        Ok(self)
    }

    // Dispatches a single request by method.
    pub fn dispatch(&self, state: &mut LspServerState, req: lsp_server::Request) {
        if let Some(handler) = self.handlers.get(req.method.as_str()) {
            handler(state, req);
        } else {
            tracing::error!("unknown request: {:?}", req);
            let response = lsp_server::Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                "unknown request".to_string(),
            );
            state.respond(response);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use lsp_types::request::Request;
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn parse_mismatched_params_returns_invalid_params() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut state = LspServerState::new(sender, Config::new(PathBuf::from("/test")));

        let request = lsp_server::Request {
            id: lsp_server::RequestId::from(1),
            method: lsp_types::request::Completion::METHOD.to_string(),
            params: json!({"unexpected": "value"}),
        };

        state
            .req_queue
            .incoming
            .register(request.id.clone(), (request.method.clone(), Instant::now()));

        let mut router = RequestRouter::new();
        router
            .on::<lsp_types::request::Completion>(|_, _| Ok(None))
            .unwrap();
        router.dispatch(&mut state, request);

        let response = match receiver.recv().expect("response should be sent") {
            lsp_server::Message::Response(response) => response,
            other => panic!("expected response, got {other:?}"),
        };

        let error = response.error.expect("expected error response");
        assert_eq!(
            error.code,
            lsp_server::ErrorCode::InvalidParams as i32,
            "mismatched params should return InvalidParams"
        );
    }
}
