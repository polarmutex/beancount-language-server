use crate::providers::references::matching_nodes;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::{
    lsp_position_to_tree_sitter_point, lsp_position_to_tree_sitter_point_range,
    text_for_tree_sitter_node, tree_sitter_node_to_lsp_range,
};
use anyhow::Result;
use lsp_types::{DocumentHighlight, DocumentHighlightKind};
use ropey::Rope;
use tracing::debug;
use tree_sitter_beancount::tree_sitter;

const SYMBOL_KINDS: [&str; 4] = ["account", "payee", "tag", "link"];

fn symbol_at_position<'t>(
    tree: &'t tree_sitter::Tree,
    content: &Rope,
    position: lsp_types::Position,
) -> Result<Option<tree_sitter::Node<'t>>> {
    let point = lsp_position_to_tree_sitter_point(content, position)?;
    let (start, end) = lsp_position_to_tree_sitter_point_range(content, position)?;

    // The cursor can be directly before or directly after the token.
    Ok([(point, point), (start, end)]
        .into_iter()
        .filter_map(|(start, end)| {
            tree.root_node()
                .named_descendant_for_point_range(start, end)
        })
        .find(|node| SYMBOL_KINDS.contains(&node.kind())))
}

fn highlight_kind(node: &tree_sitter::Node) -> DocumentHighlightKind {
    if node.kind() != "account" {
        return DocumentHighlightKind::Text;
    }
    match node.parent().map(|parent| parent.kind()) {
        Some("open" | "close") => DocumentHighlightKind::Write,
        _ => DocumentHighlightKind::Read,
    }
}

/// Provider function for `textDocument/documentHighlight`.
pub(crate) fn document_highlight(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::DocumentHighlightParams,
) -> Result<Option<Vec<DocumentHighlight>>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(e) => {
            debug!("DocumentHighlight: failed to get tree/document for uri: {e}");
            return Ok(None);
        }
    };
    let content = &doc.content;

    let position = params.text_document_position_params.position;
    if position.line as usize >= content.len_lines() {
        return Ok(None);
    }
    let Some(symbol) = symbol_at_position(tree, content, position)? else {
        return Ok(None);
    };

    let symbol_text = text_for_tree_sitter_node(content, &symbol);
    let text = content.to_string();
    let highlights = matching_nodes(tree, text.as_bytes(), symbol.kind(), &symbol_text)
        .iter()
        .map(|node| DocumentHighlight {
            range: tree_sitter_node_to_lsp_range(content, node),
            kind: Some(highlight_kind(node)),
        })
        .collect();
    Ok(Some(highlights))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    const JOURNAL: &str = r#"
2024-01-01 open Assets:Checking
2024-01-01 open Expenses:Food
2024-01-02 * "Cafe" "Lunch" #trip ^inv1
  Assets:Checking  -10.00 USD
  Expenses:Food     10.00 USD
2024-01-03 * "Cafe" "Dinner" #trip ^inv1
  Assets:Checking  -20.00 USD
  Expenses:Food     20.00 USD
