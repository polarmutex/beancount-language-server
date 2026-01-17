use crate::providers::inlay_hints::transaction_inlay_hints;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use lsp_types::{
    Hover, HoverContents, HoverParams, InlayHintLabel, MarkupContent, MarkupKind, Range,
};
use std::collections::HashSet;
use tree_sitter_beancount::{NodeKind, tree_sitter};

/// Provider function for `textDocument/hover`.
pub(crate) fn hover(
    snapshot: LspServerStateSnapshot,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let uri = match params
        .text_document_position_params
        .text_document
        .uri
        .to_file_path()
    {
        Ok(path) => path,
        Err(_) => {
            tracing::debug!("Failed to convert URI to file path");
            return Ok(None);
        }
    };

    let line = params.text_document_position_params.position.line;
    let char = params.text_document_position_params.position.character;

    let forest = snapshot.forest;
    let tree = match forest.get(&uri) {
        Some(tree) => tree,
        None => {
            tracing::warn!("Tree not found in forest: {:?}", uri);
            return Ok(None);
        }
    };

    let content = match snapshot.open_docs.get(&uri) {
        Some(doc) => doc.content.clone(),
        None => {
            tracing::warn!("Document not found in open_docs: {:?}", uri);
            return Ok(None);
        }
    };

    let start = tree_sitter::Point {
        row: line as usize,
        column: if char == 0 {
            char as usize
        } else {
            char as usize - 1
        },
    };
    let end = tree_sitter::Point {
        row: line as usize,
        column: char as usize,
    };

    let Some(node) = tree
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };

    let posting_hint = find_posting_inlay_hint(&content, node);

    let account_node = find_node_of_kind(node, NodeKind::Account);
    let Some(account_node) = account_node else {
        return Ok(None);
    };

    let account_name = text_for_tree_sitter_node(&content, &account_node);
    let notes = collect_account_notes(&snapshot.beancount_data, &account_name);

    if notes.is_empty() && posting_hint.is_none() {
        return Ok(None);
    }

    let mut sections = Vec::new();

    if !notes.is_empty() {
        sections.push(format_account_hover_text(&account_name, &notes));
    }

    if let Some(label) = posting_hint {
        sections.push(format_posting_hover_text(&label));
    }

    let hover_text = sections.join("\n\n");
    let range = Range {
        start: lsp_types::Position::new(
            account_node.start_position().row as u32,
            account_node.start_position().column as u32,
        ),
        end: lsp_types::Position::new(
            account_node.end_position().row as u32,
            account_node.end_position().column as u32,
        ),
    };

    Ok(Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: hover_text,
        }),
        range: Some(range),
    }))
}

fn find_node_of_kind<'a>(
    mut node: tree_sitter::Node<'a>,
    kind: NodeKind,
) -> Option<tree_sitter::Node<'a>> {
    loop {
        if NodeKind::from(node.kind()) == kind {
            return Some(node);
        }
        if let Some(parent) = node.parent() {
            node = parent;
        } else {
            return None;
        }
    }
}

fn collect_account_notes(
    data_map: &std::collections::HashMap<
        std::path::PathBuf,
        std::sync::Arc<crate::beancount_data::BeancountData>,
    >,
    account: &str,
) -> Vec<String> {
    let mut notes = Vec::new();
    let mut seen = HashSet::new();

    for data in data_map.values() {
        if let Some(values) = data.get_account_notes().get(account) {
            for note in values {
                if seen.insert(note.clone()) {
                    notes.push(note.clone());
                }
            }
        }
    }

    notes
}

fn format_account_hover_text(account: &str, notes: &[String]) -> String {
    if notes.len() == 1 {
        format!("**{}**\n\n{}", account, notes[0])
    } else {
        let mut text = format!("**{}**\n\nNotes:\n", account);
        for note in notes {
            text.push_str(&format!("- {}\n", note));
        }
        text
    }
}

fn format_posting_hover_text(label: &str) -> String {
    format!("**Posting hint**\n\n{}", label.trim_start())
}

fn find_posting_inlay_hint(content: &ropey::Rope, node: tree_sitter::Node) -> Option<String> {
    let posting_node = find_node_of_kind(node, NodeKind::Posting)?;
    let transaction_node = find_node_of_kind(posting_node, NodeKind::Transaction)?;
    let hints = transaction_inlay_hints(&transaction_node, content)?;
    let target_line = posting_node.start_position().row as u32;

    hints
        .into_iter()
        .filter(|hint| hint.position.line == target_line)
        .map(|hint| match hint.label {
            InlayHintLabel::String(label) => label.trim_start().to_string(),
            InlayHintLabel::LabelParts(parts) => parts.into_iter().map(|part| part.value).collect(),
        })
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use ropey::Rope;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;
    use tree_sitter_beancount::tree_sitter;
    use url::Url;

    struct TestState {
        snapshot: LspServerStateSnapshot,
        path: PathBuf,
    }

    impl TestState {
        fn new(content: &str) -> anyhow::Result<Self> {
            let path = std::env::current_dir()?.join("test.beancount");
            let rope_content = Rope::from_str(content);

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(content, None).unwrap();

            let mut forest = HashMap::new();
            forest.insert(path.clone(), Arc::new(tree.clone()));

            let mut open_docs = HashMap::new();
            open_docs.insert(
                path.clone(),
                Document {
                    content: rope_content.clone(),
                    version: 0,
                },
            );

            let mut beancount_data = HashMap::new();
            beancount_data.insert(
                path.clone(),
                Arc::new(BeancountData::new(&tree, &rope_content)),
            );

            let config = Config::new(path.clone());

            Ok(Self {
                snapshot: LspServerStateSnapshot {
                    forest,
                    open_docs,
                    beancount_data,
                    config,
                    checker: None,
                },
                path,
            })
        }
    }

    #[test]
    fn test_hover_returns_account_note() {
        let content = "2024-01-01 note Assets:Cash \"cash note\"\n2024-01-02 * \"Test\"\n  Assets:Cash  1 USD\n";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position::new(2, 4),
            },
            work_done_progress_params: Default::default(),
        };

        let result = hover(state.snapshot, params).unwrap();
        let hover = result.expect("Expected hover result");
        match hover.contents {
            HoverContents::Markup(markup) => {
                assert!(markup.value.contains("cash note"));
            }
            _ => panic!("Expected markup hover content"),
        }
    }

    #[test]
    fn test_hover_includes_posting_hint_when_missing_amount() {
        let content = "2024-01-01 * \"Test\"\n  Assets:Cash  1 USD\n  Expenses:Food\n";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position::new(2, 4),
            },
            work_done_progress_params: Default::default(),
        };

        let result = hover(state.snapshot, params).unwrap();
        let hover = result.expect("Expected hover result");
        match hover.contents {
            HoverContents::Markup(markup) => {
                assert!(
                    markup.value.contains("-1 USD"),
                    "Hover should surface the balancing hint for postings"
                );
            }
            _ => panic!("Expected markup hover content"),
        }
    }
}
