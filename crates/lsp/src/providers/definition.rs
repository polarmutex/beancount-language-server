use crate::beancount_data::get_unified_query;
use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use anyhow::{Context, anyhow};
use lsp_types::GotoDefinitionResponse;
use lsp_types::Location;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::NodeKind;
use tree_sitter_beancount::tree_sitter;
use url::Url;

/// Provider function for `textDocument/definition`.
pub(crate) fn definition(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let doc_uri = &params.text_document_position_params.text_document.uri;
    let uri = doc_uri.to_file_path().map_err(|_| {
        anyhow!(
            "failed to convert document URI to file path for definition: {}",
            doc_uri.as_str()
        )
    })?;
    let line = params.text_document_position_params.position.line;
    let char = params.text_document_position_params.position.character;
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
    let tree = forest
        .get(&uri)
        .with_context(|| format!("missing syntax tree for file: {}", uri.display()))?;
    let Some(node) = tree
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };

    if NodeKind::Account != node.kind().into() {
        return Ok(None);
    }

    let content = snapshot
        .open_docs
        .get(&uri)
        .with_context(|| format!("missing open document content for file: {}", uri.display()))?
        .content
        .clone();
    let node_text = text_for_tree_sitter_node(&content, &node);
    let open_docs = snapshot.open_docs;
    let locs = find_account_open_definitions(&forest, &open_docs, node_text);
    if locs.is_empty() {
        return Ok(None);
    }
    Ok(Some(GotoDefinitionResponse::Array(locs)))
}

fn find_account_open_definitions(
    forest: &HashMap<PathBuf, Arc<tree_sitter::Tree>>,
    open_docs: &HashMap<PathBuf, Document>,
    node_text: String,
) -> Vec<Location> {
    forest
        .iter()
        .flat_map(|(url, tree)| {
            let query = get_unified_query();
            let capture_account = match query.capture_index_for_name("account") {
                Some(index) => index,
                None => {
                    tracing::warn!("Query missing capture 'account'");
                    return vec![];
                }
            };
            let text = if let Some(doc) = open_docs.get(url) {
                doc.text().to_string()
            } else {
                match std::fs::read_to_string(url) {
                    Ok(content) => content,
                    Err(_) => {
                        tracing::debug!("Failed to read file: {:?}", url);
                        return vec![];
                    }
                }
            };
            let source = text.as_bytes();
            let mut query_cursor = tree_sitter::QueryCursor::new();
            let mut matches = query_cursor.matches(query, tree.root_node(), source);
            let mut results = Vec::new();
            while let Some(m) = matches.next() {
                if let Some(node) = m.nodes_for_capture_index(capture_account).next() {
                    let m_text = match node.utf8_text(source) {
                        Ok(text) => text,
                        Err(err) => {
                            tracing::debug!("Failed to read node text: {err}");
                            continue;
                        }
                    };
                    if m_text == node_text {
                        results.push((url.clone(), node));
                    }
                }
            }
            results
        })
        .filter_map(|(url, node): (PathBuf, tree_sitter::Node)| {
            let range = node.range();
            let uri = Url::from_file_path(&url)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_ref()).ok());
            let Some(uri) = uri else {
                tracing::debug!("Failed to convert file path to URI: {}", url.display());
                return None;
            };
            Some(Location::new(
                uri,
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
            ))
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;
    use tree_sitter::Parser;

    fn make_tree(text: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        parser.parse(text, None).unwrap()
    }

    fn make_doc(text: &str) -> Document {
        Document {
            content: Rope::from_str(text),
            version: 1,
        }
    }

    #[test]
    fn test_find_account_open_definitions_single_match() {
        let text = "2024-01-01 open Assets:Cash\n";
        let path = std::env::temp_dir().join("definition_test.bean");
        let tree = Arc::new(make_tree(text));

        let mut forest = HashMap::new();
        forest.insert(path.clone(), tree);

        let mut open_docs = HashMap::new();
        open_docs.insert(path.clone(), make_doc(text));

        let locs = find_account_open_definitions(&forest, &open_docs, "Assets:Cash".to_string());

        assert_eq!(locs.len(), 1);
        let loc = &locs[0];
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 16);
        assert_eq!(loc.range.end.line, 0);
        assert_eq!(loc.range.end.character, 27);

        let expected_uri = Url::from_file_path(&path)
            .ok()
            .and_then(|u| lsp_types::Uri::from_str(u.as_ref()).ok())
            .unwrap();
        assert_eq!(loc.uri, expected_uri);
    }

    #[test]
    fn test_find_account_open_definitions_multiple_files() {
        let text_a = "2024-01-01 open Assets:Cash\n";
        let text_b = "2024-01-02 open Assets:Cash\n";
        let path_a = std::env::temp_dir().join("definition_test_a.bean");
        let path_b = std::env::temp_dir().join("definition_test_b.bean");

        let mut forest = HashMap::new();
        forest.insert(path_a.clone(), Arc::new(make_tree(text_a)));
        forest.insert(path_b.clone(), Arc::new(make_tree(text_b)));

        let mut open_docs = HashMap::new();
        open_docs.insert(path_a, make_doc(text_a));
        open_docs.insert(path_b, make_doc(text_b));

        let locs = find_account_open_definitions(&forest, &open_docs, "Assets:Cash".to_string());

        assert_eq!(locs.len(), 2);
    }

    #[test]
    fn test_find_account_open_definitions_no_match() {
        let text = "2024-01-01 open Assets:Cash\n";
        let path = std::env::temp_dir().join("definition_test_none.bean");
        let tree = Arc::new(make_tree(text));

        let mut forest = HashMap::new();
        forest.insert(path.clone(), tree);

        let mut open_docs = HashMap::new();
        open_docs.insert(path, make_doc(text));

        let locs =
            find_account_open_definitions(&forest, &open_docs, "Liabilities:Card".to_string());

        assert!(locs.is_empty());
    }
}
