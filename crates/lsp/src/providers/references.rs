use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::{
    lsp_position_to_tree_sitter_point_range, text_for_tree_sitter_node,
    tree_sitter_node_to_lsp_range,
};
use crate::utils::file_path_to_uri;
use anyhow::{Context, Result};
use lsp_types::Location;
use ropey::Rope;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use tracing::debug;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

fn cached_account_query() -> &'static (tree_sitter::Query, u32) {
    static ACCOUNT_QUERY: OnceLock<(tree_sitter::Query, u32)> = OnceLock::new();
    ACCOUNT_QUERY.get_or_init(|| {
        let query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), "(account)@account")
                .expect("account query should compile");
        let capture_index = query
            .capture_index_for_name("account")
            .expect("account should be captured");
        (query, capture_index)
    })
}

fn cached_tag_query() -> &'static (tree_sitter::Query, u32) {
    static TAG_QUERY: OnceLock<(tree_sitter::Query, u32)> = OnceLock::new();
    TAG_QUERY.get_or_init(|| {
        let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), "(tag)@tag")
            .expect("tag query should compile");
        let capture_index = query
            .capture_index_for_name("tag")
            .expect("tag should be captured");
        (query, capture_index)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceKind {
    Account,
    Tag,
}

fn reference_node_at_position<'a>(
    tree: &'a tree_sitter::Tree,
    content: &Rope,
    position: lsp_types::Position,
) -> Result<Option<(ReferenceKind, tree_sitter::Node<'a>)>> {
    let (start, end) = lsp_position_to_tree_sitter_point_range(content, position)?;
    let Some(mut node) = tree
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };

    // The cursor may land on a child node; walk up until we find a referenceable node.
    loop {
        match node.kind() {
            "account" => return Ok(Some((ReferenceKind::Account, node))),
            "tag" => return Ok(Some((ReferenceKind::Tag, node))),
            _ => {}
        }

        if let Some(parent) = node.parent() {
            node = parent;
            continue;
        }
        return Ok(None);
    }
}

fn reference_text_at_position(
    tree: &tree_sitter::Tree,
    content: &Rope,
    position: lsp_types::Position,
) -> Result<Option<(ReferenceKind, String)>> {
    let Some((kind, node)) = reference_node_at_position(tree, content, position)? else {
        return Ok(None);
    };
    Ok(Some((kind, text_for_tree_sitter_node(content, &node))))
}

/// Provider function for `textDocument/references`.
pub(crate) fn references(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::ReferenceParams,
) -> Result<Option<Vec<lsp_types::Location>>> {
    let uri = &params.text_document_position.text_document.uri;
    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(e) => {
            debug!("References: failed to get tree/document for uri: {e}");
            return Ok(None);
        }
    };
    let content = doc.content.clone();

    // Keep behavior consistent: references only works on open documents.
    let position = params.text_document_position.position;
    let Some((kind, node_text)) = reference_text_at_position(tree, &content, position)
        .with_context(|| {
            format!(
                "failed to get node text at position for uri: {}",
                uri.as_str()
            )
        })?
    else {
        return Ok(None);
    };

    let locs = find_references(&snapshot.forest, &snapshot.open_docs, kind, &node_text);
    Ok(Some(locs))
}

/// Provider function for `textDocument/rename`.
#[allow(clippy::mutable_key_type)]
pub(crate) fn rename(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::RenameParams,
) -> Result<Option<lsp_types::WorkspaceEdit>> {
    let uri = &params.text_document_position.text_document.uri;
    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(e) => {
            debug!("Rename: failed to get tree/document for uri: {e}");
            return Ok(None);
        }
    };

    let content = doc.content.clone();
    let position = params.text_document_position.position;
    let Some((kind, node_text)) = reference_text_at_position(tree, &content, position)
        .with_context(|| {
            format!(
                "failed to get node text at position for uri: {}",
                uri.as_str()
            )
        })?
    else {
        return Ok(None);
    };

    let locs = find_references(&snapshot.forest, &snapshot.open_docs, kind, &node_text);
    let new_name = params.new_name;

    // Group locations by URI string to avoid mutable key type warning
    let mut grouped_locs: std::collections::HashMap<String, Vec<lsp_types::Location>> =
        std::collections::HashMap::new();
    for loc in locs {
        grouped_locs
            .entry(loc.uri.to_string())
            .or_default()
            .push(loc);
    }

    let mut changes: std::collections::HashMap<lsp_types::Uri, Vec<lsp_types::TextEdit>> =
        std::collections::HashMap::new();
    for (uri_str, locations) in grouped_locs {
        let uri = match lsp_types::Uri::from_str(&uri_str) {
            Ok(uri) => uri,
            Err(e) => {
                debug!("Failed to parse URI string {}: {}", uri_str, e);
                continue;
            }
        };
        let mut edits: Vec<_> = locations
            .into_iter()
            .map(|l| lsp_types::TextEdit::new(l.range, new_name.clone()))
            .collect();
        // Send edits ordered from the back so we do not invalidate following positions.
        edits.sort_by_key(|edit| edit.range.start);
        edits.reverse();
        changes.insert(uri, edits);
    }
    Ok(Some(lsp_types::WorkspaceEdit::new(changes)))
}

