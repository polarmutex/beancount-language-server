pub mod completion;
/// Provider definitions for LSP `textDocument/publishDiagnostics`.
pub mod diagnostics;
pub mod formatting;
/// Provider definitions for LSP `textDocument/references` and `textDocument/rename`.
pub mod references;
/// Provider definitions for LSP text document lifecycle events.
pub mod text_document;
/// Utilities for cross-platform URI handling.
pub mod uri;
