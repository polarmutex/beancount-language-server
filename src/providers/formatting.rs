use crate::core;
use log::debug;
use std::cmp::Ordering;
use std::sync::Arc;
use tower_lsp::lsp_types;

struct TSRange {
    pub start: tree_sitter::Point,
    pub end: tree_sitter::Point,
}

struct Match {
    prefix: Option<TSRange>,
    number: Option<TSRange>,
}

const QUERY_STR: &str = r#"
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

// Adapter to convert rope chunks to bytes
struct ChunksBytes<'a> {
    chunks: ropey::iter::Chunks<'a>,
}
impl<'a> Iterator for ChunksBytes<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.next().map(str::as_bytes)
    }
}

struct RopeProvider<'a>(ropey::RopeSlice<'a>);
impl<'a> tree_sitter::TextProvider<'a> for RopeProvider<'a> {
    type I = ChunksBytes<'a>;

    fn text(&mut self, node: tree_sitter::Node) -> Self::I {
        let start_char = self.0.byte_to_char(node.start_byte());
        let end_char = self.0.byte_to_char(node.end_byte());
        let fragment = self.0.slice(start_char..end_char);
        ChunksBytes {
            chunks: fragment.chunks(),
        }
    }
}

/// Provider function for LSP ``.
pub(crate) async fn formatting(
    session: Arc<core::Session>,
    params: lsp_types::DocumentFormattingParams,
) -> anyhow::Result<Option<Vec<lsp_types::TextEdit>>> {
    debug!("providers::formatting");

    let uri = params.text_document.uri;
    let tree = session.get_mut_tree(&uri).await?;
    let tree = tree.lock().await;
    let doc = session.get_document(&uri).await?;

    let query = tree_sitter::Query::new(tree.language(), QUERY_STR).unwrap();
    let mut query_cursor = tree_sitter::QueryCursor::new();
    let matches = query_cursor.matches(
        &query,
        tree.root_node(),
        RopeProvider(doc.content.get_slice(..).unwrap()),
    );

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

            let insert_pos = lsp_types::Position {
                line: prefix.end.row as u32,
                character: prefix.end.column as u32,
            };

            match new_num_pos.cmp(&num_col_pos) {
                Ordering::Greater => {
                    // Insert Spaces
                    let edit = lsp_types::TextEdit {
                        range: lsp_types::Range {
                            start: insert_pos,
                            end: insert_pos,
                        },
                        new_text: " ".repeat(new_num_pos - num_col_pos),
                    };
                    text_edits.push(edit)
                },
                Ordering::Less => {
                    // remove spaces
                    // TODO conform text will not be deleted
                    let end_pos = lsp_types::Position {
                        line: insert_pos.line,
                        character: insert_pos.character + (num_col_pos - new_num_pos) as u32,
                    };
                    let edit = lsp_types::TextEdit {
                        range: lsp_types::Range {
                            start: insert_pos,
                            end: end_pos,
                        },
                        new_text: "".to_string(),
                    };
                    text_edits.push(edit)
                },
                Ordering::Equal => {},
            }
        }
    }

    Ok(Some(text_edits))
}
