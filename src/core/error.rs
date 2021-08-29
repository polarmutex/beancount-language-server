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
}
