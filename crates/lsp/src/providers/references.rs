use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::{
    lsp_position_to_tree_sitter_point_range, text_for_tree_sitter_node,
    tree_sitter_node_to_lsp_range,
};
use crate::utils::file_path_to_uri;
use anyhow::Result;
use lsp_types::Location;
use ropey::Rope;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
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

/// Provider function for `textDocument/references`.
pub(crate) fn references(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::ReferenceParams,
) -> Result<Option<Vec<lsp_types::Location>>> {
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "Failed to get tree/document for URI {}: {}",
                uri.as_str(),
                e
            );
            return Ok(None);
        }
    };

    let (start, end) = lsp_position_to_tree_sitter_point_range(&doc.content, position)?;

    let Some(node) = tree
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };

    let node_text = text_for_tree_sitter_node(&doc.content, &node);
    let locs = find_references(&snapshot.forest, &snapshot.open_docs, node_text.as_str());
    Ok(Some(locs))
}

/// Provider function for `textDocument/rename`.
#[allow(clippy::mutable_key_type)]
pub(crate) fn rename(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::RenameParams,
) -> Result<Option<lsp_types::WorkspaceEdit>> {
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "Failed to get tree/document for URI {}: {}",
                uri.as_str(),
                e
            );
            return Ok(None);
        }
    };

    let (start, end) = match lsp_position_to_tree_sitter_point_range(&doc.content, position) {
        Ok(range) => range,
        Err(e) => {
            tracing::warn!("Failed to convert LSP position to tree-sitter Point range: {e}");
            return Ok(None);
        }
    };

    let Some(node) = tree
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };
    let node_text = text_for_tree_sitter_node(&doc.content, &node);
    let locs = find_references(&snapshot.forest, &snapshot.open_docs, node_text.as_str());
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
                tracing::warn!("Failed to parse URI string {}: {}", uri_str, e);
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
    node_text: &str,
) -> Vec<lsp_types::Location> {
    // Decide which syntax node type to search for.
    // For now we support accounts and tags.
    let (query, capture_index) = if node_text.starts_with('#') {
        cached_tag_query()
    } else {
        cached_account_query()
    };

    let mut results: Vec<lsp_types::Location> = Vec::new();
    for (url, tree) in forest.iter() {
        let (text, rope): (String, Rope) = if let Some(doc) = open_docs.get(url) {
            (doc.text().to_string(), doc.content.clone())
        } else {
            match std::fs::read_to_string(url) {
                Ok(content) => {
                    let rope = Rope::from_str(&content);
                    (content, rope)
                }
                Err(err) => {
                    // If file read fails, skip this file and continue.
                    tracing::warn!("Skipping file due to read error: {:?}: {}", url, err);
                    continue;
                }
            }
        };

        let uri = match file_path_to_uri(url) {
            Ok(u) => u,
            Err(_) => {
                // If URI conversion fails, skip this file.
                tracing::warn!("Skipping file due to URI conversion error: {:?}", url);
                continue;
            }
        };

        let source = text.as_bytes();
        let mut query_cursor = tree_sitter::QueryCursor::new();
        let mut matches = query_cursor.matches(query, tree.root_node(), source);

        while let Some(m) = matches.next() {
            if let Some(node) = m.nodes_for_capture_index(*capture_index).next() {
                let m_text = match node.utf8_text(source) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if m_text == node_text {
                    let range = tree_sitter_node_to_lsp_range(&rope, &node);
                    results.push(Location::new(uri.clone(), range));
                }
            }
        }
    }

    results
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
            "Assets:Nonexistent",
        );

        assert_eq!(locs.len(), 0);
    }

    #[test]
    fn test_find_references_single_tag() {
        let content = r#"
2024-01-01 * "Test" #vacation #travel
    Assets:Checking  100.00 USD
    Expenses:Food   -100.00 USD

2024-01-02 * "Test2" #vacation
    Assets:Checking  50.00 USD
    Expenses:Food   -50.00 USD
"#;
        let state = TestState::new(content).unwrap();
        let locs = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            "#vacation",
        );

        assert_eq!(locs.len(), 2);
        // Tag occurrences should be on the two txn header lines.
        // Note: the raw string starts with a leading newline.
        assert!(locs.iter().any(|l| l.range.start.line == 1));
        assert!(locs.iter().any(|l| l.range.start.line == 5));
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

        let locs = find_references(&forest, &open_docs, "Assets:Bank");

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
    fn test_references_handler_tag_with_utf8_prefix_and_cursor_at_line_end() {
        let tag = "#credit-cmb-2025-08";
        let content = format!(
            r#"
2025-07-30 * "财财财财财财财财财财财财财财财财" {tag}
  Assets:Cash  -10 CNY
  Expenses:Food 10 CNY

2025-08-01 * "Other" {tag}
  Assets:Cash  -5 CNY
  Expenses:Food 5 CNY
"#
        );
        let state = TestState::new(&content).unwrap();

        // Put cursor at the end of the first tag occurrence.
        let byte_idx = content.find(tag).expect("tag should exist") + tag.len();

        let rope = &state
            .snapshot
            .open_docs
            .get(&state.path)
            .expect("doc should exist")
            .content;
        let line_idx = rope.byte_to_line(byte_idx);
        let line_char_idx = rope.line_to_char(line_idx);
        let line_utf16_cu_idx = rope.char_to_utf16_cu(line_char_idx);
        let char_idx = rope.byte_to_char(byte_idx);
        let utf16_cu_idx = rope.char_to_utf16_cu(char_idx);
        let col_utf16 = utf16_cu_idx - line_utf16_cu_idx;

        let uri = file_path_to_uri(&state.path).unwrap();

        // Verify we actually pick the tag node at the cursor.
        let tree = state.snapshot.forest.get(&state.path).unwrap();
        let (start, end) = lsp_position_to_tree_sitter_point_range(
            rope,
            lsp_types::Position {
                line: line_idx as u32,
                character: col_utf16 as u32,
            },
        )
        .unwrap();
        let node = tree
            .root_node()
            .named_descendant_for_point_range(start, end)
            .unwrap();
        let node_text = text_for_tree_sitter_node(rope, &node);
        assert_eq!(node_text, tag);

        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: lsp_types::Position {
                    line: line_idx as u32,
                    character: col_utf16 as u32,
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
        let locs = result.unwrap();
        assert_eq!(locs.len(), 2);

        // The returned highlight range should cover the whole tag.
        // Since the tag is ASCII, UTF-16 columns match byte/char counts for the tag itself.
        let expected_start = (col_utf16 as u32).saturating_sub(tag.len() as u32);
        let first_line_loc = locs
            .iter()
            .find(|l| l.range.start.line == line_idx as u32)
            .expect("should include the first tag occurrence");
        assert_eq!(first_line_loc.range.start.character, expected_start);
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
            "Expenses:Food",
        );
        assert_eq!(locs_food.len(), 2); // open + posting

        let locs_cash = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            "Assets:Cash",
        );
        assert_eq!(locs_cash.len(), 2); // open + posting
    }
}
