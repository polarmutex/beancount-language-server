/// Inlay hints provider for beancount files
///
/// Provides inline hints for:
/// 1. Calculated balancing amounts - shows implicit amounts for postings without explicit amounts
/// 2. Transaction totals - displays total when transaction doesn't balance
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::{Context, Result};
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams, Position};
use std::collections::HashMap;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

/// Transaction query to find all transactions and their postings
const TRANSACTION_QUERY: &str = r#"
(transaction) @transaction
"#;

#[derive(Debug, Clone)]
struct Amount {
    value: rust_decimal::Decimal,
    currency: String,
}

#[derive(Debug, Clone)]
struct Price {
    amount: Amount,
    is_total: bool, // true for @@, false for @
}

#[derive(Debug, Clone)]
struct Cost {
    amount: Amount,
    is_total: bool, // true for {{total}}, false for {unit}
}

#[derive(Debug, Clone)]
struct PostingAmount {
    amount: Amount,
    price: Option<Price>,
    cost: Option<Cost>,
}

impl PostingAmount {
    /// Convert this posting amount to a specific currency if price or cost is specified
    fn convert_to_currency(&self) -> Option<(rust_decimal::Decimal, String)> {
        // Cost basis takes precedence over price for balance calculations
        if let Some(cost) = &self.cost {
            // Cost is specified, convert to the cost currency
            let converted_value = if cost.is_total {
                // {{total}} means total cost, use the cost amount directly
                cost.amount.value
            } else {
                // {unit} means unit cost, multiply by quantity
                self.amount.value * cost.amount.value
            };
            Some((converted_value, cost.amount.currency.clone()))
        } else if let Some(price) = &self.price {
            // Price is specified, convert to the price currency
            let converted_value = if price.is_total {
                // @@ means total cost, use the price amount directly
                price.amount.value
            } else {
                // @ means unit price, multiply by quantity
                self.amount.value * price.amount.value
            };
            Some((converted_value, price.amount.currency.clone()))
        } else {
            // No price or cost, return original
            None
        }
    }
}

impl Amount {
    fn parse(text: &str) -> Option<Self> {
        // Parse amount like "100.00 USD" or "100.00USD" or "100 USD"
        let text = text.trim();
        let parts: Vec<&str> = text.split_whitespace().collect();

        if parts.len() >= 2 {
            // Format: "100.00 USD"
            let value = rust_decimal::Decimal::from_str_exact(parts[0]).ok()?;
            let currency = parts[1].to_string();
            Some(Amount { value, currency })
        } else if parts.len() == 1 {
            // Try to split number and currency without space: "100.00USD"
            let text = parts[0];
            for (i, c) in text.char_indices() {
                if c.is_alphabetic() {
                    let (num_part, curr_part) = text.split_at(i);
                    let value = rust_decimal::Decimal::from_str_exact(num_part).ok()?;
                    let currency = curr_part.to_string();
                    return Some(Amount { value, currency });
                }
            }
            None
        } else {
            None
        }
    }
}

#[derive(Debug)]
struct Posting {
    node: tree_sitter::Node<'static>,
    amount: Option<PostingAmount>,
}

/// Main entry point for inlay hints
pub(crate) fn inlay_hints(
    snapshot: LspServerStateSnapshot,
    params: InlayHintParams,
) -> Result<Option<Vec<InlayHint>>> {
    let uri = &params.text_document.uri;
    let file_path = uri
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Invalid file path"))?;

    // Get the tree and document content
    let tree = snapshot
        .forest
        .get(&file_path)
        .context("File not found in forest")?;
    let doc = snapshot
        .open_docs
        .get(&file_path)
        .context("Document not found")?;
    let content = &doc.content;
    let content_str = content.to_string();
    let content_bytes = content_str.as_bytes();

    let mut hints = Vec::new();

    // Query for all transactions
    let transaction_query =
        tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY)
            .context("Failed to compile transaction query")?;

    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_byte_range(0..content_bytes.len());

    let mut matches = cursor.matches(&transaction_query, tree.root_node(), content_bytes);

    while let Some(qmatch) = matches.next() {
        for capture in qmatch.captures {
            let txn_node = capture.node;

            // Check if this transaction is in the requested range
            let txn_range = txn_node.range();
            let txn_start = Position::new(
                txn_range.start_point.row as u32,
                txn_range.start_point.column as u32,
            );
            let txn_end = Position::new(
                txn_range.end_point.row as u32,
                txn_range.end_point.column as u32,
            );

            // Skip if transaction is outside the requested range
            if txn_end < params.range.start || txn_start > params.range.end {
                continue;
            }

            // Process this transaction
            if let Some(txn_hints) = process_transaction(&txn_node, content) {
                hints.extend(txn_hints);
            }
        }
    }

    Ok(if hints.is_empty() { None } else { Some(hints) })
}

