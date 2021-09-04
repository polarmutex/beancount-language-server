use crate::core;
use log::debug;
use lspower::lsp;
use std::sync::Arc;

/// Provider function for LSP ``.
pub async fn completion(
    session: Arc<core::Session>,
    params: lsp::CompletionParams,
) -> anyhow::Result<Option<lsp::CompletionResponse>> {
    debug!("providers::completion");

    let tree = session
        .get_mut_tree(&params.text_document_position.text_document.uri)
        .await?;
    let mut tree = tree.lock().await;
    let line = params.text_document_position.position.line as usize;
    debug!("providers::completion - line {}", line);
    let char = params.text_document_position.position.character as usize;
    debug!("providers::completion - char {}", char);
    let start = tree_sitter::Point {
        row: line,
        column: if char == 0 { char } else { char - 1 },
    };
    debug!("providers::completion - start {}", start);
    let end = tree_sitter::Point {
        row: line,
        column: char,
    };
    debug!("providers::completion - end {}", end);
    let is_character_triggered = params
        .context
        .and_then(|c| c.trigger_character)
        .and_then(|c| if c == ":" { Some(()) } else { None })
        .is_some();
    debug!("providers::completion - is_char_triggered {}", is_character_triggered);
    let node = tree.root_node().named_descendant_for_point_range(start, end);
    debug!("providers::completion - node {:?}", node);

    match node {
        Some(node) => {
            if is_character_triggered {
                debug!("providers::completion - handle trigger char");
                Ok(None)
            } else {
                debug!("providers::completion - handle node");
                Ok(None)
            }
        },
        None => Ok(None),
    }
}
