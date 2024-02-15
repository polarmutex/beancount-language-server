use async_lsp::lsp_types::request::Request;
use async_lsp::lsp_types::TextDocumentPositionParams;
use async_lsp::lsp_types::TextEdit;

pub enum SortTransactions {}

impl Request for SortTransactions {
    type Params = TextDocumentPositionParams;
    type Result = Option<Vec<TextEdit>>;
    const METHOD: &'static str = "beancountlsp/sortTransactions";
}