/// Calculate inlay hints for a single transaction node.
///
/// This is shared with other features (e.g. hover) that want to surface
/// the same information even when inline hints are disabled.
pub(crate) fn transaction_inlay_hints(
    txn_node: &tree_sitter::Node,
    content: &ropey::Rope,
) -> Option<Vec<InlayHint>> {
    process_transaction(txn_node, content)
}

/// Process a single transaction and return hints
fn process_transaction(
    txn_node: &tree_sitter::Node,
    content: &ropey::Rope,
) -> Option<Vec<InlayHint>> {
    let mut hints = Vec::new();

    // Find all postings in this transaction
    let postings = extract_postings(txn_node, content)?;

    // Check if there's a posting without an amount
    let has_missing_amount = postings.iter().any(|p| p.amount.is_none());

    if has_missing_amount {
        // If there's a missing amount, show the balancing amount at the end of that posting line
        if let Some(hint) = calculate_balancing_hint(&postings) {
            hints.push(hint);
        }
    } else {
        // If all postings have amounts, only show hint if transaction doesn't balance
        let txn_line_end_pos = get_transaction_line_end_position(txn_node);
        if let Some(hint) = calculate_total_hint(&postings, txn_line_end_pos) {
            hints.push(hint);
        }
    }

    Some(hints)
}

/// Get the position at the end of the transaction's first line
fn get_transaction_line_end_position(txn_node: &tree_sitter::Node) -> Position {
    // Find the end of the first line of the transaction (after narration/payee)
    let mut cursor = txn_node.walk();
    let mut last_col = txn_node.start_position().column;
    let txn_row = txn_node.start_position().row;

    for child in txn_node.children(&mut cursor) {
        // Only look at children on the first line of the transaction
        if child.start_position().row == txn_row {
            last_col = child.end_position().column;
        } else {
            // Once we hit a child on a different line, stop
            break;
        }
    }

    Position::new(txn_row as u32, last_col as u32)
}

/// Extract all postings from a transaction
fn extract_postings(txn_node: &tree_sitter::Node, content: &ropey::Rope) -> Option<Vec<Posting>> {
    let mut postings = Vec::new();
    let mut cursor = txn_node.walk();

    for child in txn_node.children(&mut cursor) {
        if child.kind() == "posting" {
            let amount = extract_amount(&child, content);
            // SAFETY: We're storing the node in a context where we know the tree outlives it
            // This is safe because we're processing synchronously and the tree is kept alive
            // by the snapshot throughout the entire inlay_hints call
            let static_node = unsafe {
                std::mem::transmute::<tree_sitter::Node<'_>, tree_sitter::Node<'static>>(child)
            };
            postings.push(Posting {
                node: static_node,
                amount,
            });
        }
    }

    if postings.is_empty() {
        None
    } else {
        Some(postings)
    }
}

