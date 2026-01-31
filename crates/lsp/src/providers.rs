pub mod completion;
/// Provider definitions for LSP `textDocument/definition`.
pub mod definition;
/// Provider definitions for LSP `textDocument/publishDiagnostics`.
pub mod diagnostics;
/// Provider definitions for LSP `textDocument/documentSymbol`.
pub mod document_symbol;
/// Provider definitions for LSP `textDocument/foldingRange`.
pub mod folding_range;
pub mod formatting;
/// Provider definitions for LSP `textDocument/hover`.
pub mod hover;
/// Provider definitions for LSP `textDocument/inlayHint`.
pub mod inlay_hints;
/// Provider definitions for LSP `textDocument/references` and `textDocument/rename`.
pub mod references;
/// Provider definitions for LSP semantic tokens (syntax highlighting).
pub mod semantic_tokens;
/// Provider definitions for LSP text document lifecycle events.
pub mod text_document;
/// Utilities for cross-platform URI handling.
pub mod uri;
/// Provider definitions for LSP `workspace/symbol`.
pub mod workspace_symbol;