2024-01-04 pad Assets:Checking Equity:Opening
2024-01-05 balance Assets:Checking  -30.00 USD
2024-01-06 close Assets:Checking
"#;

    struct TestState {
        snapshot: LspServerStateSnapshot,
        path: PathBuf,
    }

    impl TestState {
        fn new(content: &str) -> anyhow::Result<Self> {
            Self::with_other_file(content, None)
        }

        fn with_other_file(content: &str, other: Option<&str>) -> anyhow::Result<Self> {
            let cwd = std::env::current_dir()?;
            let path = cwd.join("test.beancount");

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;

            let mut forest = HashMap::new();
            let mut open_docs = HashMap::new();
            let mut beancount_data = HashMap::new();
            let files = [
                (path.clone(), Some(content)),
                (cwd.join("other.beancount"), other),
            ];
            for (file_path, file_content) in files {
                let Some(file_content) = file_content else {
                    continue;
                };
                let rope = Rope::from_str(file_content);
                let tree = parser.parse(file_content, None).unwrap();
                beancount_data.insert(
                    file_path.clone(),
                    Arc::new(BeancountData::new(&tree, &rope)),
                );
                forest.insert(file_path.clone(), Arc::new(tree));
                open_docs.insert(
                    file_path,
                    Document {
                        content: rope,
                        version: 0,
                    },
                );
            }

            Ok(Self {
                snapshot: LspServerStateSnapshot {
                    forest: Arc::new(forest),
                    forest_content: Arc::new(HashMap::new()),
                    open_docs: Arc::new(open_docs),
                    beancount_data: Arc::new(beancount_data),
                    config: Config::new(path.clone()),
                    checker: None,
                },
                path,
            })
        }

        fn highlight(self, line: u32, character: u32) -> Option<Vec<DocumentHighlight>> {
            let uri = lsp_types::Uri::from_file_path(&self.path).unwrap();
            let params = lsp_types::DocumentHighlightParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                    position: lsp_types::Position { line, character },
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            document_highlight(self.snapshot, params).unwrap()
        }
    }

    fn lines_and_kinds(highlights: &[DocumentHighlight]) -> Vec<(u32, DocumentHighlightKind)> {
        highlights
            .iter()
            .map(|h| (h.range.start.line, h.kind.unwrap()))
            .collect()
    }

    #[test]
    fn test_account_highlights_have_read_and_write_kinds() {
        let state = TestState::new(JOURNAL).unwrap();
        let highlights = state.highlight(4, 8).unwrap();

        assert_eq!(
            lines_and_kinds(&highlights),
            vec![
                (1, DocumentHighlightKind::Write),
                (4, DocumentHighlightKind::Read),
                (7, DocumentHighlightKind::Read),
                (9, DocumentHighlightKind::Read),
                (10, DocumentHighlightKind::Read),
                (11, DocumentHighlightKind::Write),
            ]
        );
        assert_eq!(
            highlights[1].range,
            lsp_types::Range::new(
                lsp_types::Position::new(4, 2),
                lsp_types::Position::new(4, 17)
            )
        );
    }

    #[test]
    fn test_account_highlight_at_token_boundaries() {
        for character in [2, 17] {
            let state = TestState::new(JOURNAL).unwrap();
            let highlights = state.highlight(4, character).unwrap();
            assert_eq!(highlights.len(), 6, "cursor at character {character}");
        }
    }

    #[test]
    fn test_payee_tag_and_link_highlights_have_text_kind() {
        // Cursor on "Cafe", #trip and ^inv1 in the first transaction.
        for character in [15, 30, 36] {
            let state = TestState::new(JOURNAL).unwrap();
            let highlights = state.highlight(3, character).unwrap();
            assert_eq!(
                lines_and_kinds(&highlights),
                vec![
                    (3, DocumentHighlightKind::Text),
                    (6, DocumentHighlightKind::Text),
                ],
                "cursor at character {character}"
            );
        }
    }

    #[test]
    fn test_non_symbol_returns_none() {
        // Date, narration and number.
        for (line, character) in [(3, 3), (3, 22), (4, 21)] {
            let state = TestState::new(JOURNAL).unwrap();
            assert!(
                state.highlight(line, character).is_none(),
                "cursor at {line}:{character}"
            );
        }
    }

    #[test]
    fn test_position_after_end_of_document_returns_none() {
        let state = TestState::new(JOURNAL).unwrap();
        assert!(state.highlight(500, 0).is_none());
    }

    #[test]
    fn test_unknown_document_returns_none() {
        let mut state = TestState::new(JOURNAL).unwrap();
        state.path = state.path.with_file_name("unknown.beancount");
        assert!(state.highlight(4, 8).is_none());
    }

    #[test]
    fn test_other_files_are_not_included() {
        let other = r#"
2024-02-01 * "Cafe" "Breakfast"
  Assets:Checking  -5.00 USD
  Expenses:Food     5.00 USD
"#;
        let state = TestState::with_other_file(JOURNAL, Some(other)).unwrap();
        let highlights = state.highlight(4, 8).unwrap();
        assert_eq!(highlights.len(), 6);
    }

    #[test]
    fn test_range_after_multibyte_text_is_utf16() {
        let content = r#"
2024-01-02 * "咖啡" "Lunch" #trip
  Assets:Checking  -10.00 USD
  Expenses:Food     10.00 USD
"#;
        let state = TestState::new(content).unwrap();
        let highlights = state.highlight(1, 28).unwrap();

        assert_eq!(highlights.len(), 1);
        assert_eq!(
            highlights[0].range,
            lsp_types::Range::new(
                lsp_types::Position::new(1, 26),
                lsp_types::Position::new(1, 31)
            )
        );
    }
}
