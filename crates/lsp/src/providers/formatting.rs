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
/// This function behaves like bean-format by default, performing targeted formatting on Beancount files:
/// 1. Aligning amounts in postings and balance directives like bean-format
/// 2. Supporting bean-format's prefix-width, num-width, and currency-column options
/// 3. Maintaining consistent spacing and indentation
/// 4. Generating minimal text edits that only modify what needs to change
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

    // Get formatting configuration from the LSP snapshot
    let format_config = &snapshot.config.formatting;

    // Calculate maximum widths like bean-format does
    let auto_max_prefix_width = match_pairs
        .iter()
        .filter_map(|m| m.prefix.as_ref())
        .map(|prefix| prefix.end.column)
        .max()
        .unwrap_or(0);

    let auto_max_number_width = match_pairs
        .iter()
        .filter_map(|m| m.number.as_ref())
        .map(|number| number.end.column - number.start.column)
        .max()
        .unwrap_or(0);

    // Use configuration overrides if provided (like bean-format's -w and -W options)
    let max_prefix_width = format_config.prefix_width.unwrap_or(auto_max_prefix_width);
    let max_number_width = format_config.num_width.unwrap_or(auto_max_number_width);

    // Account-amount spacing (like bean-format's default)
    let spacing = format_config.account_amount_spacing;

    let mut text_edits = Vec::new();

    // Handle currency column alignment if specified (like bean-format's -c option)
    if let Some(currency_col) = format_config.currency_column {
        // Currency column mode: align currencies at the specified column
        for match_pair in &match_pairs {
            if let (Some(prefix), Some(number)) = (&match_pair.prefix, &match_pair.number) {
                // Find the actual currency position in the text to properly calculate alignment
                let line_start_char = doc.content.line_to_char(number.end.row);
                let line_end_char = if number.end.row + 1 < doc.content.len_lines() {
                    doc.content.line_to_char(number.end.row + 1)
                } else {
                    doc.content.len_chars()
                };
                let line_text = doc
                    .content
                    .slice(line_start_char..line_end_char)
                    .to_string();

                // Find where the currency actually starts in this line
                let currency_start_in_line =
                    if let Some(pos) = line_text[number.end.column..].find(char::is_alphabetic) {
                        number.end.column + pos
                    } else {
                        // Fallback: assume currency is right after number with configured spacing
                        number.end.column + format_config.number_currency_spacing
                    };

                // Calculate how much we need to move the number to align the currency at the target column
                let target_number_start = if currency_start_in_line >= currency_col {
                    // Currency is already past the target, don't try to fix it
                    number.start.column
                } else {
                    let currency_offset = currency_start_in_line - number.end.column;
                    currency_col
                        .saturating_sub((number.end.column - number.start.column) + currency_offset)
                };
                let current_number_start = number.start.column;

                let insert_pos = lsp_types::Position {
                    line: prefix.end.row as u32,
                    character: prefix.end.column as u32,
                };

                match target_number_start.cmp(&current_number_start) {
                    Ordering::Greater => {
                        let spaces_needed = target_number_start - current_number_start;
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
                        let spaces_to_remove = current_number_start - target_number_start;
                        let end_pos = lsp_types::Position {
                            line: insert_pos.line,
                            character: insert_pos.character + spaces_to_remove as u32,
                        };

                        if let Some(edit) = create_removal_edit(&doc.content, insert_pos, end_pos) {
                            text_edits.push(edit);
                        }
                    }
                    Ordering::Equal => {
                        // Already properly aligned
                    }
                }
            }
        }
    } else {
        // Default mode: right-align numbers like bean-format's default behavior
        let number_start_column = max_prefix_width + spacing;

        for match_pair in &match_pairs {
            if let (Some(prefix), Some(number)) = (&match_pair.prefix, &match_pair.number) {
                let num_len = number.end.column - number.start.column;
                let current_number_start = number.start.column;

                // Right-align: position number so it ends at the same column
                let target_number_start = number_start_column + (max_number_width - num_len);

                let insert_pos = lsp_types::Position {
                    line: prefix.end.row as u32,
                    character: prefix.end.column as u32,
                };

                match target_number_start.cmp(&current_number_start) {
                    Ordering::Greater => {
                        let spaces_needed = target_number_start - current_number_start;
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
                        let spaces_to_remove = current_number_start - target_number_start;
                        let end_pos = lsp_types::Position {
                            line: insert_pos.line,
                            character: insert_pos.character + spaces_to_remove as u32,
                        };

                        if let Some(edit) = create_removal_edit(&doc.content, insert_pos, end_pos) {
                            text_edits.push(edit);
                        }
                    }
                    Ordering::Equal => {
                        // Already properly aligned
                    }
                }
            }
        }
    }

    // Adjust spacing between numbers and currencies if configured
    if format_config.number_currency_spacing != 1 {
        for match_pair in &match_pairs {
            if let Some(number) = &match_pair.number {
                let line_start_char = doc.content.line_to_char(number.end.row);
                let line_end_char = if number.end.row + 1 < doc.content.len_lines() {
                    doc.content.line_to_char(number.end.row + 1)
                } else {
                    doc.content.len_chars()
                };
                let line_text = doc
                    .content
                    .slice(line_start_char..line_end_char)
                    .to_string();

                // Find where the currency starts after the number
                if let Some(currency_pos) = line_text[number.end.column..].find(char::is_alphabetic)
                {
                    let actual_currency_start = number.end.column + currency_pos;
                    let current_spacing = currency_pos;
                    let target_spacing = format_config.number_currency_spacing;

                    if current_spacing != target_spacing {
                        let number_end_pos = lsp_types::Position {
                            line: number.end.row as u32,
                            character: number.end.column as u32,
                        };
                        let currency_start_pos = lsp_types::Position {
                            line: number.end.row as u32,
                            character: actual_currency_start as u32,
                        };

                        if current_spacing > target_spacing {
                            // Remove excess spaces
                            let spaces_to_remove = current_spacing - target_spacing;
                            let remove_end_pos = lsp_types::Position {
                                line: number.end.row as u32,
                                character: (number.end.column + spaces_to_remove) as u32,
                            };

                            if let Some(edit) =
                                create_removal_edit(&doc.content, number_end_pos, remove_end_pos)
                            {
                                text_edits.push(edit);
                            }
                        } else {
                            // Add more spaces
                            let spaces_to_add = target_spacing - current_spacing;
                            let edit = lsp_types::TextEdit {
                                range: lsp_types::Range {
                                    start: number_end_pos,
                                    end: number_end_pos,
                                },
                                new_text: " ".repeat(spaces_to_add),
                            };
                            text_edits.push(edit);
                        }
                    }
                }
            }
        }
    }

    debug!("Generated {} text edits for formatting", text_edits.len());
    Ok(Some(text_edits))
}

