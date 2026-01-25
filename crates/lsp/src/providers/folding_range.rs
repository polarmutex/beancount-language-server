use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use lsp_types::{FoldingRange, FoldingRangeKind, FoldingRangeParams};
use ropey::Rope;
use tree_sitter::Node;
use tree_sitter_beancount::tree_sitter;

/// Provider function for `textDocument/foldingRange`.
pub(crate) fn folding_ranges(
    snapshot: LspServerStateSnapshot,
    params: FoldingRangeParams,
) -> Result<Option<Vec<FoldingRange>>> {
    let uri = match params.text_document.uri.to_file_path() {
        Ok(path) => path,
        Err(_) => {
            tracing::debug!("Failed to convert URI to file path");
            return Ok(None);
        }
    };

    let forest = snapshot.forest;
    let tree = match forest.get(&uri) {
        Some(tree) => tree,
        None => {
            tracing::warn!("Tree not found in forest: {:?}", uri);
            return Ok(None);
        }
    };

    let content = match snapshot.open_docs.get(&uri) {
        Some(doc) => doc.content.clone(),
        None => {
            tracing::warn!("Document not found in open_docs: {:?}", uri);
            return Ok(None);
        }
    };

    let mut ranges = Vec::new();

    // Collect all root-level nodes
    let root_node = tree.root_node();
    let mut cursor = root_node.walk();
    let children: Vec<Node> = root_node.children(&mut cursor).collect();

    // Process transactions (fold multi-line transactions)
    for child in &children {
        if let Some(range) = fold_transaction(child, &content) {
            ranges.push(range);
        }
    }

    // Process comment blocks (consecutive comments)
    ranges.extend(fold_comment_blocks(&children));

    // Process directive groups (consecutive similar directives)
    ranges.extend(fold_directive_groups(&children));

    // Sort ranges by start line for better client handling
    ranges.sort_by_key(|r| r.start_line);

    tracing::trace!("Folding ranges: found {} ranges", ranges.len());
    Ok(Some(ranges))
}

/// Fold multi-line transactions.
/// A transaction is foldable if it has postings (multiline with content).
fn fold_transaction(node: &Node, content: &Rope) -> Option<FoldingRange> {
    if node.kind() != "transaction" {
        return None;
    }

    // Check if the transaction has any posting children
    let mut cursor = node.walk();
    let has_postings = node
        .children(&mut cursor)
        .any(|child| child.kind() == "posting");

    // Only fold if the transaction has postings (truly multi-line)
    if !has_postings {
        return None;
    }

    let start_line = node.start_position().row;
    let end_line = node.end_position().row;

    // Create collapsed text showing the first line (date and narration)
    let collapsed_text = extract_transaction_summary(node, content);

    Some(FoldingRange {
        start_line: start_line as u32,
        end_line: end_line as u32,
        kind: Some(FoldingRangeKind::Region),
        collapsed_text,
        start_character: None,
        end_character: None,
    })
}

/// Extract a summary of the transaction for collapsed view.
/// Format: "YYYY-MM-DD * \"Payee\" \"Narration\""
fn extract_transaction_summary(node: &Node, content: &Rope) -> Option<String> {
    let mut cursor = node.walk();
    let mut date = String::new();
    let mut flag = String::new();
    let mut payee = String::new();
    let mut narration = String::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "date" => {
                date = text_for_tree_sitter_node(content, &child);
            }
            "txn" => {
                // "txn" node contains the flag character (* or !)
                flag = text_for_tree_sitter_node(content, &child);
            }
            "payee" => {
                payee = text_for_tree_sitter_node(content, &child);
            }
            "narration" => {
                narration = text_for_tree_sitter_node(content, &child);
            }
            _ => {}
        }
    }

    // Build the summary
    let mut summary = date;
    if !flag.is_empty() {
        summary.push(' ');
        summary.push_str(&flag);
    }
    if !payee.is_empty() {
        summary.push(' ');
        summary.push_str(&payee);
    }
    if !narration.is_empty() {
        summary.push(' ');
        summary.push_str(&narration);
    }

    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

