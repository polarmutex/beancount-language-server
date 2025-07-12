use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use lsp_types::Location;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
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

    #[allow(clippy::mutable_key_type)]
    let mut changes = std::collections::HashMap::new();
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
    forest: &HashMap<PathBuf, tree_sitter::Tree>,
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