/// Extract amount and optional price/cost from a posting node
fn extract_amount(
    posting_node: &tree_sitter::Node,
    content: &ropey::Rope,
) -> Option<PostingAmount> {
    let mut cursor = posting_node.walk();
    let mut amount_opt: Option<Amount> = None;
    let mut is_price_total = false;
    let mut price_amount_opt: Option<Amount> = None;
    let mut cost_opt: Option<Cost> = None;

    for child in posting_node.children(&mut cursor) {
        match child.kind() {
            "incomplete_amount" | "amount" => {
                if amount_opt.is_none() {
                    // First amount is the posting amount
                    let amount_text = text_for_tree_sitter_node(content, &child);
                    amount_opt = Amount::parse(&amount_text);
                }
            }
            "at" => {
                // @ means unit price
                is_price_total = false;
            }
            "atat" => {
                // @@ means total cost
                is_price_total = true;
            }
            "price_annotation" => {
                // Parse the price amount from price_annotation
                price_amount_opt = extract_price_annotation(&child, content);
            }
            "cost_spec" => {
                // Parse cost basis from cost_spec
                cost_opt = extract_cost_spec(&child, content);
            }
            _ => {}
        }
    }

    amount_opt.map(|amount| PostingAmount {
        amount,
        price: price_amount_opt.map(|price_amt| Price {
            amount: price_amt,
            is_total: is_price_total,
        }),
        cost: cost_opt,
    })
}

/// Extract price amount from a price_annotation node
fn extract_price_annotation(
    price_annotation_node: &tree_sitter::Node,
    content: &ropey::Rope,
) -> Option<Amount> {
    let mut cursor = price_annotation_node.walk();

    for child in price_annotation_node.children(&mut cursor) {
        if child.kind() == "incomplete_amount" || child.kind() == "amount" {
            let amount_text = text_for_tree_sitter_node(content, &child);
            return Amount::parse(&amount_text);
        }
    }

    None
}

/// Extract cost basis from a cost_spec node
fn extract_cost_spec(cost_spec_node: &tree_sitter::Node, content: &ropey::Rope) -> Option<Cost> {
    let mut cursor = cost_spec_node.walk();
    let mut is_total = false;
    let mut cost_amount: Option<Amount> = None;

    // Check for {{ or {
    for child in cost_spec_node.children(&mut cursor) {
        match child.kind() {
            "{{" => is_total = true,
            "{" => is_total = false,
            "cost_comp" => {
                // Extract the amount from cost_comp -> compound_amount
                cost_amount = extract_cost_comp(&child, content);
            }
            _ => {}
        }
    }

    cost_amount.map(|amount| Cost { amount, is_total })
}

/// Extract amount from a cost_comp node
fn extract_cost_comp(cost_comp_node: &tree_sitter::Node, content: &ropey::Rope) -> Option<Amount> {
    let mut cursor = cost_comp_node.walk();

    for child in cost_comp_node.children(&mut cursor) {
        if child.kind() == "compound_amount" {
            return extract_compound_amount(&child, content);
        }
    }

    None
}

/// Extract amount from a compound_amount node (has number and currency children)
fn extract_compound_amount(
    compound_amount_node: &tree_sitter::Node,
    content: &ropey::Rope,
) -> Option<Amount> {
    let mut cursor = compound_amount_node.walk();
    let mut number_str = String::new();
    let mut currency_str = String::new();

    for child in compound_amount_node.children(&mut cursor) {
        match child.kind() {
            "number" => {
                number_str = text_for_tree_sitter_node(content, &child);
            }
            "currency" => {
                currency_str = text_for_tree_sitter_node(content, &child);
            }
            _ => {}
        }
    }

    if !number_str.is_empty() && !currency_str.is_empty() {
        let value = rust_decimal::Decimal::from_str_exact(&number_str).ok()?;
        Some(Amount {
            value,
            currency: currency_str,
        })
    } else {
        None
    }
}