/// Find all references to a given text in the project using tree-sitter queries.
fn find_references(
    forest: &HashMap<PathBuf, Arc<tree_sitter::Tree>>,
    open_docs: &HashMap<PathBuf, Document>,
    kind: ReferenceKind,
    node_text: &str,
) -> Vec<lsp_types::Location> {
    forest
        .iter()
        .flat_map(|(url, tree)| {
            let (query, capture_index) = match kind {
                ReferenceKind::Account => cached_account_query(),
                ReferenceKind::Tag => cached_tag_query(),
            };

            let (rope, text) = if let Some(doc) = open_docs.get(url) {
                let rope = doc.content.clone();
                let text = rope.to_string();
                (rope, text)
            } else {
                match std::fs::read_to_string(url) {
                    Ok(content) => {
                        let rope = Rope::from_str(&content);
                        (rope, content)
                    }
                    Err(_) => {
                        debug!("Failed to read file: {:?}", url);
                        return vec![];
                    }
                }
            };

            let source = text.as_bytes();

            let mut query_cursor = tree_sitter::QueryCursor::new();
            let mut matches = query_cursor.matches(query, tree.root_node(), source);
            let mut results = Vec::new();
            while let Some(m) = matches.next() {
                if let Some(node) = m.nodes_for_capture_index(*capture_index).next() {
                    let m_text = node.utf8_text(source).expect("");
                    if m_text == node_text {
                        results.push((url.clone(), rope.clone(), node));
                    }
                }
            }

            results
        })
        .filter_map(|(url, rope, node): (PathBuf, Rope, tree_sitter::Node)| {
            let uri = file_path_to_uri(&url).ok()?;
            let range = tree_sitter_node_to_lsp_range(&rope, &node);
            Some(Location::new(uri, range))
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use std::collections::HashMap;

    struct TestState {
        snapshot: LspServerStateSnapshot,
        path: PathBuf,
    }

    impl TestState {
        fn new(content: &str) -> anyhow::Result<Self> {
            let path = std::env::current_dir()?.join("test.beancount");
            let rope_content = ropey::Rope::from_str(content);

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
    fn test_find_references_single_account() {
        let content = r#"
2024-01-01 open Assets:Checking
2024-01-02 * "Test"
  Assets:Checking  100.00 USD
  Expenses:Food   -100.00 USD
"#;
        let state = TestState::new(content).unwrap();
        let locs = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            ReferenceKind::Account,
            "Assets:Checking",
        );

        assert_eq!(locs.len(), 2); // open + posting
        assert!(locs[0].range.start.line == 1 || locs[1].range.start.line == 1);
        assert!(locs[0].range.start.line == 3 || locs[1].range.start.line == 3);
    }

    #[test]
    fn test_find_references_no_matches() {
        let content = r#"
2024-01-01 open Assets:Checking
2024-01-02 * "Test"
  Assets:Checking  100.00 USD
"#;
        let state = TestState::new(content).unwrap();
        let locs = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            ReferenceKind::Account,
            "Assets:Nonexistent",
        );

        assert_eq!(locs.len(), 0);
    }

    #[test]
    fn test_find_references_multiple_files() {
        let content1 = r#"
2024-01-01 open Assets:Bank
2024-01-02 * "Test"
  Assets:Bank  100.00 USD
"#;
        let content2 = r#"
2024-01-03 * "Another"
  Assets:Bank  50.00 USD
"#;
        let path1 = std::env::current_dir().unwrap().join("test1.beancount");
        let path2 = std::env::current_dir().unwrap().join("test2.beancount");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();

        let tree1 = parser.parse(content1, None).unwrap();
        let tree2 = parser.parse(content2, None).unwrap();

        let mut forest = HashMap::new();
        forest.insert(path1.clone(), Arc::new(tree1));
        forest.insert(path2.clone(), Arc::new(tree2));

        let mut open_docs = HashMap::new();
        open_docs.insert(
            path1,
            Document {
                content: ropey::Rope::from_str(content1),
                version: 0,
            },
        );
        open_docs.insert(
            path2,
            Document {
                content: ropey::Rope::from_str(content2),
                version: 0,
            },
        );

        let locs = find_references(&forest, &open_docs, ReferenceKind::Account, "Assets:Bank");

        assert_eq!(locs.len(), 3); // open in file1 + posting in file1 + posting in file2
    }

    #[test]
    fn test_references_handler() {
        let content = r#"
2024-01-01 open Assets:Checking
2024-01-02 * "Test"
  Assets:Checking  100.00 USD
  Expenses:Food   -100.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let uri = file_path_to_uri(&state.path).unwrap();
        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position {
                    line: 1,
                    character: 20,
                }, // Position in "Assets:Checking"
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        };

        let result = references(state.snapshot, params).unwrap();
        assert!(result.is_some());
        let locs = result.unwrap();
        assert_eq!(locs.len(), 2); // open + posting
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn test_rename_handler() {
        let content = r#"
2024-01-01 open Assets:Checking
2024-01-02 * "Test"
  Assets:Checking  100.00 USD
  Expenses:Food   -100.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let uri = file_path_to_uri(&state.path).unwrap();
        let params = lsp_types::RenameParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position {
                    line: 1,
                    character: 20,
                },
            },
            new_name: "Assets:Bank".to_string(),
            work_done_progress_params: Default::default(),
        };

        let result = rename(state.snapshot, params).unwrap();
        assert!(result.is_some());
        let edit = result.unwrap();
        assert!(edit.changes.is_some());
        let changes = edit.changes.unwrap();
        assert_eq!(changes.len(), 1);
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 2); // Rename in both locations
        assert_eq!(edits[0].new_text, "Assets:Bank");
        assert_eq!(edits[1].new_text, "Assets:Bank");
    }

    #[test]
    fn test_references_at_different_positions() {
        let content = r#"
2024-01-01 open Expenses:Food
2024-01-02 * "Lunch"
  Expenses:Food  10.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let uri = file_path_to_uri(&state.path).unwrap();

        // Test at line 1 (open directive)
        let params1 = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position {
                    line: 1,
                    character: 20,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        };

        let result1 = references(state.snapshot, params1).unwrap();
        assert!(result1.is_some());
        assert_eq!(result1.unwrap().len(), 2);
    }

    #[test]
    fn test_references_with_multiple_accounts() {
        let content = r#"
2024-01-01 open Expenses:Food
2024-01-01 open Assets:Cash
2024-01-02 * "Lunch"
  Expenses:Food  10.00 USD
  Assets:Cash   -10.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let locs_food = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            ReferenceKind::Account,
            "Expenses:Food",
        );
        assert_eq!(locs_food.len(), 2); // open + posting

        let locs_cash = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            ReferenceKind::Account,
            "Assets:Cash",
        );
        assert_eq!(locs_cash.len(), 2); // open + posting
    }

    #[test]
    fn test_find_references_single_tag() {
        let content = r#"
2024-01-02 * "Test" #Groceries
  Assets:Cash  -10.00 USD
  Expenses:Food  10.00 USD
2024-01-03 * "Another" #Groceries
  Assets:Cash  -5.00 USD
  Expenses:Food  5.00 USD
"#;
        let state = TestState::new(content).unwrap();
        let locs = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            ReferenceKind::Tag,
            "#Groceries",
        );

        assert_eq!(locs.len(), 2);
    }

    #[test]
    fn test_references_handler_for_tag() {
        let content = r#"
2024-01-02 * "Test" #Groceries
  Assets:Cash  -10.00 USD
  Expenses:Food  10.00 USD
2024-01-03 * "Another" #Groceries
  Assets:Cash  -5.00 USD
  Expenses:Food  5.00 USD
"#;
        let state = TestState::new(content).unwrap();
        let uri = file_path_to_uri(&state.path).unwrap();

        // Cursor somewhere inside "#Groceries".
        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position {
                    line: 1,
                    character: 22,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        };

        let result = references(state.snapshot, params).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn test_rename_handler_for_tag() {
        let content = r#"
2024-01-02 * "Test" #Groceries
  Assets:Cash  -10.00 USD
  Expenses:Food  10.00 USD
2024-01-03 * "Another" #Groceries
  Assets:Cash  -5.00 USD
  Expenses:Food  5.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let uri = file_path_to_uri(&state.path).unwrap();
        let params = lsp_types::RenameParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position {
                    line: 1,
                    character: 22,
                },
            },
            new_name: "#Food".to_string(),
            work_done_progress_params: Default::default(),
        };

        let result = rename(state.snapshot, params).unwrap();
        assert!(result.is_some());
        let edit = result.unwrap();
        assert!(edit.changes.is_some());
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "#Food");
        assert_eq!(edits[1].new_text, "#Food");
    }

    #[test]
    fn test_references_tag_cursor_at_end_of_line_utf16() {
        let content = r#"
2025-07-31 * "财财财-财财财财财财财财财财财财财财财财财财" #credit-cmb-2025-08
    Liabilities:A:B:C                           -2.50 CNY
    Expenses:Food
2025-08-01 * "Second" #credit-cmb-2025-08
    Liabilities:A:B:C                           -1.00 CNY
    Expenses:Food
"#;

        let state = TestState::new(content).unwrap();
        let uri = file_path_to_uri(&state.path).unwrap();

        // LSP uses UTF-16 code units for character offsets; place the cursor at end-of-line.
        let first_txn_line = content.lines().nth(1).expect("expected first txn line");
        let eol_utf16 = first_txn_line.encode_utf16().count() as u32;

        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position {
                    line: 1,
                    character: eol_utf16,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        };

        let result = references(state.snapshot, params).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }
}
