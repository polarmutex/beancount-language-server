use crate::core;
use lspower::lsp;
use thiserror::Error;

/// Runtime errors for the LSP server.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Error, PartialEq)]
pub enum Error {
    /// Error that occurs when [`core::Session.client`] is accessed and is `None`.
    #[error("ClientNotInitialzed")]
    ClientNotInitialized,
    /// Error that occurs when a session resource is requested and does not exist.
    #[error("core::SessionResourceNotFound: kind={kind:?}, uri={uri:?}")]
    SessionResourceNotFound {
        /// The kind of the requested session resource.
        kind: core::session::SessionResourceKind,
        /// The URL of the requested session resource.
        uri: lsp::Url,
    },

    #[error("Cannot convert URI to file path")]
    UriToPathConversion,
}

/// Wrapper struct for converting [`anyhow::Error`] into [`lspower::jsonrpc::Error`].
pub struct IntoJsonRpcError(pub anyhow::Error);

impl From<IntoJsonRpcError> for lspower::jsonrpc::Error {
    fn from(error: IntoJsonRpcError) -> Self {
        let mut rpc_error = lspower::jsonrpc::Error::internal_error();
        rpc_error.data = Some(serde_json::to_value(format!("{}", error.0)).unwrap());
        rpc_error
    }
}
