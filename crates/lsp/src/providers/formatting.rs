use crate::server::LspServerStateSnapshot;
use crate::utils::ToFilePath;
use anyhow::Result;
use std::cmp::Ordering;
use tracing::debug;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

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
impl<'a> tree_sitter::TextProvider<&'a [u8]> for RopeProvider<'a> {
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

/// Provider function for LSP `textDocument/formatting`.
///
/// This function performs targeted formatting on Beancount files by:
/// 1. Aligning amounts in postings and balance directives
/// 2. Maintaining consistent spacing and indentation
/// 3. Generating minimal text edits that only modify what needs to change
pub(crate) fn formatting(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::DocumentFormattingParams,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    debug!("providers::formatting");

    let uri = match params.text_document.uri.to_file_path() {
        Ok(path) => path,
        Err(_) => {
            debug!(
                "Failed to convert URI to file path: {:?}",
                params.text_document.uri
            );
            return Ok(None);
        }
    };

    let tree = match snapshot.forest.get(&uri) {
        Some(tree) => tree,
        None => {
            debug!("No tree found for URI: {:?}", uri);
            return Ok(None);
        }
    };

    let doc = match snapshot.open_docs.get(&uri) {
        Some(doc) => doc,
        None => {
            debug!("No document found for URI: {:?}", uri);
            return Ok(None);
        }
    };

    let query = match tree_sitter::Query::new(&tree.language(), QUERY_STR) {
        Ok(query) => query,
        Err(e) => {
            debug!("Failed to create tree-sitter query: {}", e);
            return Ok(None);
        }
    };

    let mut query_cursor = tree_sitter::QueryCursor::new();
    let mut matches = query_cursor.matches(
        &query,
        tree.root_node(),
        RopeProvider(doc.content.get_slice(..).unwrap()),
    );

    let mut match_pairs: Vec<Match> = Vec::new();
    while let Some(matched) = matches.next() {
        let mut prefix: Option<TSRange> = None;
        let mut number: Option<TSRange> = None;
        for capture in matched.captures {
            let capture_name = query.capture_names()[capture.index as usize];
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

    // If no matches found, no formatting needed
    if match_pairs.is_empty() {
        debug!("No formatting matches found");
        return Ok(Some(vec![]));
    }

    // Find the maximum width of prefixes and numbers for proper alignment
    let mut max_prefix_width = 0;
    let mut max_number_width = 0;

    for match_pair in match_pairs.iter() {
        if let (Some(prefix), Some(number)) = (&match_pair.prefix, &match_pair.number) {
            let prefix_len = prefix.end.column;
            if prefix_len > max_prefix_width {
                max_prefix_width = prefix_len;
            }
            let number_len = number.end.column - number.start.column;
            if number_len > max_number_width {
                max_number_width = number_len;
            }
        }
    }

    // Configuration: spacing between account and amount
    let prefix_number_buffer = 2;
    let correct_number_placement = max_prefix_width + prefix_number_buffer;

    let mut text_edits = Vec::new();
    for match_pair in match_pairs {
        if let (Some(prefix), Some(number)) = (match_pair.prefix, match_pair.number) {
            let num_len = number.end.column - number.start.column;
            let num_col_pos = number.start.column;
            let new_num_pos = correct_number_placement + (max_number_width - num_len);

            let insert_pos = lsp_types::Position {
                line: prefix.end.row as u32,
                character: prefix.end.column as u32,
            };

            match new_num_pos.cmp(&num_col_pos) {
                Ordering::Greater => {
                    // Insert spaces to align numbers properly
                    let spaces_needed = new_num_pos - num_col_pos;
                    let edit = lsp_types::TextEdit {
                        range: lsp_types::Range {
                            start: insert_pos,
                            end: insert_pos,
                        },
                        new_text: " ".repeat(spaces_needed),
                    };
                    text_edits.push(edit);
                }
                Ordering::Less => {
                    // Remove excess spaces
                    let spaces_to_remove = num_col_pos - new_num_pos;
                    let end_pos = lsp_types::Position {
                        line: insert_pos.line,
                        character: insert_pos.character + spaces_to_remove as u32,
                    };

                    // Validate that we're only removing whitespace
                    let start_char = doc.content.line_to_char(insert_pos.line as usize)
                        + insert_pos.character as usize;
                    let end_char = doc.content.line_to_char(end_pos.line as usize)
                        + end_pos.character as usize;

                    if end_char <= doc.content.len_chars() {
                        let text_to_remove = doc.content.slice(start_char..end_char);
                        // Only remove if it's all whitespace
                        if text_to_remove.to_string().trim().is_empty() {
                            let edit = lsp_types::TextEdit {
                                range: lsp_types::Range {
                                    start: insert_pos,
                                    end: end_pos,
                                },
                                new_text: String::new(),
                            };
                            text_edits.push(edit);
                        }
                    }
                }
                Ordering::Equal => {
                    // Already properly aligned, no edit needed
                }
            }
        }
    }

    debug!("Generated {} text edits for formatting", text_edits.len());
    Ok(Some(text_edits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use crate::server::LspServerStateSnapshot;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;
    use tree_sitter_beancount::tree_sitter;

    struct TestState {
        snapshot: LspServerStateSnapshot,
    }

    impl TestState {
        fn new(content: &str) -> anyhow::Result<Self> {
            let path = PathBuf::from("/test.beancount");
            let rope_content = ropey::Rope::from_str(content);

            // Parse the content with tree-sitter
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(content, None).unwrap();

            // Create the necessary data structures
            let mut forest = HashMap::new();
            forest.insert(path.clone(), tree.clone());

            let mut open_docs = HashMap::new();
            open_docs.insert(
                path.clone(),
                Document {
                    content: rope_content.clone(),
                },
            );

            let mut beancount_data = HashMap::new();
            beancount_data.insert(path.clone(), BeancountData::new(&tree, &rope_content));

            let snapshot = LspServerStateSnapshot {
                beancount_data,
                config: Config::new(PathBuf::from("/test")),
                forest,
                open_docs,
            };

            Ok(TestState { snapshot })
        }

        fn format(&self) -> anyhow::Result<Option<Vec<lsp_types::TextEdit>>> {
            let params = lsp_types::DocumentFormattingParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: lsp_types::Uri::from_str("file:///test.beancount").unwrap(),
                },
                options: lsp_types::FormattingOptions {
                    tab_size: 4,
                    insert_spaces: true,
                    properties: std::collections::HashMap::new(),
                    trim_trailing_whitespace: Some(false),
                    insert_final_newline: Some(false),
                    trim_final_newlines: Some(false),
                },
                work_done_progress_params: lsp_types::WorkDoneProgressParams {
                    work_done_token: None,
                },
            };

            // Create a new snapshot for each test call
            let snapshot = LspServerStateSnapshot {
                beancount_data: self.snapshot.beancount_data.clone(),
                config: self.snapshot.config.clone(),
                forest: self.snapshot.forest.clone(),
                open_docs: self.snapshot.open_docs.clone(),
            };

            formatting(snapshot, params)
        }
    }

    fn apply_edits(content: &str, edits: &[lsp_types::TextEdit]) -> String {
        let rope = ropey::Rope::from_str(content);
        let mut sorted_edits = edits.to_vec();

        // Sort edits in reverse order by position to avoid invalidating positions
        sorted_edits.sort_by(|a, b| {
            let line_cmp = b.range.start.line.cmp(&a.range.start.line);
            if line_cmp == std::cmp::Ordering::Equal {
                b.range.start.character.cmp(&a.range.start.character)
            } else {
                line_cmp
            }
        });

        let mut result = rope;
        for edit in sorted_edits {
            let start_line = edit.range.start.line as usize;
            let start_char = edit.range.start.character as usize;
            let end_line = edit.range.end.line as usize;
            let end_char = edit.range.end.character as usize;

            let start_char_idx = result.line_to_char(start_line) + start_char;
            let end_char_idx = result.line_to_char(end_line) + end_char;

            // Remove the old text
            if start_char_idx < end_char_idx {
                result.remove(start_char_idx..end_char_idx);
            }

            // Insert the new text
            if !edit.new_text.is_empty() {
                result.insert(start_char_idx, &edit.new_text);
            }
        }

        result.to_string()
    }

    #[test]
    fn test_formatting_basic_alignment() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0 USD
  Assets:Bank
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Should generate edits to align the numbers
        assert!(!edits.is_empty(), "Should generate formatting edits");

        let formatted = apply_edits(content, &edits);
        println!("Original:\n{content}");
        println!("Formatted:\n{formatted}");

        // Verify that numbers are right-aligned (end positions should be the same)
        let lines: Vec<&str> = formatted.lines().collect();
        if lines.len() >= 3 {
            let line1 = lines[1]; // Assets:Cash line
            let line2 = lines[2]; // Expenses:Food line

            if let (Some(pos1), Some(pos2)) = (line1.find("100.00"), line2.find("50.0")) {
                let end1 = pos1 + "100.00".len();
                let end2 = pos2 + "50.0".len();
                // The end positions of numbers should be aligned (right-aligned)
                assert_eq!(
                    end1, end2,
                    "Numbers should be right-aligned at the same end column"
                );
            }
        }
    }

    #[test]
    fn test_formatting_already_aligned() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash      100.00 USD
  Expenses:Food     50.0 USD
  Assets:Bank
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Should generate minimal or no edits if already aligned
        let formatted = apply_edits(content, &edits);

        // The formatting should preserve good alignment
        let lines: Vec<&str> = formatted.lines().collect();
        if lines.len() >= 3 {
            let line1 = lines[1];
            let line2 = lines[2];

            if let (Some(pos1), Some(pos2)) = (line1.find("100.00"), line2.find("50.0")) {
                let end1 = pos1 + "100.00".len();
                let end2 = pos2 + "50.0".len();
                assert_eq!(end1, end2, "Numbers should remain right-aligned");
            }
        }
    }

    #[test]
    fn test_formatting_balance_directive() {
        let content = r#"2023-01-01 balance Assets:Cash 1000.00 USD
2023-01-01 balance Assets:Bank 500.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("Balance formatted:\n{formatted}");

        // Verify balance directives are formatted
        let lines: Vec<&str> = formatted.lines().collect();
        if lines.len() >= 2 {
            let line1 = lines[0];
            let line2 = lines[1];

            if let (Some(pos1), Some(pos2)) = (line1.find("1000.00"), line2.find("500.0")) {
                let end1 = pos1 + "1000.00".len();
                let end2 = pos2 + "500.0".len();
                // Numbers in balance directives should be right-aligned
                assert_eq!(end1, end2, "Balance amounts should be right-aligned");
            }
        }
    }

    #[test]
    fn test_formatting_mixed_lengths() {
        let content = r#"2023-01-01 * "Mixed length accounts"
  Assets:Cash:Checking:Account:Name 100.00 USD
  Expenses:Food 50.0 USD
  Assets:Very:Long:Account:Name:Here 25.123 USD
  Income:Job
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("Mixed lengths formatted:\n{formatted}");

        // Verify all numbers are right-aligned
        let lines: Vec<&str> = formatted.lines().collect();
        let mut number_end_positions = Vec::new();

        for line in &lines[1..=3] {
            // Skip first line and last line
            // Find numbers with USD currency
            if let Some(usd_pos) = line.find(" USD") {
                // Find the end of the number (before " USD")
                number_end_positions.push(usd_pos);
            }
        }

        // All numbers should end at the same position (right-aligned)
        if number_end_positions.len() > 1 {
            let first_end = number_end_positions[0];
            for &end_pos in &number_end_positions[1..] {
                assert_eq!(
                    end_pos, first_end,
                    "All numbers should be right-aligned at the same end column"
                );
            }
        }
    }

    #[test]
    fn test_formatting_no_amounts() {
        let content = r#"2023-01-01 * "No amounts"
  Assets:Cash
  Expenses:Food
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Should return empty edits if no amounts to format
        assert!(
            edits.is_empty(),
            "Should not generate edits when no amounts present"
        );
    }

    #[test]
    fn test_formatting_preserves_non_whitespace() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash     100.00 USD  ; Comment
  Expenses:Food 50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);

        // Should preserve comments and other non-whitespace content
        assert!(formatted.contains("; Comment"), "Should preserve comments");
        assert!(formatted.contains("USD"), "Should preserve currency codes");
    }

    #[test]
    fn test_formatting_empty_file() {
        let content = "";

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Should handle empty files gracefully
        assert!(edits.is_empty(), "Should not generate edits for empty file");
    }

    #[test]
    fn test_formatting_only_metadata() {
        let content = r#"1900-01-01 open Assets:Cash

2023-01-01 close Assets:Cash
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Should handle files with only directives that don't have amounts
        assert!(
            edits.is_empty(),
            "Should not generate edits for files without amounts"
        );
    }

    #[test]
    fn test_edit_positions() {
        let content = r#"2023-01-01 * "Test"
  Assets:Cash 100.00 USD
  Expenses:Food   50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Verify that edit positions are valid (line and character are u32, so always >= 0)
        for edit in &edits {
            assert!(
                edit.range.end.line >= edit.range.start.line,
                "End line should be >= start line"
            );

            if edit.range.end.line == edit.range.start.line {
                assert!(
                    edit.range.end.character >= edit.range.start.character,
                    "End character should be >= start character on same line"
                );
            }
        }
    }
}
