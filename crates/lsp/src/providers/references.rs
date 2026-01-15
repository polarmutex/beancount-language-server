use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use lsp_types::Location;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tracing::debug;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

/// Provider function for `textDocument/references`.
pub(crate) fn references(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::ReferenceParams,
) -> Result<Option<Vec<lsp_types::Location>>> {
    let uri = params
        .text_document_position
        .text_document
        .uri
        .to_file_path()
        .unwrap();
    let line = params.text_document_position.position.line;
    let char = params.text_document_position.position.character;
    let forest = snapshot.forest;
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
    let Some(node) = forest
        .get(&uri)
        .expect("to have tree found")
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };
    let content = snapshot.open_docs.get(&uri).unwrap().content.clone();
    let node_text = text_for_tree_sitter_node(&content, &node);
    let open_docs = snapshot.open_docs;
    let locs = find_references(&forest, &open_docs, node_text);
    Ok(Some(locs))
}

/// Provider function for `textDocument/rename`.
#[allow(clippy::mutable_key_type)]
pub(crate) fn rename(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::RenameParams,
) -> Result<Option<lsp_types::WorkspaceEdit>> {
    let uri = &params
        .text_document_position
        .text_document
        .uri
        .to_file_path()
        .unwrap();
    let line = &params.text_document_position.position.line;
    let char = &params.text_document_position.position.character;
    let forest = snapshot.forest;
    let _tree = forest.get(uri).unwrap();
    let open_docs = snapshot.open_docs;
    let doc = open_docs.get(uri).unwrap();
    let content = doc.clone().content;
    let start = tree_sitter::Point {
        row: *line as usize,
        column: if *char == 0 {
            *char as usize
        } else {
            *char as usize - 1
        },
    };
    let end = tree_sitter::Point {
        row: *line as usize,
        column: *char as usize,
    };
    let Some(node) = forest
        .get(uri)
        .expect("to have tree found")
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };
    let node_text = text_for_tree_sitter_node(&content, &node);
    let locs = find_references(&forest, &open_docs, node_text);
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
        let uri = lsp_types::Uri::from_str(&uri_str).unwrap();
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
    node_text: String,
) -> Vec<lsp_types::Location> {
    forest
        .iter()
        .flat_map(|(url, tree)| {
            let query = match tree_sitter::Query::new(
                &tree_sitter_beancount::language(),
                "(account)@account",
            ) {
                Ok(q) => q,
                Err(_e) => return vec![],
            };
            let capture_account = query
                .capture_index_for_name("account")
                .expect("account should be captured");
            let text = if open_docs.get(url).is_some() {
                open_docs.get(url).unwrap().text().to_string()
            } else {
                match std::fs::read_to_string(url) {
                    Ok(content) => content,
                    Err(_) => {
                        // If file read fails, return empty results
                        debug!("Failed to read file: {:?}", url);
                        return vec![];
                    }
                }
            };
            let source = text.as_bytes();
            {
                let mut query_cursor = tree_sitter::QueryCursor::new();
                let mut matches = query_cursor.matches(&query, tree.root_node(), source);
                let mut results = Vec::new();
                while let Some(m) = matches.next() {
                    if let Some(node) = m.nodes_for_capture_index(capture_account).next() {
                        let m_text = node.utf8_text(source).expect("");
                        if m_text == node_text {
                            results.push((url.clone(), node));
                        }
                    }
                }
                results
            }
        })
        .map(|(url, node): (PathBuf, tree_sitter::Node)| {
            let range = node.range();
            Location::new(
                {
                    // Handle cross-platform file URI creation
                    let file_path_str = url.to_str().unwrap();
                    let uri_str = if cfg!(windows)
                        && file_path_str.len() > 1
                        && file_path_str.chars().nth(1) == Some(':')
                    {
                        // Windows absolute path like "C:\path"
                        format!("file:///{}", file_path_str.replace('\\', "/"))
                    } else if cfg!(windows) && file_path_str.starts_with('/') {
                        // Unix-style path on Windows, convert to Windows style
                        format!("file:///C:{}", file_path_str.replace('\\', "/"))
                    } else {
                        // Unix path or other platforms
                        format!("file://{file_path_str}")
                    };
                    lsp_types::Uri::from_str(&uri_str).unwrap()
                },
                lsp_types::Range {
                    start: lsp_types::Position {
                        line: range.start_point.row as u32,
                        character: range.start_point.column as u32,
                    },
                    end: lsp_types::Position {
                        line: range.end_point.row as u32,
                        character: range.end_point.column as u32,
                    },
                },
            )
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
            "Assets:Checking".to_string(),
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
            "Assets:Nonexistent".to_string(),
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

        let locs = find_references(&forest, &open_docs, "Assets:Bank".to_string());

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

        let uri =
            lsp_types::Uri::from_str(&format!("file://{}", state.path.to_string_lossy())).unwrap();
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

        let uri =
            lsp_types::Uri::from_str(&format!("file://{}", state.path.to_string_lossy())).unwrap();
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

        let uri =
            lsp_types::Uri::from_str(&format!("file://{}", state.path.to_string_lossy())).unwrap();

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
            "Expenses:Food".to_string(),
        );
        assert_eq!(locs_food.len(), 2); // open + posting

        let locs_cash = find_references(
            &state.snapshot.forest,
            &state.snapshot.open_docs,
            "Assets:Cash".to_string(),
        );
        assert_eq!(locs_cash.len(), 2); // open + posting
    }
}
