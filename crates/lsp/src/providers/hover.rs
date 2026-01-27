use crate::document::Document;
use crate::providers::inlay_hints::transaction_inlay_hints;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::{
    lsp_position_to_tree_sitter_point_range, text_for_tree_sitter_node,
    tree_sitter_node_to_lsp_range,
};
use anyhow::Result;
use lsp_types::{
    Hover, HoverContents, HoverParams, InlayHintLabel, MarkupContent, MarkupKind, Range,
};
use ropey::Rope;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::{NodeKind, tree_sitter};

static COMMODITY_HOVER_QUERY: OnceLock<(tree_sitter::Query, u32, u32)> = OnceLock::new();

fn commodity_hover_query() -> (&'static tree_sitter::Query, u32, u32) {
    let cached = COMMODITY_HOVER_QUERY.get_or_init(|| {
        let query_string = "(commodity (currency) @currency) @commodity";
        let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .expect("Failed to compile commodity hover query");
        let capture_currency = query
            .capture_index_for_name("currency")
            .expect("commodity hover query should have 'currency' capture");
        let capture_commodity = query
            .capture_index_for_name("commodity")
            .expect("commodity hover query should have 'commodity' capture");
        (query, capture_currency, capture_commodity)
    });
    (&cached.0, cached.1, cached.2)
}

fn find_commodity_directive_for_currency(
    forest: &HashMap<PathBuf, Arc<tree_sitter::Tree>>,
    open_docs: &HashMap<PathBuf, Document>,
    currency: &str,
) -> Option<String> {
    let (query, capture_currency, capture_commodity) = commodity_hover_query();

    for (url, tree) in forest {
        let (text, rope) = if let Some(doc) = open_docs.get(url) {
            (doc.text().to_string(), doc.content.clone())
        } else {
            let Ok(content) = std::fs::read_to_string(url) else {
                tracing::debug!("Failed to read file: {:?}", url);
                continue;
            };
            let rope = Rope::from_str(&content);
            (content, rope)
        };

        let source = text.as_bytes();
        let mut query_cursor = tree_sitter::QueryCursor::new();
        let mut matches = query_cursor.matches(query, tree.root_node(), source);

        while let Some(m) = matches.next() {
            let Some(currency_node) = m.nodes_for_capture_index(capture_currency).next() else {
                continue;
            };
            let Some(commodity_node) = m.nodes_for_capture_index(capture_commodity).next() else {
                continue;
            };
            let Ok(m_text) = currency_node.utf8_text(source) else {
                continue;
            };
            if m_text == currency {
                return Some(text_for_tree_sitter_node(&rope, &commodity_node));
            }
        }
    }

    None
}

/// Provider function for `textDocument/hover`.
pub(crate) fn hover(
    snapshot: LspServerStateSnapshot,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let uri = &params.text_document_position_params.text_document.uri;

    let position = params.text_document_position_params.position;

    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Hover: failed to get tree/doc for uri: {e}");
            return Ok(None);
        }
    };
    let content = doc.content.clone();

    let (start, end) = lsp_position_to_tree_sitter_point_range(&content, position)?;

    let Some(node) = tree
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };

    let posting_hint = find_posting_inlay_hint(&content, node);

    let mut sections = Vec::new();

    let account_node = find_node_of_kind(node, NodeKind::Account);
    if let Some(account_node) = account_node {
        let account_name = text_for_tree_sitter_node(&content, &account_node);
        let notes = collect_account_notes(&snapshot.beancount_data, &account_name);
        if !notes.is_empty() {
            sections.push(format_account_hover_text(&account_name, &notes));
        }
    }

    if let Some(label) = posting_hint {
        sections.push(format_posting_hover_text(&label));
    }

    let currency_node = find_descendant_of_kind(node, NodeKind::Currency);
    if let Some(currency_node) = currency_node {
        let currency = text_for_tree_sitter_node(&content, &currency_node);
        if let Some(commodity_text) =
            find_commodity_directive_for_currency(&snapshot.forest, &snapshot.open_docs, &currency)
        {
            sections.push(format!("```beancount\n{}\n```", commodity_text.trim_end()));
        }
    }

    if sections.is_empty() {
        return Ok(None);
    }

    let hover_text = sections.join("\n\n");
    let range = if let Some(account_node) = find_node_of_kind(node, NodeKind::Account) {
        tree_sitter_node_to_lsp_range(&content, &account_node)
    } else if let Some(currency_node) = find_node_of_kind(node, NodeKind::Currency) {
        tree_sitter_node_to_lsp_range(&content, &currency_node)
    } else {
        tree_sitter_node_to_lsp_range(&content, &node)
    };

    Ok(Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: hover_text,
        }),
        range: Some(Range {
            start: range.start,
            end: range.end,
        }),
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

fn find_descendant_of_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: NodeKind,
) -> Option<tree_sitter::Node<'a>> {
    if NodeKind::from(node.kind()) == kind {
        return Some(node);
    }

    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            if NodeKind::from(child.kind()) == kind {
                return Some(child);
            }
            stack.push(child);
        }
    }

    None
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

    #[test]
    fn test_hover_shows_commodity_metadata_for_currency() {
        let content = "2024-01-01 commodity USD\n  name: \"US Dollar\"\n  precision: 2\n\n2024-01-02 * \"Test\" \"Test\"\n  Assets:Cash  1 USD\n";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position::new(5, 17),
            },
            work_done_progress_params: Default::default(),
        };

        let result = hover(state.snapshot, params).unwrap();
        let hover = result.expect("Expected hover result");
        match hover.contents {
            HoverContents::Markup(markup) => {
                assert!(markup.value.contains("commodity USD"));
                assert!(markup.value.contains("name: \"US Dollar\""));
                assert!(markup.value.contains("precision: 2"));
            }
            _ => panic!("Expected markup hover content"),
        }
    }
}