/// Calculate hint for balancing amounts (postings without explicit amounts)
fn calculate_balancing_hint(postings: &[Posting]) -> Option<InlayHint> {
    // Find posting without amount
    let posting_without_amount = postings.iter().find(|p| p.amount.is_none())?;

    // Calculate the sum of all other postings grouped by currency
    // If a posting has a price, convert it to the price currency
    let mut totals: HashMap<String, rust_decimal::Decimal> = HashMap::new();

    for posting in postings {
        if let Some(posting_amount) = &posting.amount {
            // Check if this posting has a price annotation
            if let Some((converted_value, converted_currency)) =
                posting_amount.convert_to_currency()
            {
                // Use the converted amount and currency
                *totals
                    .entry(converted_currency)
                    .or_insert(rust_decimal::Decimal::ZERO) += converted_value;
            } else {
                // Use the original amount and currency
                *totals
                    .entry(posting_amount.amount.currency.clone())
                    .or_insert(rust_decimal::Decimal::ZERO) += posting_amount.amount.value;
            }
        }
    }

    // The balancing amount is the negative of the total
    if totals.is_empty() {
        return None;
    }

    // Format the balancing amount(s) - just plain text, no comment markers
    let mut amounts: Vec<String> = totals
        .iter()
        .map(|(currency, value)| {
            let balancing = -value;
            format!("{} {}", balancing, currency)
        })
        .collect();
    amounts.sort(); // For consistent output

    // Find the column where amounts start in other postings for alignment
    let amount_column = find_amount_column(postings);

    // Find where the account name ends on this posting
    let account_end_column = find_account_end_column(&posting_without_amount.node);

    // Calculate how many spaces we need to align with other amounts
    let base_spaces = if amount_column > account_end_column {
        amount_column - account_end_column
    } else {
        2 // At least 2 spaces
    };

    // Check if the first (or only) amount is negative
    let first_amount_value = totals.values().next()?;
    let is_negative = (-first_amount_value).is_sign_negative();

    // Adjust spacing: if positive add 1 space, if negative subtract 1 space
    // (negative sign takes up a character)
    // But always ensure at least 2 spaces minimum
    let spaces_needed = if is_negative {
        base_spaces.saturating_sub(1).max(2)
    } else {
        (base_spaces + 1).max(2)
    };

    let label = if amounts.len() == 1 {
        format!("{:width$}{}", "", amounts[0], width = spaces_needed)
    } else {
        format!("{:width$}{}", "", amounts.join(", "), width = spaces_needed)
    };

    // Position at the end of the account name
    let range = posting_without_amount.node.range();
    // Use start_point.row to ensure we're on the posting line itself
    let position = Position::new(range.start_point.row as u32, account_end_column as u32);

    Some(InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: Some(InlayHintKind::PARAMETER),
        text_edits: None,
        tooltip: Some(lsp_types::InlayHintTooltip::String(
            "Calculated balancing amount".to_string(),
        )),
        padding_left: Some(false),
        padding_right: Some(false),
        data: None,
    })
}

/// Find the column where amounts typically appear in postings for alignment
fn find_amount_column(postings: &[Posting]) -> usize {
    // Look at postings with amounts to find where the amount starts
    for posting in postings {
        if posting.amount.is_some() {
            // Find the amount node within this posting
            let mut cursor = posting.node.walk();
            for child in posting.node.children(&mut cursor) {
                if child.kind() == "incomplete_amount" || child.kind() == "amount" {
                    return child.start_position().column;
                }
            }
        }
    }

    // Default to column 52 if we can't find any amounts (common beancount alignment)
    52
}

/// Find where the account name ends in a posting
fn find_account_end_column(posting_node: &tree_sitter::Node) -> usize {
    let mut cursor = posting_node.walk();
    for child in posting_node.children(&mut cursor) {
        if child.kind() == "account" {
            return child.end_position().column;
        }
    }
    // Default if we can't find the account
    posting_node.start_position().column
}