/// Fold consecutive comment lines into blocks.
/// Groups consecutive comment nodes together.
fn fold_comment_blocks(nodes: &[Node]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut block_start: Option<usize> = None;
    let mut block_end: usize = 0;

    for (i, node) in nodes.iter().enumerate() {
        if node.kind() == "comment" {
            let current_line = node.start_position().row;

            if let Some(start) = block_start {
                // Check if this comment is consecutive (within 1 line of the previous)
                if current_line <= block_end + 1 {
                    block_end = node.end_position().row;
                } else {
                    // Non-consecutive comment, finalize the previous block
                    if block_end > nodes[start].start_position().row {
                        ranges.push(FoldingRange {
                            start_line: nodes[start].start_position().row as u32,
                            end_line: block_end as u32,
                            kind: Some(FoldingRangeKind::Comment),
                            collapsed_text: None,
                            start_character: None,
                            end_character: None,
                        });
                    }
                    // Start a new block
                    block_start = Some(i);
                    block_end = node.end_position().row;
                }
            } else {
                // Start of a new comment block
                block_start = Some(i);
                block_end = node.end_position().row;
            }
        } else if block_start.is_some() {
            // Non-comment node encountered, finalize the comment block
            if let Some(start) = block_start
                && block_end > nodes[start].start_position().row
            {
                ranges.push(FoldingRange {
                    start_line: nodes[start].start_position().row as u32,
                    end_line: block_end as u32,
                    kind: Some(FoldingRangeKind::Comment),
                    collapsed_text: None,
                    start_character: None,
                    end_character: None,
                });
            }
            block_start = None;
        }
    }

    // Finalize the last comment block if any
    if let Some(start) = block_start
        && block_end > nodes[start].start_position().row
    {
        ranges.push(FoldingRange {
            start_line: nodes[start].start_position().row as u32,
            end_line: block_end as u32,
            kind: Some(FoldingRangeKind::Comment),
            collapsed_text: None,
            start_character: None,
            end_character: None,
        });
    }

    ranges
}

