use crate::{core, core::RopeExt};
use chrono::{Datelike, NaiveDate};
use dashmap::DashMap;
use log::debug;
use lspower::lsp;
use std::sync::Arc;
use tokio::sync::Mutex;

struct TSRange {
    pub start: tree_sitter::Point,
    pub end: tree_sitter::Point,
}

struct Match {
    prefix: Option<TSRange>,
    number: Option<TSRange>,
}

const QUERY_STR: &'static str = r#"
 ( posting
                (account) @prefix
                amount: (incomplete_amount
                    [
                        (unary_number_expr)
                        (number)
                    ] @number
                )?
            )
            ( balance
                (account) @prefix
                (amount_tolerance
                    ([
                        (unary_number_expr)
                        (number)
                    ] @number)
                )
            )
"#;

/// Provider function for LSP ``.
pub async fn formatting(
    session: Arc<core::Session>,
    params: lsp::DocumentFormattingParams,
) -> anyhow::Result<Option<Vec<lsp::TextEdit>>> {
    debug!("providers::completion");

    let uri = params.text_document.uri;
    let tree = session.get_mut_tree(&uri).await?;
    let tree = tree.lock().await;
    let doc = session.get_document(&uri).await?;
    let content = doc.clone().content;

    let query = tree_sitter::Query::new(tree.language(), QUERY_STR).unwrap();
    let mut query_cursor = tree_sitter::QueryCursor::new();
    let matches = query_cursor.matches(&query, tree.root_node(), |_| &[]);

    let mut match_pairs: Vec<Match> = Vec::new();
    for matched in matches {
        let mut prefix: Option<TSRange> = None;
        let mut number: Option<TSRange> = None;
        for capture in matched.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            if capture_name == "prefix" {
                prefix = Some(TSRange {
                    start: capture.node.start_position(),
                    end: capture.node.end_position(),
                });
            } else if capture_name == "number" {
                number = Some(TSRange {
                    start: capture.node.start_position(),
                    end: capture.node.end_position(),
                });
            }
        }
        match_pairs.push(Match { prefix, number });
    }

    // TODO
    // Can we normalize the indents of the postings?
    // the optional flags kind of make this hard

    // find the max width of prefix and numbers
    let mut max_prefix_width = 0;
    let mut max_number_width = 0;

    for match_pair in match_pairs.iter() {
        if match_pair.prefix.is_some() && match_pair.number.is_some() {
            let prefix = match_pair.prefix.as_ref().unwrap();
            let mut len = prefix.end.column;
            if len > max_prefix_width {
                max_prefix_width = len;
            }
            let number = match_pair.number.as_ref().unwrap();
            len = number.end.column - number.start.column;
            if len > max_number_width {
                max_number_width = len;
            }
        }
    }

    let prefix_number_buffer = 2;
    let correct_number_placement = max_prefix_width + prefix_number_buffer;
    let mut text_edits = Vec::new();
    for match_pair in match_pairs {
        if match_pair.prefix.is_some() && match_pair.number.is_some() {
            let prefix = match_pair.prefix.as_ref().unwrap();
            let number = match_pair.number.as_ref().unwrap();
            let num_len = number.end.column - number.start.column;
            let num_col_pos = number.start.column;
            let new_num_pos = correct_number_placement + (max_number_width - num_len);

            let insert_pos = lsp::Position {
                line: prefix.end.row as u32,
                character: prefix.end.column as u32,
            };

            if new_num_pos > num_col_pos {
                // Insert Spaces
                let edit = lsp::TextEdit {
                    range: lsp::Range {
                        start: insert_pos,
                        end: insert_pos,
                    },
                    new_text: std::iter::repeat(" ")
                        .take(new_num_pos - num_col_pos)
                        .collect::<String>(),
                };
                text_edits.push(edit)
            } else if num_col_pos > new_num_pos {
                // remove spaces
                // TODO conform text will not be deleted
                let end_pos = lsp::Position {
                    line: insert_pos.line,
                    character: insert_pos.character + (num_col_pos - new_num_pos) as u32,
                };
                let edit = lsp::TextEdit {
                    range: lsp::Range {
                        start: insert_pos,
                        end: end_pos,
                    },
                    new_text: "".to_string(),
                };
                text_edits.push(edit)
            }
        }
    }

    Ok(Some(text_edits))
}