/// Calculate hint for transaction total (only when not balanced)
fn calculate_total_hint(postings: &[Posting], position: Position) -> Option<InlayHint> {
    // Calculate total for each currency
    // If a posting has a price, convert it to the price currency
    let mut totals: HashMap<String, rust_decimal::Decimal> = HashMap::new();

    for posting in postings {
        if let Some(posting_amount) = &posting.amount {
            // Check if this posting has a price annotation
            if let Some((converted_value, converted_currency)) =
                posting_amount.convert_to_currency()
            {
                // Use the converted amount and currency
                *totals
                    .entry(converted_currency)
                    .or_insert(rust_decimal::Decimal::ZERO) += converted_value;
            } else {
                // Use the original amount and currency
                *totals
                    .entry(posting_amount.amount.currency.clone())
                    .or_insert(rust_decimal::Decimal::ZERO) += posting_amount.amount.value;
            }
        }
    }

    // Check if any currency doesn't balance (non-zero total)
    let unbalanced: Vec<_> = totals
        .iter()
        .filter(|(_, value)| !value.is_zero())
        .collect();

    if unbalanced.is_empty() {
        // Transaction is balanced, no hint needed
        return None;
    }

    // Format the unbalanced amounts
    let mut amounts: Vec<String> = unbalanced
        .iter()
        .map(|(currency, value)| format!("{} {}", value, currency))
        .collect();
    amounts.sort(); // For consistent output

    let label = if amounts.len() == 1 {
        format!("  total = {} ⚠", amounts[0])
    } else {
        format!("  total = {} ⚠", amounts.join(", "))
    };

    Some(InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: Some(lsp_types::InlayHintTooltip::String(
            "Transaction does not balance".to_string(),
        )),
        padding_left: Some(true),
        padding_right: None,
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_amount_parse() {
        let amount = Amount::parse("100.00 USD").unwrap();
        assert_eq!(amount.value, Decimal::from_str("100.00").unwrap());
        assert_eq!(amount.currency, "USD");

        let amount = Amount::parse("100.00USD").unwrap();
        assert_eq!(amount.value, Decimal::from_str("100.00").unwrap());
        assert_eq!(amount.currency, "USD");

        let amount = Amount::parse("-45.23 EUR").unwrap();
        assert_eq!(amount.value, Decimal::from_str("-45.23").unwrap());
        assert_eq!(amount.currency, "EUR");
    }

    #[test]
    fn test_amount_parse_invalid() {
        assert!(Amount::parse("invalid").is_none());
        assert!(Amount::parse("").is_none());
        assert!(Amount::parse("USD").is_none());
    }

    #[test]
    fn test_balancing_hint() {
        // Test transaction with one missing amount
        let content = r#"2024-01-15 * "Grocery Store"
  Expenses:Food:Groceries    45.23 USD
  Assets:Bank:Checking
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have at least the balancing hint
            assert!(!hints.is_empty());

            // Check that one hint is for balancing amount - plain format, no comment markers
            let balancing_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("-45.23") && !label.contains("/*")
                } else {
                    false
                }
            });
            assert!(
                balancing_hint.is_some(),
                "Should have balancing hint in plain format"
            );

            // Verify hint is on line 2 (the posting line without amount)
            let hint = balancing_hint.unwrap();
            assert_eq!(
                hint.position.line, 2,
                "Hint should be on the posting line without amount (line 2), got line {}",
                hint.position.line
            );

            // Verify hint label starts with spaces for alignment
            if let InlayHintLabel::String(label) = &hint.label {
                assert!(
                    label.starts_with(' '),
                    "Hint label should start with spaces for alignment, got: '{}'",
                    label
                );
                assert!(
                    label.contains("-45.23"),
                    "Hint should contain the amount -45.23"
                );
            } else {
                panic!("Expected string label");
            }
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_unbalanced_transaction_hint() {
        // Test unbalanced transaction
        let content = r#"2024-01-15 * "Transfer"
  Assets:Savings           1000.00 USD
  Assets:Checking         -500.00 USD
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have the unbalanced hint
            assert!(!hints.is_empty());

            // Check for warning symbol in hint with comment style
            let unbalanced_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠") && label.contains("500.00") && label.contains("total =")
                } else {
                    false
                }
            });
            assert!(
                unbalanced_hint.is_some(),
                "Should have unbalanced warning hint with comment style"
            );

            // Verify hint is on line 0 (transaction line)
            let hint = unbalanced_hint.unwrap();
            assert_eq!(
                hint.position.line, 0,
                "Hint should be on transaction line (line 0)"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_balanced_transaction_no_total_hint() {
        // Test balanced transaction - should not show total hint
        let content = r#"2024-01-15 * "Transfer"
  Assets:Savings           1000.00 USD
  Assets:Checking         -1000.00 USD
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should not have a warning hint for balanced transaction
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                !has_warning,
                "Balanced transaction should not have warning hint"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_balanced_transaction_with_missing_amount() {
        // Test balanced transaction with missing amount - should show balancing hint on posting line
        let content = r#"2024-01-15 * "Transfer"
  Assets:Savings           1000.00 USD
  Assets:Checking
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have balancing hint
            let balancing_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("-1000.00") && !label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                balancing_hint.is_some(),
                "Should have balancing hint without warning"
            );

            // Verify it's on the posting line (line 2)
            let hint = balancing_hint.unwrap();
            assert_eq!(
                hint.position.line, 2,
                "Hint should be on posting line (line 2), got line {}",
                hint.position.line
            );

            // Verify hint label starts with spaces for alignment
            if let InlayHintLabel::String(label) = &hint.label {
                assert!(
                    label.starts_with(' '),
                    "Hint label should start with spaces for alignment, got: '{}'",
                    label
                );
                assert!(
                    label.contains("-1000.00"),
                    "Hint should contain the amount -1000.00"
                );
            } else {
                panic!("Expected string label");
            }

            // Should not have warning
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                !has_warning,
                "Balanced transaction should not have warning hint"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_positive_balancing_amount_spacing() {
        // Test that positive balancing amounts get extra space for alignment
        let content = r#"2024-01-15 * "Purchase"
  Liabilities:CreditCard              -50.00 USD
  Expenses:Shopping
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Find the balancing hint (positive amount)
            let balancing_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("50.00") && !label.contains("-")
                } else {
                    false
                }
            });
            assert!(
                balancing_hint.is_some(),
                "Should have positive balancing hint"
            );

            // Verify the hint has proper spacing for positive amount
            if let InlayHintLabel::String(label) = &balancing_hint.unwrap().label {
                // Positive amounts should have extra space (no minus sign)
                assert!(
                    label.starts_with(' '),
                    "Positive amount hint should start with spaces"
                );
                assert!(
                    label.contains("50.00 USD"),
                    "Should contain positive amount 50.00 USD"
                );
                // Count leading spaces - should have at least one extra for positive
                let leading_spaces = label.chars().take_while(|c| *c == ' ').count();
                assert!(
                    leading_spaces >= 1,
                    "Positive amount should have spacing, got {} spaces",
                    leading_spaces
                );
            }
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_negative_balancing_amount_spacing() {
        // Test that negative balancing amounts get reduced space (minus sign takes a character)
        let content = r#"2024-01-15 * "Income"
  Income:Salary                        50.00 USD
  Assets:Bank
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Find the balancing hint (negative amount)
            let balancing_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("-50.00")
                } else {
                    false
                }
            });
            assert!(
                balancing_hint.is_some(),
                "Should have negative balancing hint"
            );

            // Verify the hint has reduced spacing for negative amount
            if let InlayHintLabel::String(label) = &balancing_hint.unwrap().label {
                assert!(
                    label.contains("-50.00 USD"),
                    "Should contain negative amount -50.00 USD"
                );
                // The spacing should be present but potentially less than positive amounts
                // because the minus sign takes up a character
                let leading_spaces = label.chars().take_while(|c| *c == ' ').count();
                assert!(
                    leading_spaces >= 2,
                    "Negative amount should have at least 2 spaces minimum, got {} spaces",
                    leading_spaces
                );
            }
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_long_account_name_alignment() {
        // Test alignment with very long account name
        let content = r#"2024-01-15 * "Long Account Test"
  Expenses:Food:Groceries              45.00 USD
  Assets:Bank:Checking:Personal:Daily:Operations
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have balancing hint
            assert!(!hints.is_empty(), "Should have balancing hint");

            let balancing_hint = &hints[0];
            // Verify hint is on the posting line with long account
            assert_eq!(
                balancing_hint.position.line, 2,
                "Hint should be on posting line with long account"
            );

            // Verify it has spacing (at least 2 spaces minimum)
            if let InlayHintLabel::String(label) = &balancing_hint.label {
                let leading_spaces = label.chars().take_while(|c| *c == ' ').count();
                assert!(
                    leading_spaces >= 2,
                    "Should have at least 2 spaces even for long account names, got {}",
                    leading_spaces
                );
            }
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_multi_currency_with_unit_price() {
        // Test multi-currency transaction with @ (unit price) - should balance
        let content = r#"2024-01-15 * "Currency exchange with unit price"
  Assets:USD              100.00 USD @ 0.8 EUR
  Assets:EUR              -80.00 EUR
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should not have warning hint - transaction should balance with conversion
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                !has_warning,
                "Transaction with correct price conversion should not show warning"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_multi_currency_with_total_price() {
        // Test multi-currency transaction with @@ (total cost) - should balance
        let content = r#"2024-01-15 * "Currency exchange with total cost"
  Assets:USD              100.00 USD @@ 80.00 EUR
  Assets:EUR              -80.00 EUR
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should not have warning hint - transaction should balance with conversion
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                !has_warning,
                "Transaction with correct total cost should not show warning"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_multi_currency_without_price_unbalanced() {
        // Test multi-currency without price - should show as unbalanced
        let content = r#"2024-01-15 * "Multi-currency without conversion"
  Assets:USD              100.00 USD
  Assets:EUR              -80.00 EUR
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have warning hint - different currencies without conversion
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                has_warning,
                "Multi-currency transaction without conversion should show warning"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_cost_basis_unit_cost() {
        // Test transaction with unit cost basis - should balance
        let content = r#"2026-01-08 * "Starbucks purchase"
  Liabilities:CC:Apple                      -25 USD
  Assets:Cash:Starbucks:Brian                25 STARBUCKS_GC {1 USD}
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should not have warning hint - transaction should balance with cost basis
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                !has_warning,
                "Transaction with correct cost basis should not show warning"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_cost_basis_with_missing_amount() {
        // Test transaction with cost basis and missing amount
        let content = r#"2026-01-08 * "Gift card purchase"
  Assets:Checking
  Liabilities:CC                             25 STARBUCKS_GC {1 USD}
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have balancing hint showing the converted cost amount
            let balancing_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    // 25 * 1 USD = 25 USD
                    label.contains("-25") && label.contains("USD")
                } else {
                    false
                }
            });
            assert!(
                balancing_hint.is_some(),
                "Should show balancing hint with cost-converted amount"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_cost_basis_without_conversion_unbalanced() {
        // Test transaction with different commodities but no cost - should show as unbalanced
        let content = r#"2026-01-08 * "Purchase without cost"
  Assets:Checking                            -25 USD
  Liabilities:CC                             25 STARBUCKS_GC
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have warning hint - different commodities without conversion
            let has_warning = hints.iter().any(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    label.contains("⚠")
                } else {
                    false
                }
            });
            assert!(
                has_warning,
                "Transaction with different commodities and no cost should show warning"
            );
        } else {
            panic!("No transaction found");
        }
    }

    #[test]
    fn test_multi_currency_with_missing_amount_and_price() {
        // Test transaction with missing amount and price conversion
        let content = r#"2024-01-15 * "Purchase with price"
  Expenses:Shopping        50.00 USD @ 0.85 EUR
  Assets:EUR
"#;
        let rope_content = ropey::Rope::from_str(content);

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(content, None).unwrap();

        let txn_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), TRANSACTION_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let content_bytes = content.as_bytes();
        let mut matches = cursor.matches(&txn_query, tree.root_node(), content_bytes);

        if let Some(qmatch) = matches.next() {
            let txn_node = qmatch.captures[0].node;
            let hints = process_transaction(&txn_node, &rope_content).unwrap();

            // Should have balancing hint showing the converted amount
            let balancing_hint = hints.iter().find(|h| {
                if let InlayHintLabel::String(label) = &h.label {
                    // 50 * 0.85 = 42.5 EUR
                    label.contains("-42.5") && label.contains("EUR")
                } else {
                    false
                }
            });
            assert!(
                balancing_hint.is_some(),
                "Should show balancing hint with converted amount"
            );
        } else {
            panic!("No transaction found");
        }
    }
}