/// Fold groups of consecutive similar directives.
/// Groups consecutive open, close, option, plugin, include directives.
/// Only creates fold ranges if there are at least 2 consecutive directives.
fn fold_directive_groups(nodes: &[Node]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let foldable_directives = ["open", "close", "option", "plugin", "include"];

    let mut block_start: Option<(usize, String)> = None;
    let mut block_end: usize = 0;
    let mut block_count: usize = 0;

    for (i, node) in nodes.iter().enumerate() {
        let node_kind = node.kind();

        if foldable_directives.contains(&node_kind) {
            let current_line = node.start_position().row;

            if let Some((start, ref kind)) = block_start {
                // Check if this directive is the same kind and consecutive
                if node_kind == kind && current_line <= block_end + 1 {
                    block_end = node.end_position().row;
                    block_count += 1;
                } else {
                    // Different kind or non-consecutive, finalize the previous block
                    if block_count >= 2 {
                        ranges.push(FoldingRange {
                            start_line: nodes[start].start_position().row as u32,
                            end_line: block_end as u32,
                            kind: Some(FoldingRangeKind::Region),
                            collapsed_text: None,
                            start_character: None,
                            end_character: None,
                        });
                    }
                    // Start a new block
                    block_start = Some((i, node_kind.to_string()));
                    block_end = node.end_position().row;
                    block_count = 1;
                }
            } else {
                // Start of a new directive block
                block_start = Some((i, node_kind.to_string()));
                block_end = node.end_position().row;
                block_count = 1;
            }
        } else if block_start.is_some() {
            // Non-directive node encountered, finalize the directive block
            if block_count >= 2
                && let Some((start, _)) = block_start
            {
                ranges.push(FoldingRange {
                    start_line: nodes[start].start_position().row as u32,
                    end_line: block_end as u32,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                    start_character: None,
                    end_character: None,
                });
            }
            block_start = None;
            block_count = 0;
        }
    }

    // Finalize the last directive block if any
    if block_count >= 2
        && let Some((start, _)) = block_start
    {
        ranges.push(FoldingRange {
            start_line: nodes[start].start_position().row as u32,
            end_line: block_end as u32,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
            start_character: None,
            end_character: None,
        });
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tree_sitter_beancount::tree_sitter;

    /// Helper to parse beancount content and create a tree
    fn parse_beancount(content: &str) -> Arc<tree_sitter::Tree> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .expect("Failed to set language");
        let tree = parser.parse(content, None).expect("Failed to parse");
        Arc::new(tree)
    }

    #[test]
    fn test_fold_transaction_single_line() {
        let content = "2024-01-15 * \"Simple\" \"Transaction\"\n";
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let txn = &children[0];
        println!(
            "Single-line txn: start={}:{} end={}:{}",
            txn.start_position().row,
            txn.start_position().column,
            txn.end_position().row,
            txn.end_position().column
        );

        // Single line transaction should not be foldable
        let range = fold_transaction(&children[0], &rope);
        assert!(range.is_none(), "Single-line transaction should not fold");
    }

    #[test]
    fn test_fold_transaction_multi_line() {
        let content = r#"2024-01-15 * "Grocery Store" "Weekly shopping"
  Expenses:Food:Groceries    45.23 USD
  Assets:Bank:Checking      -45.23 USD
"#;
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let range = fold_transaction(&children[0], &rope);
        assert!(range.is_some(), "Multi-line transaction should fold");

        let range = range.unwrap();
        assert_eq!(range.start_line, 0);
        // Transaction ends at line 3 (after the last posting line with content on line 2)
        assert!(range.end_line >= 2, "End line should be >= 2");
        assert_eq!(range.kind, Some(FoldingRangeKind::Region));

        // Check collapsed text contains transaction summary
        let collapsed = range.collapsed_text.unwrap();
        assert!(collapsed.contains("2024-01-15"), "Should contain date");
        assert!(collapsed.contains("*"), "Should contain flag");
        assert!(collapsed.contains("Grocery Store"), "Should contain payee");
        assert!(
            collapsed.contains("Weekly shopping"),
            "Should contain narration"
        );
    }

    #[test]
    fn test_fold_transaction_with_flag_only() {
        let content = r#"2024-01-15 * "No payee"
  Assets:Bank:Checking    100.00 USD
  Income:Salary          -100.00 USD
"#;
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let range = fold_transaction(&children[0], &rope);
        assert!(range.is_some());

        let collapsed = range.unwrap().collapsed_text.unwrap();
        assert!(collapsed.contains("2024-01-15"));
        assert!(collapsed.contains("*"));
        assert!(collapsed.contains("No payee"));
    }

    #[test]
    fn test_fold_comment_blocks_single_comment() {
        let content = "; Single comment\n2024-01-01 open Assets:Checking\n";
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_comment_blocks(&children);
        // Single comment line should not create a fold range
        assert_eq!(ranges.len(), 0, "Single comment should not fold");
    }

    #[test]
    fn test_fold_comment_blocks_consecutive() {
        let content = r#"; Comment line 1
; Comment line 2
; Comment line 3
2024-01-01 open Assets:Checking
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_comment_blocks(&children);
        assert_eq!(
            ranges.len(),
            1,
            "Consecutive comments should create one fold"
        );

        let range = &ranges[0];
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 2);
        assert_eq!(range.kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn test_fold_comment_blocks_with_gap() {
        let content = r#"; Comment line 1
; Comment line 2

; Comment line 3 (after gap)
; Comment line 4
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_comment_blocks(&children);
        assert_eq!(ranges.len(), 2, "Comments with gap should create two folds");

        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 1);
        assert_eq!(ranges[1].start_line, 3);
        assert_eq!(ranges[1].end_line, 4);
    }

    #[test]
    fn test_fold_comment_blocks_multiline_block() {
        let content = r#";; ============================================
;; IMPORTANT NOTES ABOUT ACCOUNTING
;; ============================================
;; This section contains important information
;; about the accounting methodology used
;; ============================================
2024-01-01 open Assets:Checking
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_comment_blocks(&children);
        assert_eq!(
            ranges.len(),
            1,
            "Large comment block should create one fold"
        );

        let range = &ranges[0];
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 5);
        assert_eq!(range.kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn test_fold_directive_groups_single_directive() {
        let content = "2020-01-01 open Assets:Checking\n";
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_directive_groups(&children);
        assert_eq!(ranges.len(), 0, "Single directive should not fold");
    }

    #[test]
    fn test_fold_directive_groups_consecutive_open() {
        let content = r#"2020-01-01 open Assets:Bank:Checking
2020-01-01 open Assets:Bank:Savings
2020-01-01 open Assets:Bank:CreditCard
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_directive_groups(&children);
        assert_eq!(
            ranges.len(),
            1,
            "Consecutive open directives should create one fold"
        );

        let range = &ranges[0];
        assert_eq!(range.start_line, 0);
        assert!(range.end_line >= 2, "End line should be at least 2");
        assert_eq!(range.kind, Some(FoldingRangeKind::Region));
    }

    #[test]
    fn test_fold_directive_groups_mixed_types() {
        let content = r#"2020-01-01 open Assets:Checking
2020-01-01 open Assets:Savings
2020-01-01 close Expenses:Old
2020-01-02 open Assets:New
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_directive_groups(&children);
        // Should create 2 folds: first for 2 opens, then close doesn't fold (single), then last open doesn't fold (single)
        assert_eq!(
            ranges.len(),
            1,
            "Should create fold for first group of opens only"
        );

        let range = &ranges[0];
        assert_eq!(range.start_line, 0);
        assert!(range.end_line >= 1, "End line should be at least 1");
    }

    #[test]
    fn test_fold_directive_groups_options() {
        let content = r#"option "title" "My Ledger"
option "operating_currency" "USD"
option "inferred_tolerance_default" "*:0.01"
2024-01-01 open Assets:Checking
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_directive_groups(&children);
        assert_eq!(
            ranges.len(),
            1,
            "Consecutive options should create one fold"
        );

        let range = &ranges[0];
        assert_eq!(range.start_line, 0);
        assert!(range.end_line >= 2, "End line should be at least 2");
        assert_eq!(range.kind, Some(FoldingRangeKind::Region));
    }

    #[test]
    fn test_fold_directive_groups_with_gap() {
        let content = r#"2020-01-01 open Assets:Checking
2020-01-01 open Assets:Savings

2020-01-03 open Expenses:Food
2020-01-03 open Expenses:Transport
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let ranges = fold_directive_groups(&children);
        // Gap should separate into two groups
        assert_eq!(ranges.len(), 2, "Gap should create two separate folds");

        assert_eq!(ranges[0].start_line, 0);
        assert!(ranges[0].end_line >= 1, "First group end >= 1");
        assert!(ranges[1].start_line >= 3, "Second group start >= 3");
        assert!(ranges[1].end_line >= 4, "Second group end >= 4");
    }

    #[test]
    fn test_extract_transaction_summary_complete() {
        let content = r#"2024-01-15 * "Grocery Store" "Weekly shopping"
  Expenses:Food:Groceries    45.23 USD
"#;
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let summary = extract_transaction_summary(&children[0], &rope);
        assert!(summary.is_some());

        let summary = summary.unwrap();
        assert!(summary.contains("2024-01-15"));
        assert!(summary.contains("*"));
        assert!(summary.contains("Grocery Store"));
        assert!(summary.contains("Weekly shopping"));
    }

    #[test]
    fn test_extract_transaction_summary_no_narration() {
        let content = r#"2024-01-15 * "Payee only"
  Assets:Checking    100.00 USD
"#;
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let summary = extract_transaction_summary(&children[0], &rope);
        assert!(summary.is_some());

        let summary = summary.unwrap();
        assert!(summary.contains("2024-01-15"));
        assert!(summary.contains("*"));
        assert!(summary.contains("Payee only"));
    }

    #[test]
    fn test_extract_transaction_summary_flag_exclamation() {
        let content = r#"2024-01-15 ! "Pending" "Transaction"
  Assets:Checking    100.00 USD
"#;
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let summary = extract_transaction_summary(&children[0], &rope);
        assert!(summary.is_some());

        let summary = summary.unwrap();
        assert!(summary.contains("2024-01-15"));
        assert!(summary.contains("!"));
        assert!(summary.contains("Pending"));
    }

    #[test]
    fn test_non_transaction_node() {
        let content = "2020-01-01 open Assets:Checking\n";
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        // Try to fold an 'open' directive (not a transaction)
        let range = fold_transaction(&children[0], &rope);
        assert!(range.is_none(), "Non-transaction nodes should not fold");
    }

    #[test]
    fn test_empty_file() {
        let content = "";
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        assert_eq!(children.len(), 0);
        let comment_ranges = fold_comment_blocks(&children);
        let directive_ranges = fold_directive_groups(&children);

        assert_eq!(comment_ranges.len(), 0);
        assert_eq!(directive_ranges.len(), 0);
    }

    #[test]
    fn test_mixed_content_comprehensive() {
        let content = r#"; Configuration Options
option "title" "My Ledger"
option "operating_currency" "USD"

; Bank Accounts
2020-01-01 open Assets:Bank:Checking
2020-01-01 open Assets:Bank:Savings

; Transactions
2024-01-15 * "Grocery Store" "Weekly shopping"
  Expenses:Food:Groceries    45.23 USD
  Assets:Bank:Checking      -45.23 USD

2024-01-20 * "Gas Station"
  Expenses:Transport         50.00 USD
  Assets:Bank:CreditCard    -50.00 USD
"#;
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        // Test comment folding
        let comment_ranges = fold_comment_blocks(&children);
        // Note: Single comments don't fold, need at least 2 consecutive
        // This content has single comments, so expect 0 folds
        assert_eq!(comment_ranges.len(), 0, "Single comments should not fold");

        // Test directive folding
        let directive_ranges = fold_directive_groups(&children);
        assert_eq!(
            directive_ranges.len(),
            2,
            "Should find two directive groups (2 options, 2 opens)"
        );

        // Test transaction folding
        let rope = Rope::from_str(content);
        let mut tx_count = 0;
        for child in &children {
            if let Some(_range) = fold_transaction(child, &rope) {
                tx_count += 1;
            }
        }
        assert_eq!(tx_count, 2, "Should find 2 foldable transactions");
    }

    #[test]
    fn test_ranges_sorted_by_line() {
        let content = r#"2024-01-20 * "Transaction 2"
  Expenses:Transport    50.00 USD
  Assets:Bank          -50.00 USD

2024-01-15 * "Transaction 1"
  Expenses:Food        45.23 USD
  Assets:Bank         -45.23 USD
"#;
        let rope = Rope::from_str(content);
        let tree = parse_beancount(content);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();

        let mut ranges = Vec::new();
        for child in &children {
            if let Some(range) = fold_transaction(child, &rope) {
                ranges.push(range);
            }
        }

        // Sort ranges
        ranges.sort_by_key(|r| r.start_line);

        // Verify they're in order
        assert_eq!(ranges.len(), 2);
        assert!(ranges[0].start_line <= ranges[1].start_line);
    }
}
