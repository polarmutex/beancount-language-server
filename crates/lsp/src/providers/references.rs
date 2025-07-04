use crate::document::Document;
use crate::treesitter_utils::lsp_position_to_core;
use crate::{
    server::LspServerStateSnapshot, treesitter_utils::text_for_tree_sitter_node, utils::ToFilePath,
};
use anyhow::Result;
use lsp_types::Location;
use std::str::FromStr;
use std::{collections::HashMap, path::PathBuf};
use streaming_iterator::StreamingIterator;

pub(crate) fn references(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::ReferenceParams,
) -> Result<Option<Vec<lsp_types::Location>>> {
    let uri = params.text_document_position.text_document.uri;
    let path_buf = uri.to_file_path().unwrap();
    let doc = snapshot.open_docs.get(&path_buf).unwrap();

    let position =
        lsp_position_to_core(&doc.content, params.text_document_position.position).unwrap();
    let tree = snapshot.forest.get(&path_buf).unwrap();

    let node = tree
        .root_node()
        .descendant_for_point_range(position.point, position.point)
        .unwrap();

    // TODO: honor `include_declaration` from params, right now it defaults to true

    let node_text = text_for_tree_sitter_node(&doc.content, &node);
    let open_docs = snapshot.open_docs;
    let locs = ts_references(&snapshot.forest, &open_docs, node_text);
    Ok(Some(locs))
}

pub(crate) fn ts_references(
    forest: &HashMap<PathBuf, tree_sitter::Tree>,
    open_docs: &HashMap<PathBuf, Document>,
    node_text: String,
) -> Vec<lsp_types::Location> {
    forest
        // .get(&uri)
        .iter()
        // .map(|x| (uri.clone(), x))
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
                std::fs::read_to_string(url).expect("")
            };
            let source = text.as_bytes();

            tree_sitter::QueryCursor::new()
                .matches(&query, tree.root_node(), source)
                .filter_map(|m| {
                    let m = m.nodes_for_capture_index(capture_account).next()?;
                    let m_text = m.utf8_text(source).expect("");
                    if m_text == node_text {
                        Some((url.clone(), m))
                    } else {
                        None
                    }
                })
                .cloned()
                .collect()
            // vec![]
        })
        .map(|(url, node): (PathBuf, tree_sitter::Node)| {
            let range = node.range();
            Location::new(
                lsp_types::Uri::from_str(format!("file://{}", url.to_str().unwrap()).as_str())
                    .unwrap(),
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
        // .filter(|x| true)
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use crate::providers::references::references;
    use lsp_types::{
        PartialResultParams, ReferenceContext, ReferenceParams, WorkDoneProgressParams,
    };

    use crate::test_utils::TestState;

    #[test]
    fn handle_account_references() {
        let _ = env_logger::builder().is_test(true).try_init();

        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
                |
                ^
2023-10-01 open Expenses:Test USD
2023-10-01 commodity USD

2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test                                                    1 USD
    Expenses:Test

2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test                                                    1 USD
    Expenses:Test
"#;
        let test_state = TestState::new(fixure).unwrap();
        let text_document_position = test_state.cursor().unwrap();

        assert_eq!(text_document_position.position.line, 0);
        assert_eq!(text_document_position.position.character, 16);

        let params = ReferenceParams {
            text_document_position,
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
            context: ReferenceContext {
                include_declaration: false,
            },
        };

        let references = references(test_state.snapshot, params).unwrap();
        assert!(references.is_some());
        println!("{:?}", references);
        assert_eq!(references.expect("not to be none").len(), 3);
    }
}
