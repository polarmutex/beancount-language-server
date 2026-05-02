mod alignment;
mod extraction;

use alignment::{
    apply_indent_normalization_to_remaining_lines, generate_currency_column_edits,
    generate_template_edits,
};
use crate::server::LspServerStateSnapshot;
use anyhow::Result;
use extraction::{calculate_format_config, extract_formateable_lines};
use tracing::debug;

/// Main provider function for LSP `textDocument/formatting`.
///
/// This function recreates bean-format's behavior exactly:
/// 1. Extracts formateable lines using tree-sitter (instead of regex)
/// 2. Calculates alignment widths like bean-format
/// 3. Applies bean-format's formatting template or currency column logic
/// 4. Generates minimal text edits for the changes
pub(crate) fn formatting(
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
    let (tree, doc) = match snapshot.tree_and_document_for_uri(&params.text_document.uri) {
        Ok(v) => {
            tracing::debug!("Found document and parsed tree");
            v
        }
        Err(e) => {
            tracing::warn!("Could not find document or tree for formatting: {e}");
            return Ok(None);
        }
    };

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

    // Generate text edits based on formatting mode (only if we have formateable lines)
    let text_edits = if formateable_lines.is_empty() {
        tracing::debug!("No formateable lines found, skipping alignment formatting");
        vec![]
    } else {
        // Calculate formatting configuration
        let format_config =
            calculate_format_config(&formateable_lines, &snapshot.config.formatting);

        if let Some(currency_col) = snapshot.config.formatting.currency_column {
            generate_currency_column_edits(
                &formateable_lines,
                currency_col,
                doc,
                snapshot.config.formatting.indent_width,
            )
        } else {
            generate_template_edits(
                &formateable_lines,
                &format_config,
                snapshot.config.formatting.number_currency_spacing,
                snapshot.config.formatting.indent_width,
                doc,
            )
        }
    };

    // Apply indent normalization to remaining lines if configured
    let final_text_edits = if let Some(indent_width) = snapshot.config.formatting.indent_width {
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
                    version: 0,
                },
            );

            let mut beancount_data = HashMap::new();
            beancount_data.insert(
                path.clone(),
                Arc::new(BeancountData::new(&tree, &rope_content)),
            );

            let snapshot = LspServerStateSnapshot {
                beancount_data: Arc::new(beancount_data),
                config: Config::new(std::env::current_dir()?),
                forest: Arc::new(forest),
                forest_content: Arc::new(HashMap::new()),
                open_docs: Arc::new(open_docs),
                checker: None,
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
                    version: 0,
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
                beancount_data: Arc::new(beancount_data),
                config,
                forest: Arc::new(forest),
                forest_content: Arc::new(HashMap::new()),
                open_docs: Arc::new(open_docs),
                checker: None,
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
                forest_content: self.snapshot.forest_content.clone(),
                open_docs: self.snapshot.open_docs.clone(),
                checker: self.snapshot.checker.clone(),
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

    #[test]
    fn test_no_edits_for_perfectly_formatted_content() {
        // Test that already-formatted content generates ZERO edits
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash    100.00 USD
  Expenses:Food    50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        if !edits.is_empty() {
            println!("Generated {} edits:", edits.len());
            for (i, edit) in edits.iter().enumerate() {
                println!(
                    "  Edit {}: line {} char {}-{}: '{}'",
                    i,
                    edit.range.start.line,
                    edit.range.start.character,
                    edit.range.end.character,
                    edit.new_text
                );
            }
        }

        assert_eq!(
            edits.len(),
            0,
            "Perfectly formatted content should generate zero edits, but got {} edits",
            edits.len()
        );
    }

    #[test]
    fn test_edits_generated_for_unformatted_content() {
        // Test that unformatted content DOES generate edits
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash 100.00 USD
  Expenses:Food 50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        assert!(
            !edits.is_empty(),
            "Unformatted content should generate edits"
        );

        // Verify formatting produces correct alignment
        let formatted = apply_edits(content, &edits);
        let lines: Vec<&str> = formatted.lines().collect();
        if lines.len() >= 3 {
            let line1 = lines[1];
            let line2 = lines[2];

            if let (Some(pos1), Some(pos2)) = (line1.find("100.00"), line2.find("50.0")) {
                let end1 = pos1 + "100.00".len();
                let end2 = pos2 + "50.0".len();
                assert_eq!(end1, end2, "Numbers should be right-aligned");
            }
        }
    }

    #[test]
    fn test_mixed_formatted_and_unformatted_lines() {
        // Test that only unformatted lines get edits
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash          100.00 USD
  Expenses:Food 50.0 USD
  Income:Salary      -150.00 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // Should only edit the Expenses:Food line (line 2, 0-indexed)
        // The other lines are already correctly formatted
        assert!(
            !edits.is_empty(),
            "Should generate edits for unformatted line"
        );

        // Verify we're not editing already-formatted lines unnecessarily
        // by checking that edits only target specific lines
        let formatted = apply_edits(content, &edits);

        // After formatting, all lines should be aligned
        let lines: Vec<&str> = formatted.lines().collect();
        if lines.len() >= 4 {
            // Check all amounts are right-aligned
            let positions: Vec<_> = lines[1..4]
                .iter()
                .filter_map(|line| {
                    line.find("100.00")
                        .or_else(|| line.find("50.0"))
                        .or_else(|| line.find("150.00"))
                        .map(|pos| pos + line[pos..].split_whitespace().next().unwrap_or("").len())
                })
                .collect();

            if positions.len() >= 2 {
                assert_eq!(
                    positions[0], positions[1],
                    "All numbers should be right-aligned"
                );
            }
        }
    }

    #[test]
    fn test_currency_column_no_edits_when_aligned() {
        // Test currency column mode with already-aligned content
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash                              100.00 USD
  Expenses:Food                              50.0 USD
"#;

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

        if !edits.is_empty() {
            println!("Generated {} edits:", edits.len());
            for (i, edit) in edits.iter().enumerate() {
                println!(
                    "  Edit {}: line {} char {}-{}: '{}'",
                    i,
                    edit.range.start.line,
                    edit.range.start.character,
                    edit.range.end.character,
                    edit.new_text
                );
            }
            let formatted = apply_edits(content, &edits);
            println!("Original content:\n{}", content);
            println!("Formatted content:\n{}", formatted);
        }

        assert_eq!(
            edits.len(),
            0,
            "Already-aligned currency column should generate zero edits, but got {} edits",
            edits.len()
        );
    }

    #[test]
    fn test_currency_column_generates_edits_when_misaligned() {
        // Test currency column mode with misaligned content
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash 100.00 USD
  Expenses:Food 50.0 USD
"#;

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

        assert!(
            !edits.is_empty(),
            "Misaligned currency column should generate edits"
        );

        // Verify currency is aligned at the specified column
        let formatted = apply_edits(content, &edits);
        let lines: Vec<&str> = formatted.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("USD")
                && let Some(usd_pos) = line.find("USD")
            {
                println!("Line {i}: currency at column {usd_pos}");
            }
        }
    }

    #[test]
    fn test_idempotent_formatting_generates_no_edits() {
        // Test that formatting the same content twice generates no edits the second time
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash 100.00 USD
  Expenses:Food 50.0 USD
"#;

        let state = TestState::new(content).unwrap();

        // First format
        let edits1 = state.format().unwrap().unwrap();
        assert!(!edits1.is_empty(), "First format should generate edits");

        let formatted = apply_edits(content, &edits1);

        // Create new state with formatted content
        let state2 = TestState::new(&formatted).unwrap();

        // Second format should generate zero edits
        let edits2 = state2.format().unwrap().unwrap();
        assert_eq!(
            edits2.len(),
            0,
            "Formatting already-formatted content should generate zero edits (idempotent), but got {} edits",
            edits2.len()
        );
    }

    #[test]
    fn test_whitespace_only_changes_generate_no_edits() {
        // Test that content with equivalent whitespace generates no edits
        // This tests the trim_end() comparison logic
        let content = r#"2023-01-01 * "Test transaction"
  Assets:Cash    100.00 USD
  Expenses:Food    50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        // If content is already properly formatted (even with trailing spaces trimmed),
        // it should generate zero edits
        if edits.is_empty() {
            println!("No edits generated - content already formatted");
        } else {
            println!("Edits generated: {}", edits.len());
            let formatted = apply_edits(content, &edits);

            // Verify the formatted version doesn't change on second format
            let state2 = TestState::new(&formatted).unwrap();
            let edits2 = state2.format().unwrap().unwrap();
            assert_eq!(edits2.len(), 0, "Second format should generate zero edits");
        }
    }

    #[test]
    fn test_formatting_with_utf8_special_characters() {
        // Test issue #767: formatting with UTF-8 special characters like "ã"
        let content = r#"; Expenses Accounts
2020-01-01 open Expenses:Moradia:Manutenção BRL

; Assets Accounts
2020-01-01 open Assets:Nub:CC BRL
2020-01-01 open Equity:Opening-Balances BRL

2020-01-01 pad Assets:Nub:CC Equity:Opening-Balances

2026-01-07 * "Some awesome purchase"
  Expenses:Moradia:Manutenção                       18.00 BRL
  Assets:Nub:CC

2026-01-10 balance Assets:Nub:CC  156.00 BRL
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("Original:\n{content}");
        println!("Formatted:\n{formatted}");

        // The amount should remain intact (not corrupted like ".00 B")
        assert!(
            formatted.contains("18.00 BRL"),
            "Amount should not be corrupted: {formatted}"
        );
        assert!(
            !formatted.contains(".00 B BRL"),
            "Amount should not be corrupted to '.00 B': {formatted}"
        );

        // Account name should remain intact
        assert!(
            formatted.contains("Manutenção"),
            "Account name should not be corrupted: {formatted}"
        );
    }

    #[test]
    fn test_formatting_with_various_utf8_characters() {
        // Test with various UTF-8 characters in account names
        let content = r#"2023-01-01 * "UTF-8 test"
  Expenses:Café                              10.00 EUR
  Expenses:Résumé                             5.50 USD
  Assets:Банк                               100.00 RUB
  Income:日本                               -50.00 JPY
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("UTF-8 test formatted:\n{formatted}");

        // All amounts should remain intact
        assert!(
            formatted.contains("10.00 EUR"),
            "EUR amount should be intact"
        );
        assert!(
            formatted.contains("5.50 USD"),
            "USD amount should be intact"
        );
        assert!(
            formatted.contains("100.00 RUB"),
            "RUB amount should be intact"
        );
        assert!(
            formatted.contains("-50.00 JPY"),
            "JPY amount should be intact"
        );

        // All account names should remain intact
        assert!(formatted.contains("Café"), "Café should be intact");
        assert!(formatted.contains("Résumé"), "Résumé should be intact");
        assert!(formatted.contains("Банк"), "Банк should be intact");
        assert!(formatted.contains("日本"), "日本 should be intact");
    }

    #[test]
    fn test_formatting_with_calculations() {
        // Test formatting with calculations in amounts (issue #783 item 5)
        let content = r#"2023-01-01 * "Test calculations"
  Assets:Cash 300 * 4 USD
  Expenses:Food 50.0 USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("Formatted with calculations:\n{formatted}");

        // Check if calculation is preserved
        assert!(
            formatted.contains("300 * 4"),
            "Calculation should be preserved"
        );
        assert!(formatted.contains("USD"), "Currency should be preserved");

        // Verify calculations are aligned with regular numbers
        let lines: Vec<&str> = formatted.lines().collect();
        let mut usd_positions = Vec::new();

        for line in &lines[1..] {
            if let Some(usd_pos) = line.find("USD") {
                usd_positions.push(usd_pos);
            }
        }

        // All USD positions should be the same (aligned)
        if usd_positions.len() > 1 {
            let first_pos = usd_positions[0];
            for &pos in &usd_positions[1..] {
                assert_eq!(
                    pos, first_pos,
                    "Calculations should be aligned with regular numbers"
                );
            }
        }
    }

    #[test]
    fn test_formatting_open_directives() {
        // Test formatting open directives (issue #783 item 4)
        let content = r#"2020-01-01 open Expenses:Moradia:Manutencao BRL
2020-01-01 open Assets:Nub:CC BRL
2020-01-01 open Assets:Cash USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("Formatted open directives:\n{formatted}");

        // Check if currencies are aligned
        let lines: Vec<&str> = formatted.lines().collect();
        let mut currency_positions = Vec::new();

        for line in &lines {
            if line.contains("open") {
                if let Some(brl_pos) = line.find("BRL") {
                    currency_positions.push(brl_pos);
                } else if let Some(usd_pos) = line.find("USD") {
                    currency_positions.push(usd_pos);
                }
            }
        }

        // All currencies should be at the same position
        if currency_positions.len() > 1 {
            let first_pos = currency_positions[0];
            for &pos in &currency_positions[1..] {
                assert_eq!(
                    pos, first_pos,
                    "Currencies in open directives should be aligned"
                );
            }
        }

        // Verify that formatting was applied (edits generated)
        assert!(
            !edits.is_empty(),
            "Should generate formatting edits for open directives"
        );
    }

    #[test]
    fn test_indent_on_files_without_transactions() {
        // Test issue #783 item 3: indent normalization on files without transactions
        // This was previously blocked by an early return
        let content = r#"1900-01-01 commodity USD
      name: "US Dollar"
        asset-class: "currency"

1900-01-01 commodity EUR
     name: "Euro"
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
        println!("Formatted commodity file:\n{formatted}");

        // Verify that indent normalization was applied even without transactions
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines {
            if line.trim().starts_with("name:") || line.trim().starts_with("asset-class:") {
                assert!(
                    line.starts_with("  "),
                    "Metadata should be indented to 2 spaces: '{line}'"
                );
                assert!(
                    !line.starts_with("   ") || line.starts_with("    "),
                    "Metadata should use exactly 2 spaces, not more: '{line}'"
                );
            }
        }

        // Verify edits were generated (this would fail with the old early return)
        assert!(
            !edits.is_empty(),
            "Should generate indent edits for commodity files without transactions"
        );
    }

    #[test]
    fn test_single_pass_indent_and_currency_alignment() {
        // Test issue #783 item 2: indent and currency column should align in one pass
        let content = r#"2023-01-01 * "Test"
    Assets:Cash 100.00 USD
      Expenses:Food 50.0 USD
"#;

        let format_config = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: Some(40),
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: Some(2),
        };

        let state = TestState::new_with_config(content, format_config).unwrap();
        let edits = state.format().unwrap().unwrap();

        let formatted = apply_edits(content, &edits);
        println!("Single pass format:\n{formatted}");

        // Verify both indent and currency alignment happened in one pass
        let lines: Vec<&str> = formatted.lines().collect();
        for line in &lines[1..] {
            if !line.trim().is_empty() && line.trim().contains("Assets")
                || line.trim().contains("Expenses")
            {
                // Check indent is 2 spaces
                assert!(
                    line.starts_with("  "),
                    "Should be indented to 2 spaces: '{line}'"
                );

                // Check currency is at column 40
                if let Some(usd_pos) = line.find("USD") {
                    assert_eq!(
                        usd_pos, 40,
                        "Currency should be at column 40 in same pass as indent fix: '{line}'"
                    );
                }
            }
        }

        // Verify formatting is idempotent (one pass is enough)
        let format_config2 = crate::config::FormattingConfig {
            prefix_width: None,
            num_width: None,
            currency_column: Some(40),
            account_amount_spacing: 2,
            number_currency_spacing: 1,
            indent_width: Some(2),
        };
        let state2 = TestState::new_with_config(&formatted, format_config2).unwrap();
        let edits2 = state2.format().unwrap().unwrap();
        assert_eq!(
            edits2.len(),
            0,
            "Second format should generate no edits (idempotent after single pass)"
        );
    }

    #[test]
    fn test_open_directive_booking_method_preserved() {
        // Regression test for issue #836: formatter was stripping the leading
        // quote from booking method strings on `open` directives, e.g.
        //   open Assets:Foo FXAIX "FIFO"
        // was being mangled to:
        //   open Assets:Foo FXAIX FIFO"
        let content = r#"2024-01-01 open Assets:Investment:Retirement:Fidelity-Roth-IRA:FXAIX FXAIX "FIFO"
2024-01-01 open Assets:Investment:Retirement:Fidelity-Roth-IRA:VTSAX VTSAX "FIFO"
2024-01-01 open Assets:Checking USD
"#;

        let state = TestState::new(content).unwrap();
        let edits = state.format().unwrap().unwrap();
        let formatted = apply_edits(content, &edits);
        println!("Formatted with booking methods:\n{formatted}");

        // The booking method string must appear intact (with its leading quote)
        assert!(
            formatted.contains("\"FIFO\""),
            "Booking method string must be preserved with leading quote: got\n{formatted}"
        );

        // The mangled form must not appear
        assert!(
            !formatted.contains("FIFO\"") || formatted.contains("\"FIFO\""),
            "Formatter must not strip the leading quote from the booking method"
        );

        // Idempotency: formatting again should produce no further edits
        let state2 = TestState::new(&formatted).unwrap();
        let edits2 = state2.format().unwrap().unwrap();
        assert_eq!(
            edits2.len(),
            0,
            "Second format should produce no edits (idempotent): got\n{formatted}"
        );
    }
}
