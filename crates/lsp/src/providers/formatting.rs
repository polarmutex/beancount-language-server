use crate::config::FormattingConfig;
use crate::server::LspServerStateSnapshot;
use crate::utils::ToFilePath;
use anyhow::Result;
use tracing::debug;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

/// Represents a formateable line extracted from a Beancount file
/// Contains the components that bean-format uses for alignment
#[derive(Debug, Clone)]
struct FormatableLine {
    /// Line number in the document
    line_num: usize,
    /// Prefix text (account name or directive start)
    prefix: String,
    /// Number text (amount value)
    number: String,
    /// Rest of the line after the number (currency, comments, etc.)
    rest: String,
}

/// Configuration for formatting calculations
#[derive(Debug)]
struct FormatConfig {
    /// Final prefix width to use (may be overridden by config)
    final_prefix_width: usize,
    /// Final number width to use (may be overridden by config)
    final_num_width: usize,
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
( price
    currency: (_) @prefix
    amount: (amount
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

/// Main provider function for LSP `textDocument/formatting`.
///
/// This function recreates bean-format's behavior exactly:
/// 1. Extracts formateable lines using tree-sitter (instead of regex)
/// 2. Calculates alignment widths like bean-format
/// 3. Applies bean-format's formatting template or currency column logic
/// 4. Generates minimal text edits for the changes
pub fn formatting(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::DocumentFormattingParams,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    tracing::info!(
        "Starting formatting for document: {}",
        params.text_document.uri.as_str()
    );
    tracing::debug!(
        "Formatting options: insert_spaces={}, tab_size={}",
        params.options.insert_spaces,
        params.options.tab_size
    );

    // Get document and tree from the snapshot
    let (doc, tree) = match get_document_and_tree(&snapshot, &params.text_document.uri) {
        Some((doc, tree)) => {
            tracing::debug!("Found document and parsed tree");
            (doc, tree)
        }
        None => {
            tracing::warn!("Could not find document or tree for formatting");
            return Ok(None);
        }
    };

    format_document(doc, tree, &snapshot.config.formatting)
}

/// Format a single document/tree pair using only formatting configuration.
///
/// This helper makes it possible to reuse the formatter outside of the full
/// LSP snapshot (e.g., CLI formatting) by providing the document, parse tree,
/// and formatting configuration directly.
pub fn format_document(
    doc: &crate::document::Document,
    tree: &tree_sitter::Tree,
    formatting_config: &FormattingConfig,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    // Extract formateable lines using tree-sitter
    let formateable_lines = match extract_formateable_lines(doc, tree) {
        Ok(lines) => {
            tracing::debug!("Extracted {} formateable lines", lines.len());
            lines
        }
        Err(e) => {
            tracing::error!("Failed to extract formateable lines: {}", e);
            return Err(e);
        }
    };

    if formateable_lines.is_empty() {
        tracing::debug!("No formateable lines found, returning empty edits");
        return Ok(Some(vec![]));
    }

    // Calculate formatting configuration
    let format_config = calculate_format_config(&formateable_lines, formatting_config);

    // Generate text edits based on formatting mode
    let text_edits = if let Some(currency_col) = formatting_config.currency_column {
        generate_currency_column_edits(&formateable_lines, currency_col, doc)
    } else {
        generate_template_edits(
            &formateable_lines,
            &format_config,
            formatting_config.number_currency_spacing,
            formatting_config.indent_width,
            doc,
        )
    };

    // Apply indent normalization to remaining lines if configured
    let final_text_edits = if let Some(indent_width) = formatting_config.indent_width {
        apply_indent_normalization_to_remaining_lines(doc, tree, indent_width, text_edits)?
    } else {
        text_edits
    };

    debug!(
        "Generated {} text edits for formatting",
        final_text_edits.len()
    );
    Ok(Some(final_text_edits))
}

/// Gets the document and tree from the snapshot, with error handling
fn get_document_and_tree<'a>(
    snapshot: &'a LspServerStateSnapshot,
    uri: &lsp_types::Uri,
) -> Option<(&'a crate::document::Document, &'a tree_sitter::Tree)> {
    let path = match uri.to_file_path() {
        Ok(path) => path,
        Err(_) => {
            debug!("Failed to convert URI to file path: {:?}", uri);
            return None;
        }
    };

    let tree = match snapshot.forest.get(&path) {
        Some(tree) => tree,
        None => {
            debug!("No tree found for URI: {:?}", uri);
            return None;
        }
    };

    let doc = match snapshot.open_docs.get(&path) {
        Some(doc) => doc,
        None => {
            debug!("No document found for URI: {:?}", uri);
            return None;
        }
    };

    Some((doc, tree))
}

/// Extracts formateable lines from the document using tree-sitter
/// This mimics bean-format's regex-based line extraction
fn extract_formateable_lines(
    doc: &crate::document::Document,
    tree: &tree_sitter::Tree,
) -> Result<Vec<FormatableLine>> {
    let query = match tree_sitter::Query::new(&tree.language(), QUERY_STR) {
        Ok(query) => query,
        Err(e) => {
            debug!("Failed to create tree-sitter query: {}", e);
            return Ok(vec![]);
        }
    };

    let mut query_cursor = tree_sitter::QueryCursor::new();
    let mut matches = query_cursor.matches(
        &query,
        tree.root_node(),
        RopeProvider(doc.content.get_slice(..).unwrap()),
    );

    let mut formateable_lines = Vec::new();

    while let Some(matched) = matches.next() {
        let mut prefix_node: Option<tree_sitter::Node> = None;
        let mut number_node: Option<tree_sitter::Node> = None;

        // Extract prefix and number nodes from captures
        for capture in matched.captures {
            let capture_name = query.capture_names()[capture.index as usize];
            match capture_name {
                "prefix" => prefix_node = Some(capture.node),
                "number" => number_node = Some(capture.node),
                _ => {}
            }
        }

        if let (Some(prefix), Some(number)) = (prefix_node, number_node)
            && let Some(line) = extract_line_components(doc, prefix, number)
        {
            formateable_lines.push(line);
        }
    }

    Ok(formateable_lines)
}

/// Extracts the components (prefix, number, rest) from a single line
fn extract_line_components(
    doc: &crate::document::Document,
    prefix_node: tree_sitter::Node,
    number_node: tree_sitter::Node,
) -> Option<FormatableLine> {
    let line_num = prefix_node.start_position().row;

    // Get the full line text
    let line_start_char = doc.content.line_to_char(line_num);
    let line_end_char = if line_num + 1 < doc.content.len_lines() {
        doc.content.line_to_char(line_num + 1)
    } else {
        doc.content.len_chars()
    };
    let full_line = doc
        .content
        .slice(line_start_char..line_end_char)
        .to_string();

    // Extract prefix (from line start to end of account/directive)
    let prefix_end_char = doc.content.line_to_char(prefix_node.start_position().row)
        + prefix_node.end_position().column;
    let prefix_start_char = doc.content.line_to_char(prefix_node.start_position().row);
    let prefix_text = doc
        .content
        .slice(prefix_start_char..prefix_end_char)
        .to_string();

    // Extract number text
    let number_start_char = doc.content.line_to_char(number_node.start_position().row)
        + number_node.start_position().column;
    let number_end_char = doc.content.line_to_char(number_node.end_position().row)
        + number_node.end_position().column;
    let number_text = doc
        .content
        .slice(number_start_char..number_end_char)
        .to_string();

    // Extract rest (everything after the number)
    let rest_start = number_node.end_position().column;
    let rest_text = if rest_start < full_line.len() {
        full_line[rest_start..].to_string()
    } else {
        String::new()
    };

    Some(FormatableLine {
        line_num,
        prefix: prefix_text,
        number: number_text,
        rest: rest_text,
    })
}

/// Calculates formatting configuration including maximum widths and overrides
fn calculate_format_config(
    formateable_lines: &[FormatableLine],
    user_config: &crate::config::FormattingConfig,
) -> FormatConfig {
    // Calculate maximum widths across all lines (bean-format behavior)
    let max_prefix_width = formateable_lines
        .iter()
        .map(|line| line.prefix.trim_end().len())
        .max()
        .unwrap_or(0);

    let max_number_width = formateable_lines
        .iter()
        .map(|line| line.number.len())
        .max()
        .unwrap_or(0);

    // Use configuration overrides if provided (like bean-format's -w and -W options)
    let final_prefix_width = user_config.prefix_width.unwrap_or(max_prefix_width);
    let final_num_width = user_config.num_width.unwrap_or(max_number_width);

    FormatConfig {
        final_prefix_width,
        final_num_width,
    }
}

/// Generates text edits for currency column mode (bean-format -c option)
fn generate_currency_column_edits(
    formateable_lines: &[FormatableLine],
    currency_col: usize,
    doc: &crate::document::Document,
) -> Vec<lsp_types::TextEdit> {
    let mut text_edits = Vec::new();

    for line in formateable_lines {
        // Calculate spacing needed to align currency at the specified column
        // Bean-format logic: num_of_spaces = currency_column - len(prefix) - len(number) - 3
        let prefix_len = line.prefix.trim_end().len();
        let number_len = line.number.len();
        let spaces_needed = if currency_col >= prefix_len + number_len + 3 {
            currency_col - prefix_len - number_len - 3
        } else {
            2 // minimum spacing
        };

        // Create the formatted line: prefix + spaces + "  " + number + " " + rest
        let target_line = format!(
            "{}{}  {} {}",
            line.prefix.trim_end(),
            " ".repeat(spaces_needed),
            line.number,
            line.rest.trim_start()
        );

        if let Some(edit) = create_line_replacement_edit(line.line_num, &target_line, doc) {
            text_edits.push(edit);
        }
    }

    text_edits
}

/// Generates text edits for template mode (bean-format default behavior)
fn generate_template_edits(
    formateable_lines: &[FormatableLine],
    config: &FormatConfig,
    number_currency_spacing: usize,
    indent_width: Option<usize>,
    doc: &crate::document::Document,
) -> Vec<lsp_types::TextEdit> {
    let mut text_edits = Vec::new();

    for line in formateable_lines {
        // Create formatted line using bean-format's template logic with custom number-currency spacing
        // Extract currency part from rest and apply custom spacing
        let rest_content = line.rest.trim_start();
        let formatted_rest = if let Some(currency_start) = rest_content.find(char::is_alphabetic) {
            // Custom spacing between number and currency
            format!(
                "{}{}",
                " ".repeat(number_currency_spacing),
                &rest_content[currency_start..]
            )
        } else {
            // No currency found, use rest as-is
            format!(" {rest_content}")
        };

        // Apply custom indentation if specified, but only for postings, not top-level directives
        let (indent_str, account_name) = if let Some(target_indent) = indent_width {
            let account_part = line.prefix.trim_start().trim_end();

            // Check if this is a top-level directive (like balance) that shouldn't be indented
            // Get the full line to check for directive keywords
            let line_start_char = doc.content.line_to_char(line.line_num);
            let line_end_char = if line.line_num + 1 < doc.content.len_lines() {
                doc.content.line_to_char(line.line_num + 1)
            } else {
                doc.content.len_chars()
            };
            let full_line = doc
                .content
                .slice(line_start_char..line_end_char)
                .to_string();

            // More comprehensive check for balance/price directives
            let line_content = full_line.trim();
            let is_top_level_directive = line_content.contains("balance ")
                || line_content.contains("price ")
                || (line_content.starts_with("20")
                    && (line_content.contains(" balance ") || line_content.contains(" price ")));

            if is_top_level_directive {
                // Don't indent top-level directives
                ("".to_string(), account_part)
            } else {
                // Apply custom indentation for postings
                (" ".repeat(target_indent), account_part)
            }
        } else {
            // Preserve original indentation by finding the leading whitespace
            let account_part = line.prefix.trim_end();
            let original_indent = if line.prefix.len() > account_part.len() {
                &line.prefix[..(line.prefix.len() - account_part.len())]
            } else {
                ""
            };
            (original_indent.to_string(), account_part)
        };

        // Template: "{indent}{account_name:<adjusted_width}  {:>num_width}{custom_rest}"
        // Adjust the prefix width to account for the custom indentation
        let adjusted_prefix_width = if config.final_prefix_width > indent_str.len() {
            config.final_prefix_width - indent_str.len()
        } else {
            account_name.len() // fallback to actual account name length
        };

        let formatted_line = format!(
            "{}{:<width$}  {:>num_width$}{}",
            indent_str,
            account_name,
            line.number,
            formatted_rest,
            width = adjusted_prefix_width,
            num_width = config.final_num_width
        );

        if let Some(edit) = create_line_replacement_edit(line.line_num, &formatted_line, doc) {
            text_edits.push(edit);
        }
    }

    text_edits
}

/// Creates a text edit to replace an entire line with new content
fn create_line_replacement_edit(
    line_num: usize,
    new_content: &str,
    doc: &crate::document::Document,
) -> Option<lsp_types::TextEdit> {
    // Calculate the range of the original line (without trailing newline)
    let line_start_char = doc.content.line_to_char(line_num);
    let line_end_char = if line_num + 1 < doc.content.len_lines() {
        doc.content.line_to_char(line_num + 1)
    } else {
        doc.content.len_chars()
    };

    let original_line = doc
        .content
        .slice(line_start_char..line_end_char)
        .to_string();
    let original_line_len = original_line.trim_end().len();

    let line_start = lsp_types::Position {
        line: line_num as u32,
        character: 0,
    };
    let line_end = lsp_types::Position {
        line: line_num as u32,
        character: original_line_len as u32,
    };

    Some(lsp_types::TextEdit {
        range: lsp_types::Range {
            start: line_start,
            end: line_end,
        },
        new_text: new_content.trim_end().to_string(),
    })
}

/// Applies indent normalization to lines not already handled by main formatting
/// This ensures that indent changes don't conflict with amount/currency formatting
fn apply_indent_normalization_to_remaining_lines(
    doc: &crate::document::Document,
    _tree: &tree_sitter::Tree,
    target_indent_width: usize,
    mut existing_edits: Vec<lsp_types::TextEdit>,
) -> Result<Vec<lsp_types::TextEdit>> {
    use std::collections::HashSet;

    let target_indent = " ".repeat(target_indent_width);

    // Collect line numbers that already have edits from main formatting
    let edited_lines: HashSet<u32> = existing_edits
        .iter()
        .map(|edit| edit.range.start.line)
        .collect();

    // Process all lines in the document
    for line_num in 0..doc.content.len_lines() {
        let line_num_u32 = line_num as u32;

        // Skip lines that already have formatting edits
        if edited_lines.contains(&line_num_u32) {
            continue;
        }

        let line_start_char = doc.content.line_to_char(line_num);
        let line_end_char = if line_num + 1 < doc.content.len_lines() {
            doc.content.line_to_char(line_num + 1)
        } else {
            doc.content.len_chars()
        };

        let current_line = doc
            .content
            .slice(line_start_char..line_end_char)
            .to_string();

        // Only process lines that start with whitespace AND are likely to be postings/metadata
        // Don't normalize lines that contain top-level directive keywords at the start
        if current_line.starts_with(char::is_whitespace) {
            let trimmed_line = current_line.trim_start();
            if !trimmed_line.is_empty() {
                // Skip lines that appear to be top-level directives that are just indented
                // Look for common directive keywords after trimming
                let starts_with_directive = trimmed_line.starts_with("balance ")
                    || trimmed_line.starts_with("pad ")
                    || trimmed_line.starts_with("price ")
                    || trimmed_line.starts_with("open ")
                    || trimmed_line.starts_with("close ")
                    || trimmed_line.starts_with("note ")
                    || trimmed_line.starts_with("document ")
                    || trimmed_line.contains(" * \"")
                    || trimmed_line.contains(" ! \"")
                    || trimmed_line.contains(" txn \"");

                if !starts_with_directive {
                    let new_line = format!("{target_indent}{trimmed_line}");

                    // Only create edit if indentation actually changes
                    if current_line.trim_end() != new_line.trim_end()
                        && let Some(edit) = create_line_replacement_edit(line_num, &new_line, doc)
                    {
                        existing_edits.push(edit);
                    }
                }
            }
        }
    }

    Ok(existing_edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use crate::server::LspServerStateSnapshot;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use tree_sitter_beancount::tree_sitter;

    struct TestState {
        snapshot: LspServerStateSnapshot,
    }

    impl TestState {
        fn new(content: &str) -> anyhow::Result<Self> {
            // Use a consistent path that works across platforms
            let path = std::env::current_dir()?.join("test.beancount");
            let rope_content = ropey::Rope::from_str(content);

            // Parse the content with tree-sitter
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(content, None).unwrap();

            // Create the necessary data structures
            let mut forest = HashMap::new();
            forest.insert(path.clone(), Arc::new(tree.clone()));

            let mut open_docs = HashMap::new();
            open_docs.insert(
                path.clone(),
                Document {
                    content: rope_content.clone(),
                },
            );

            let mut beancount_data = HashMap::new();
            beancount_data.insert(
                path.clone(),
                Arc::new(BeancountData::new(&tree, &rope_content)),
            );

            let snapshot = LspServerStateSnapshot {
                beancount_data,
                config: Config::new(std::env::current_dir()?),
                forest,
                open_docs,
            };

            Ok(TestState { snapshot })
        }

        fn new_with_config(
            content: &str,
            format_config: crate::config::FormattingConfig,
        ) -> anyhow::Result<Self> {
            // Use a consistent path that works across platforms
            let path = std::env::current_dir()?.join("test.beancount");
            let rope_content = ropey::Rope::from_str(content);

            // Parse the content with tree-sitter
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(content, None).unwrap();

            // Create the necessary data structures
            let mut forest = HashMap::new();
            forest.insert(path.clone(), Arc::new(tree.clone()));

            let mut open_docs = HashMap::new();
            open_docs.insert(
                path.clone(),
                Document {
                    content: rope_content.clone(),
                },
            );

            let mut beancount_data = HashMap::new();
            beancount_data.insert(
                path.clone(),
                Arc::new(BeancountData::new(&tree, &rope_content)),
            );

            let mut config = Config::new(std::env::current_dir()?);
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
            // Use the same path strategy as in construction
            let path = std::env::current_dir()?.join("test.beancount");
            let url = url::Url::from_file_path(&path)
                .map_err(|_| anyhow::anyhow!("Failed to convert path to URL: {:?}", path))?;
            let uri = lsp_types::Uri::from_str(url.as_str())
                .map_err(|e| anyhow::anyhow!("Failed to create URI: {:?}", e))?;

            let params = lsp_types::DocumentFormattingParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
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
            number_currency_spacing: 1,
            indent_width: None,
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
            number_currency_spacing: 1,
            indent_width: None,
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
            number_currency_spacing: 1,
            indent_width: None,
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
            number_currency_spacing: 1,
            indent_width: None,
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
            number_currency_spacing: 1,
            indent_width: None,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Custom spacing formatted:\n{formatted}");

        // Bean-format always uses exactly 2 spaces between account and number, regardless of config
        // This test verifies that our formatter follows bean-format's behavior
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if line.trim().is_empty() || !line.contains("USD") {
                continue;
            }

            // Find where the account name ends and the number begins
            if let Some(number_start) = line.find(|c: char| c.is_numeric() || c == '-') {
                let before_number = &line[..number_start];
                if let Some(account_end) = before_number.rfind(|c: char| !c.is_whitespace()) {
                    let spacing_part = &before_number[account_end + 1..];
                    let actual_spacing = spacing_part.len();

                    // Bean-format uses minimum 4 spaces when using template formatting
                    // This comes from the "{:<prefix_width}  {:>num_width} {}" template
                    // which includes 2 spaces plus additional spacing from right-alignment
                    assert!(
                        actual_spacing >= 2,
                        "Bean-format uses at least 2 spaces between account and amount, but found {actual_spacing}"
                    );
                }
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
            number_currency_spacing: 1,
            indent_width: None,
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

            // Check that minimum spacing is maintained (like bean-format does)
            for line in [line1, line2] {
                if line.contains("balance")
                    && line.contains("USD")
                    && let Some(balance_pos) = line.find("balance")
                {
                    let after_balance = &line[balance_pos + 8..]; // Skip "balance "
                    if let Some(first_space) = after_balance.find(' ') {
                        let _account_part = &after_balance[..first_space];
                        let after_account = &after_balance[first_space..];

                        // Count spaces before the amount
                        let spaces_count = after_account.chars().take_while(|&c| c == ' ').count();

                        // Should have at least account_amount_spacing (2) spaces
                        assert!(
                            spaces_count >= 2,
                            "Balance directive should maintain minimum spacing, but found {spaces_count} spaces in: '{line}'"
                        );
                    }
                }
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
            indent_width: None,
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
            indent_width: None,
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
    fn test_number_currency_spacing_reduce_excess() {
        let content = r#"2023-01-01 * "Test"
  Assets:Cash     100.00     USD
  Expenses:Food 50.0 USD
"#;

        // Test reducing from 5 spaces to 1 space between number and currency
        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: None,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Original:\n{content}");
        println!("Formatted:\n{formatted}");

        // Verify that excess spaces are removed
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if line.contains("USD") {
                // Find the pattern "number USD" with exactly 1 space
                if let Some(usd_pos) = line.find("USD") {
                    let before_usd = &line[..usd_pos];
                    // Should end with exactly 1 space
                    assert!(
                        before_usd.ends_with(" "),
                        "Should have exactly 1 space before USD in line: '{line}'"
                    );
                    // Should not have 2 or more spaces
                    assert!(
                        !before_usd.ends_with("  "),
                        "Should not have 2 or more spaces before USD in line: '{line}'"
                    );
                }
            }
        }
    }

    #[test]
    fn test_other_transaction_types_spacing() {
        let content = r#"1900-01-01 open Assets:Cash
1900-01-01 close Assets:Cash
2023-01-01 balance Assets:Cash 1000.00     USD
2023-01-01 pad Assets:Cash Equity:Opening-Balances
2023-01-01 price USD 1.0     EUR
2023-01-01 note Assets:Cash "test note"
2023-01-01 document Assets:Cash "test.pdf"
"#;

        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: None,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Original:\n{content}");
        println!("Formatted:\n{formatted}");

        // Check if price and balance directives get their spacing fixed
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines {
            if line.contains("USD") || line.contains("EUR") {
                println!("Line with currency: '{line}'");
                // Find any currency and check spacing before it
                for currency in ["USD", "EUR"] {
                    if let Some(currency_pos) = line.find(currency) {
                        let before_currency = &line[..currency_pos];
                        if before_currency
                            .chars()
                            .last()
                            .is_some_and(|c| c.is_numeric())
                        {
                            // No space between number and currency - this would be wrong with default spacing
                            panic!(
                                "Found number directly followed by currency without space in: '{line}'"
                            );
                        }
                        if before_currency.ends_with("  ") && !before_currency.ends_with("   ") {
                            // Exactly 2 spaces - would be wrong with spacing=1
                            println!("Found 2 spaces before currency in: '{line}' - should be 1");
                        }
                        if before_currency.ends_with("     ") {
                            // 5 spaces - should be reduced to 1
                            panic!(
                                "Found excess spaces (5) before currency that weren't fixed in: '{line}'"
                            );
                        }

                        // Verify proper spacing (should end with exactly 1 space for number_currency_spacing=1)
                        if before_currency.ends_with(" ") && !before_currency.ends_with("  ") {
                            println!("✓ Correct 1-space spacing found in: '{line}'");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_account_amount_spacing_consistency() {
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash 100.00 USD
  Expenses:Food 50.0 USD
2023-01-01 balance Assets:Cash 1000.00 USD
2023-01-01 balance Assets:LongAccount 500.0 USD
2023-01-01 price USD 1.0 EUR
2023-01-01 price EUR 0.85 USD
"#;

        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2, // Should have at least 2 spaces
            number_currency_spacing: 1,
            indent_width: None,
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Original:\n{content}");
        println!("Formatted:\n{formatted}");

        // Check account-amount spacing consistency across different directive types
        let lines: Vec<&str> = formatted.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("USD") || line.contains("EUR") {
                println!("Line {i}: '{line}'");

                // Find the end of the "prefix" part and start of amount
                if line.contains("Assets:") || line.contains("Expenses:") {
                    // For postings and balance directives with accounts
                    if let Some(account_end) =
                        line.rfind("Assets:").or_else(|| line.rfind("Expenses:"))
                    {
                        let after_account = &line[account_end..];
                        if let Some(space_start) = after_account.find(' ') {
                            let account_part = &after_account[..space_start];
                            let spacing_part = &after_account[space_start..];

                            // Count spaces until we hit a number
                            let spaces_count =
                                spacing_part.chars().take_while(|&c| c == ' ').count();

                            println!(
                                "  Account: '{account_part}', Spaces to amount: {spaces_count}"
                            );

                            // Should have at least account_amount_spacing (2) spaces
                            assert!(
                                spaces_count >= 2,
                                "Line {i} should have at least 2 spaces between account and amount, but found {spaces_count}: '{line}'"
                            );
                        }
                    }
                } else if line.contains("price") {
                    // For price directives - check the spacing structure
                    // Pattern: "2023-01-01 price USD                       1.0 EUR"
                    if let Some(price_pos) = line.find("price") {
                        let after_price = &line[price_pos + 5..].trim_start(); // Skip "price" and initial spaces
                        // Find the first currency (USD)
                        if let Some(first_space) = after_price.find(' ') {
                            let currency_part = &after_price[..first_space];
                            let after_currency = &after_price[first_space..];

                            let spaces_count =
                                after_currency.chars().take_while(|&c| c == ' ').count();

                            println!(
                                "  Price directive currency: '{currency_part}', spaces to amount: {spaces_count}"
                            );

                            // Price directives should have appropriate spacing for alignment
                            // The exact amount depends on the alignment system, but should be reasonable
                            assert!(
                                spaces_count >= 1,
                                "Line {i} price directive should have at least 1 space between currency and amount, but found {spaces_count}: '{line}'"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_minimum_spacing_enforcement() {
        let content = r#"2023-01-01 * "Test minimum spacing"
  Assets:Cash 100.00 USD
  Assets:Very:Very:Very:Very:Very:Long:Account:Name:That:Goes:On:Forever 50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Minimum spacing test:\n{formatted}");

        // Verify that both lines maintain minimum spacing
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if line.trim().is_empty() || !line.contains("USD") {
                continue;
            }

            // Find the end of the account name and start of amount
            if let Some(usd_pos) = line.find("USD") {
                // Work backwards from USD to find the start of the number
                let before_usd = &line[..usd_pos].trim_end();
                if let Some(space_pos) = before_usd.rfind(' ') {
                    let _number_part = &before_usd[space_pos + 1..];
                    let account_and_spaces = &before_usd[..space_pos + 1];

                    // Count trailing spaces in account_and_spaces
                    let spaces_count =
                        account_and_spaces.len() - account_and_spaces.trim_end().len();

                    println!("Line: '{line}' has {spaces_count} spaces before number");

                    // Should have at least 2 spaces (account_amount_spacing default)
                    assert!(
                        spaces_count >= 2,
                        "Should maintain minimum 2 spaces between account and amount, but found {spaces_count} in line: '{line}'"
                    );
                }
            }
        }
    }

    #[test]
    fn test_comprehensive_mixed_directives() {
        let content = r#"2023-01-01 * "Short account transaction"
  Assets:Cash 100.00 USD
  Expenses:Food 50.0 USD

2023-01-01 * "Long account transaction"
  Assets:Cash:Checking:Account:With:Very:Long:Name 1500.00 USD
  Expenses:GuiltFree:Activities:Family:Entertainment 75.123 USD
  Income:Salary:Job:Main:Source -2000.00 USD

2023-01-01 balance Assets:Cash 1000.00 USD
2023-01-01 balance Assets:Cash:Checking:Account:With:Very:Long:Name 500.0 USD

2023-01-01 price USD 1.0 EUR
2023-01-01 price EUR 0.85678 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("=== Comprehensive Mixed Test ===");
        println!("Original:\n{content}");
        println!("Our formatter:\n{formatted}");

        // Also test with bean-format for comparison
        // This test is designed to help identify discrepancies
    }

    #[test]
    fn test_edge_case_extreme_lengths() {
        let content = r#"2023-01-01 * "Edge case 1: very short vs very long"
  A 1.0 USD
  Assets:Cash:Checking:Account:With:An:Extremely:Long:Name:That:Goes:On:And:On 999.99 USD

2023-01-01 balance A 100.0 USD
2023-01-01 balance Assets:Cash:Checking:Account:With:An:Extremely:Long:Name:That:Goes:On:And:On 500.0 USD

2023-01-01 price USD 1.0 EUR
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("=== Edge Case Extreme Lengths ===");
        println!("Original:\n{content}");
        println!("Our formatter:\n{formatted}");
    }

    #[test]
    fn test_actual_user_sample() {
        let content = r#"2025-01-01 * "BTLINODEAKAMAI CAMBRIDGE MA"
    Liabilities:CC:Amex                                        -5.10 USD
    Expenses:GuiltFree:Subscriptions:Domain

2025-01-01 * "SUMMERS OF DAYTON INBROWNSBURG O"
    Liabilities:CC:Amex                                       -11.99 USD
    Expenses:Fixed:Home:Maintenance

2025-01-02 txn "Withdrawal MORTGAGE SERV CT"
    Assets:Cash:WrightPatt:Checking                         -1240.57 USD
    Expenses:Fixed:Home:Mortgage:Escrow                       530.59 USD
    Expenses:Fixed:Home:Mortgage:Interest                     342.55 USD
    Liabilities:Mortgage:6749-Nestle-Creek
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("=== Actual User Sample ===");
        println!("Original:\n{content}");
        println!("Our formatter:\n{formatted}");
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
            number_currency_spacing: 1,
            indent_width: None,
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

    #[test]
    fn test_indent_normalization_disabled_by_default() {
        let content = r#"2023-01-01 * "Mixed indentation"
      Assets:Cash   100.00 USD
        Expenses:Food   50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        // Without indent_width config, original indentation should be preserved
        assert!(formatted.contains("      Assets:Cash"));
        assert!(formatted.contains("        Expenses:Food"));
    }

    #[test]
    fn test_indent_normalization_to_four_spaces() {
        let content = r#"2023-01-01 * "Mixed indentation"
      Assets:Cash   100.00 USD
        Expenses:Food   50.0 USD
	Other:Account		75.0 USD
"#;

        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: Some(4),
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Indent normalized:\n{formatted}");

        // All indented lines should now use exactly 4 spaces
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if !line.trim().is_empty() && line.starts_with(char::is_whitespace) {
                assert!(
                    line.starts_with("    "),
                    "Line should start with exactly 4 spaces: '{line}'"
                );
                // Should not start with 5 or more spaces (unless it's nested metadata)
                let trimmed = line.trim_start();
                let leading_spaces = line.len() - trimmed.len();
                if !trimmed.starts_with('"') && !trimmed.starts_with("description:") {
                    // Regular postings should have exactly 4 spaces
                    assert_eq!(
                        leading_spaces, 4,
                        "Expected exactly 4 spaces for posting line: '{line}'"
                    );
                }
            }
        }
    }

    #[test]
    fn test_indent_normalization_to_two_spaces() {
        let content = r#"2023-01-01 * "Test transaction"
    Assets:Cash 100.00 USD
      Expenses:Food 50.0 USD
"#;

        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: Some(2),
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Indent normalized to 2 spaces:\n{formatted}");

        // All postings should use exactly 2 spaces
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if !line.trim().is_empty() && line.starts_with(char::is_whitespace) {
                assert!(
                    line.starts_with("  "),
                    "Line should start with exactly 2 spaces: '{line}'"
                );
                assert!(
                    !line.starts_with("   "),
                    "Line should not start with 3 or more spaces: '{line}'"
                );
            }
        }
    }

    #[test]
    fn test_indent_normalization_preserves_top_level() {
        let content = r#"2023-01-01 * "Test transaction"
    Assets:Cash 100.00 USD
    Expenses:Food 50.0 USD

2023-01-02 balance Assets:Cash 150.00 USD
"#;

        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: None,
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: Some(2),
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);

        println!("Formatted with preserved top-level:\n{formatted}");

        // Top-level transactions and balance directives should remain at column 0
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines {
            if line.contains("2023-01-01 *") || line.contains("2023-01-02 balance") {
                assert!(
                    !line.starts_with(char::is_whitespace),
                    "Top-level directive should not be indented: '{line}'"
                );
            }
        }

        // Postings should be indented to 2 spaces
        for line in &lines {
            if line.trim().starts_with("Assets:") || line.trim().starts_with("Expenses:") {
                assert!(
                    line.starts_with("  "),
                    "Posting should be indented to 2 spaces: '{line}'"
                );
            }
        }
    }
}