/// Helper function to create a text edit that removes whitespace safely.
/// Returns None if the text to remove contains non-whitespace characters.
fn create_removal_edit(
    content: &ropey::Rope,
    start_pos: lsp_types::Position,
    end_pos: lsp_types::Position,
) -> Option<lsp_types::TextEdit> {
    let start_char = content.line_to_char(start_pos.line as usize) + start_pos.character as usize;
    let end_char = content.line_to_char(end_pos.line as usize) + end_pos.character as usize;

    if end_char <= content.len_chars() {
        let text_to_remove = content.slice(start_char..end_char);
        // Only remove if it's all whitespace
        if text_to_remove.to_string().trim().is_empty() {
            return Some(lsp_types::TextEdit {
                range: lsp_types::Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: String::new(),
            });
        }
    }
    None
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

        fn new_with_config(
            content: &str,
            format_config: crate::config::FormattingConfig,
        ) -> anyhow::Result<Self> {
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

            let mut config = Config::new(PathBuf::from("/test"));
            config.formatting = format_config;

            let snapshot = LspServerStateSnapshot {
                beancount_data,
                config,
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

    #[test]
    fn test_bean_format_prefix_width_override() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0 USD
  Assets:Bank
"#;

        // Test with custom prefix width of 30
        let format_config = crate::config::FormattingConfig {
            prefix_width: Some(30),
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Prefix width 30 formatted:\n{formatted}");

        // Verify that accounts are aligned to at least column 30
        let lines: Vec<&str> = formatted.lines().collect();
        for line in lines.iter().skip(1) {
            if line.trim().is_empty() || !line.contains("USD") {
                continue;
            }

            if let Some(amount_pos) = line.find(char::is_numeric) {
                // The amount should start at or after column 30 + spacing
                assert!(
                    amount_pos >= 30 + 2,
                    "Amount should start at column {} or later, but found at {}",
                    30 + 2,
                    amount_pos
                );
            }
        }
    }

    #[test]
    fn test_bean_format_num_width_override() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash     100.00 USD
  Expenses:Food 5.0 USD
  Income:Job -1000.00 USD
"#;

        // Test with custom number width of 12
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: Some(12),
            currency_column: None,
            account_amount_spacing: 2,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Number width 12 formatted:\n{formatted}");

        // Verify that all numbers are right-aligned within the 12-character width
        let lines: Vec<&str> = formatted.lines().collect();
        let mut number_end_positions = Vec::new();

        for line in &lines[1..] {
            if let Some(usd_pos) = line.find(" USD") {
                number_end_positions.push(usd_pos);
            }
        }

        // All numbers should end at the same position (right-aligned)
        if number_end_positions.len() > 1 {
            let first_end = number_end_positions[0];
            for &end_pos in &number_end_positions[1..] {
                assert_eq!(
                    end_pos, first_end,
                    "All numbers should be right-aligned at the same end column with num-width override"
                );
            }
        }
    }

    #[test]
    fn test_bean_format_currency_column_alignment() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0 USD
  Income:Job -1000.00 USD
"#;

        // Test with currency column alignment at column 50
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: Some(50),
            account_amount_spacing: 2,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Currency column 50 formatted:\n{formatted}");

        // Verify that currencies are aligned at column 50
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if let Some(usd_pos) = line.find("USD") {
                assert_eq!(
                    usd_pos, 50,
                    "Currency should be aligned at column 50, but found at {usd_pos}"
                );
            }
        }
    }

    #[test]
    fn test_bean_format_combined_options() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0 USD
  Assets:Bank
"#;

        // Test with combined prefix-width and currency-column
        let format_config = crate::config::FormattingConfig {
            prefix_width: Some(25),
            num_width: None,
            currency_column: Some(40),
            account_amount_spacing: 3,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Combined options formatted:\n{formatted}");

        // Verify that currencies are aligned at the specified column
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if let Some(usd_pos) = line.find("USD") {
                assert_eq!(
                    usd_pos, 40,
                    "Currency should be aligned at column 40 with combined options"
                );
            }
        }
    }

    #[test]
    fn test_bean_format_account_amount_spacing() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash 100.00 USD
  Expenses:Food 50.0 USD
"#;

        // Test with custom account-amount spacing
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 5,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Custom spacing formatted:\n{formatted}");

        // Verify that there's at least the specified spacing between account and amount
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if line.trim().is_empty() || !line.contains("USD") {
                continue;
            }

            // Find the end of the account name (before the spaces)
            let account_part = line.trim_start();
            if let Some(first_space) = account_part.find(' ') {
                let spaces_after_account = &account_part[first_space..];
                let actual_spacing =
                    spaces_after_account.len() - spaces_after_account.trim_start().len();

                assert!(
                    actual_spacing >= 5,
                    "Should have at least 5 spaces between account and amount, but found {actual_spacing}"
                );
            }
        }
    }

    #[test]
    fn test_bean_format_balance_directive_formatting() {
        let content = r#"2023-01-01 balance Assets:Cash 1000.00 USD
2023-01-01 balance Assets:Bank:Checking 500.0 USD
"#;

        // Test bean-format configuration with balance directives
        let format_config = crate::config::FormattingConfig {
            prefix_width: Some(35),
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Balance directive formatted:\n{formatted}");

        // Verify balance directives are formatted with bean-format options
        let lines: Vec<&str> = formatted.lines().collect();
        if lines.len() >= 2 {
            let line1 = lines[0];
            let line2 = lines[1];

            if let (Some(pos1), Some(pos2)) = (line1.find("1000.00"), line2.find("500.0")) {
                let end1 = pos1 + "1000.00".len();
                let end2 = pos2 + "500.0".len();
                // Numbers in balance directives should be right-aligned
                assert_eq!(
                    end1, end2,
                    "Balance amounts should be right-aligned with bean-format config"
                );
            }
        }
    }

    #[test]
    fn test_number_currency_spacing() {
        let content = r#"2023-01-01 * "Test"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0   USD
"#;

        // Test with 2 spaces between number and currency
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 2,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Number-currency spacing formatted:\n{formatted}");

        // Verify that there are exactly 2 spaces between numbers and currencies
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if line.contains("USD") {
                // Find the pattern "number  USD" (with exactly 2 spaces)
                if let Some(usd_pos) = line.find("USD") {
                    let before_usd = &line[..usd_pos];
                    // Should end with exactly 2 spaces
                    assert!(
                        before_usd.ends_with("  "),
                        "Should have exactly 2 spaces before USD in line: '{line}'"
                    );
                    // Should not have 3 or more spaces
                    assert!(
                        !before_usd.ends_with("   "),
                        "Should not have 3 or more spaces before USD in line: '{line}'"
                    );
                }
            }
        }
    }

    #[test]
    fn test_number_currency_spacing_zero() {
        let content = r#"2023-01-01 * "Test"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0 USD
"#;

        // Test with 0 spaces between number and currency (no space)
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 0,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Zero spacing formatted:\n{formatted}");

        // Verify that there are no spaces between numbers and currencies
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if line.contains("USD") {
                // Should find pattern like "100.00USD" (no space)
                assert!(
                    line.contains("100.00USD") || line.contains("50.0USD"),
                    "Should have no space between number and USD in line: '{line}'"
                );
            }
        }
    }

    #[test]
    fn test_currency_alignment_precision() {
        let content = r#"2023-01-01 * "Test"
  Assets:Cash     100.00 USD
  Expenses:Food 50.0 USD
"#;

        // Test currency alignment at an exact column
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: Some(30),
            account_amount_spacing: 2,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Precise currency alignment:\n{formatted}");

        // Print each line with column numbers for debugging
        for (i, line) in formatted.lines().enumerate() {
            println!("Line {i}: '{line}'");
            if line.contains("USD") {
                let usd_pos = line.find("USD").unwrap();
                println!("       USD at column: {usd_pos}");
                assert_eq!(usd_pos, 30, "Currency should be exactly at column 30");
            }
        }
    }
}
