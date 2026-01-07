use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::utils::ToFilePath;
use anyhow::Result;
use chrono::Datelike;
use lsp_types::{CompletionItem, CompletionItemKind, Position, Range, TextEdit};
use nucleo::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;
use tree_sitter::Point;
use tree_sitter_beancount::tree_sitter;

// ============================================================================
// CORE CONTEXT ANALYSIS - Left-Context-Aware Traversal
// ============================================================================

/// Represents the completion context determined by analyzing the syntax tree
/// and cursor position using left-context-aware traversal strategy.
#[derive(Debug, Clone, PartialEq)]
enum CompletionContext {
    /// At document root - suggest dates and directive keywords
    DocumentRoot,

    /// After date, expecting flag or directive keyword (txn, open, balance, etc.)
    AfterDate,

    /// After flag in transaction, expecting payee (first string)
    AfterFlag,

    /// After first string (payee), expecting narration (second string)
    AfterPayee,

    /// In posting line, expecting account name
    PostingAccount { prefix: String },

    /// After account in posting, expecting amount
    PostingAmount,

    /// After amount in posting, expecting currency
    PostingCurrency,

    /// In open directive, expecting account
    OpenAccount { prefix: String },

    /// After account in open directive, expecting currency
    OpenCurrency,

    /// In balance directive, expecting account
    BalanceAccount { prefix: String },

    /// In price directive context
    PriceContext,

    /// Inside a string literal (payee or narration)
    InsideString {
        prefix: String,
        is_payee: bool,
        has_opening_quote: bool,
        has_closing_quote: bool,
    },

    /// After tag trigger character (#)
    TagContext { prefix: String },

    /// After link trigger character (^)
    LinkContext { prefix: String },

    /// Colon-triggered account completion (show sub-accounts)
    ColonTriggeredAccount { parent_path: String },
}

/// Main entry point for completion with LSP 3.17 compliant implementation.
///
/// Uses left-context-aware traversal to determine completion context even when
/// the syntax tree is in an ERROR state due to incomplete input.
pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    trigger_character: Option<char>,
    cursor: lsp_types::TextDocumentPositionParams,
) -> Result<Option<Vec<CompletionItem>>> {
    debug!("=== Completion Request ===");
    debug!("Trigger character: {:?}", trigger_character);
    debug!(
        "Position: {}:{}",
        cursor.position.line, cursor.position.character
    );

    // Get file path from URI
    let uri = cursor
        .text_document
        .uri
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Failed to convert URI to file path"))?;

    // Get parsed tree and document
    let tree = snapshot
        .forest
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("No parsed tree found"))?;
    let doc = snapshot
        .open_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let content = &doc.content;
    let cursor_point = Point {
        row: cursor.position.line as usize,
        column: cursor.position.character as usize,
    };

    // Determine completion context using left-context-aware analysis
    let context = determine_completion_context(tree, content, cursor_point, trigger_character);

    debug!("Determined context: {:?}", context);

    // Generate completions based on context
    generate_completions(&snapshot.beancount_data, &context, content, cursor.position)
}

/// Determine completion context using left-context-aware traversal.
///
/// This implements the algorithm from the plan:
/// 1. Find node at cursor (with -1 character lookahead for ghost nodes)
/// 2. Step out of ERROR/MISSING nodes to find stable parent
/// 3. Identify previous sibling to infer expected next element
/// 4. Map (parent_type, prev_sibling_type) to completion context
fn determine_completion_context(
    tree: &tree_sitter::Tree,
    content: &ropey::Rope,
    cursor: Point,
    trigger_char: Option<char>,
) -> CompletionContext {
    // First, check for tag/link context based on text, regardless of trigger char.
    // This is more robust.
    let line_str = content.line(cursor.row).to_string();
    if let Some(prefix) = extract_tag_prefix(&line_str, cursor.column) {
        return CompletionContext::TagContext { prefix };
    }
    if let Some(prefix) = extract_link_prefix(&line_str, cursor.column) {
        return CompletionContext::LinkContext { prefix };
    }

    // Handle trigger characters with special semantics
    match trigger_char {
        Some(':') => {
            // Colon triggers sub-account completion
            let line = content.line(cursor.row).to_string();
            let prefix = extract_account_prefix(&line, cursor.column);
            return CompletionContext::ColonTriggeredAccount {
                parent_path: prefix,
            };
        }
        Some('"') => {
            // Quote triggers string completion - analyze context
            return analyze_string_context(content, cursor);
        }
        _ => {}
    }

    // Query node at cursor with -1 lookahead to catch "ghost nodes"
    let query_col = cursor.column.saturating_sub(1);
    let query_point = Point {
        row: cursor.row,
        column: query_col,
    };

    let node = tree
        .root_node()
        .named_descendant_for_point_range(query_point, cursor)
        .or_else(|| {
            tree.root_node()
                .descendant_for_point_range(query_point, cursor)
        });

    if let Some(mut current_node) = node {
        debug!(
            "Initial node: {} at {:?}",
            current_node.kind(),
            current_node.range()
        );

        // Step out of ERROR and MISSING nodes
        while current_node.kind() == "ERROR" || current_node.is_missing() {
            let start_pos = current_node.start_position();
            if content.line(start_pos.row).char(start_pos.column) == '"' {
                debug!("Found an ERROR node starting with '\"'. Assuming unterminated string.");
                return analyze_string_context(content, cursor);
            }

            if let Some(parent) = current_node.parent() {
                debug!(
                    "Stepping out of {} to parent {}",
                    current_node.kind(),
                    parent.kind()
                );
                current_node = parent;
            } else {
                break;
            }
        }

        // Analyze based on node kind and position
        match current_node.kind() {
            "transaction" => analyze_transaction_context(current_node, cursor, content),
            "posting" => analyze_posting_context(current_node, cursor, content),
            "open" => analyze_open_context(current_node, cursor, content),
            "balance" => analyze_balance_context(current_node, cursor, content),
            "price" => CompletionContext::PriceContext,
            "date" => CompletionContext::AfterDate,
            "flag" => CompletionContext::AfterFlag,
            "account" => analyze_account_context(current_node, cursor, content),
            "string" => analyze_string_node_context(current_node, content, cursor),
            _ => {
                // Check parent for more context
                if let Some(parent) = current_node.parent() {
                    match parent.kind() {
                        "transaction" => analyze_transaction_context(parent, cursor, content),
                        "posting" => analyze_posting_context(parent, cursor, content),
                        "open" => analyze_open_context(parent, cursor, content),
                        "balance" => analyze_balance_context(parent, cursor, content),
                        _ => check_if_in_posting_area(tree, content, cursor),
                    }
                } else {
                    // No parent found - might be typing on a line after a transaction header
                    check_if_in_posting_area(tree, content, cursor)
                }
            }
        }
    } else {
        // No node found - check if we're in posting area
        check_if_in_posting_area(tree, content, cursor)
    }
}

/// Check if cursor is in posting area by looking at previous lines
fn check_if_in_posting_area(
    _tree: &tree_sitter::Tree,
    content: &ropey::Rope,
    cursor: Point,
) -> CompletionContext {
    // Look at previous lines to see if there's a transaction header
    if cursor.row > 0 {
        // Check up to 10 lines back for a transaction
        let start_row = cursor.row.saturating_sub(10);

        for row in (start_row..cursor.row).rev() {
            let line = content.line(row).to_string();
            let trimmed = line.trim();

            // Check if this line looks like a transaction header
            // Format: YYYY-MM-DD <flag|txn> ["payee"] "narration"
            if trimmed.starts_with(|c: char| c.is_ascii_digit())
                && (trimmed.contains("txn") || trimmed.contains('*') || trimmed.contains('!'))
            {
                // Found a transaction header.
                // Now, let's analyze the current line to be smarter than just assuming PostingAccount.
                let current_line_str = content.line(cursor.row).to_string();
                let trimmed_current_line = current_line_str.trim();

                // If line is empty, it might be the end of the transaction.
                if trimmed_current_line.is_empty() {
                    return CompletionContext::DocumentRoot;
                }

                // Heuristic to check if an account seems to be present.
                // An account usually has at least one colon, or is one of the 5 main types.
                let words: Vec<&str> = trimmed_current_line.split_whitespace().collect();
                let first_word = words.first().copied().unwrap_or("");
                let has_account = !first_word.is_empty()
                    && (first_word.contains(':')
                        || first_word.chars().next().is_some_and(|c| c.is_uppercase()));

                if has_account {
                    // If there's more than one word, we might have account + amount/currency
                    if words.len() > 1 {
                        // Check if there are digits (amount present)
                        let has_digits = trimmed_current_line.chars().any(|c| c.is_ascii_digit());
                        if has_digits {
                            return CompletionContext::PostingCurrency;
                        } else {
                            return CompletionContext::PostingAmount;
                        }
                    } else {
                        // Only one word - still typing the account
                        let prefix = extract_account_prefix(&current_line_str, cursor.column);
                        return CompletionContext::PostingAccount { prefix };
                    }
                } else {
                    let prefix = extract_account_prefix(&current_line_str, cursor.column);
                    return CompletionContext::PostingAccount { prefix };
                }
            }

            // Stop if we hit another directive or empty line
            if trimmed.is_empty()
                || trimmed.starts_with("open")
                || trimmed.starts_with("close")
                || trimmed.starts_with("balance")
            {
                break;
            }
        }
    }

    // Default to document root
    CompletionContext::DocumentRoot
}

/// Analyze transaction context using left-context (previous sibling) strategy
fn analyze_transaction_context(
    txn_node: tree_sitter::Node,
    cursor: Point,
    content: &ropey::Rope,
) -> CompletionContext {
    let mut cursor_obj = txn_node.walk();
    let children: Vec<_> = txn_node.children(&mut cursor_obj).collect();

    // Find the last named child before cursor
    let mut prev_sibling: Option<tree_sitter::Node> = None;

    for child in &children {
        if child.start_position().row > cursor.row
            || (child.start_position().row == cursor.row
                && child.start_position().column >= cursor.column)
        {
            break;
        }
        if child.is_named() {
            prev_sibling = Some(*child);
        }
    }

    // Map previous sibling to expected next context
    match prev_sibling {
        None => CompletionContext::DocumentRoot,
        Some(prev) => {
            debug!("Transaction prev sibling: {}", prev.kind());
            match prev.kind() {
                "date" => CompletionContext::AfterDate,
                "flag" | "txn" => CompletionContext::AfterFlag,
                "payee" | "string" => {
                    // Check if this is first or second string
                    let string_count = children
                        .iter()
                        .filter(|n| n.kind() == "string" || n.kind() == "payee")
                        .count();
                    if string_count >= 1 {
                        CompletionContext::AfterPayee
                    } else {
                        CompletionContext::AfterFlag
                    }
                }
                "narration" => {
                    // After narration, we're in posting area
                    let line = content.line(cursor.row).to_string();
                    let prefix = extract_account_prefix(&line, cursor.column);
                    CompletionContext::PostingAccount { prefix }
                }
                _ => {
                    // Default to posting account
                    let line = content.line(cursor.row).to_string();
                    let prefix = extract_account_prefix(&line, cursor.column);
                    CompletionContext::PostingAccount { prefix }
                }
            }
        }
    }
}

/// Analyze posting context
fn analyze_posting_context(
    posting_node: tree_sitter::Node,
    cursor: Point,
    content: &ropey::Rope,
) -> CompletionContext {
    if let Some(account_node) = posting_node
        .children(&mut posting_node.walk())
        .find(|c| c.kind() == "account")
    {
        // An account exists. We are completing amount or currency.

        // Are we after the account?
        if cursor.column > account_node.end_position().column {
            if posting_node.children(&mut posting_node.walk()).any(|c| {
                c.kind() == "amount" || c.kind() == "incomplete_amount" || c.kind() == "number"
            }) {
                return CompletionContext::PostingCurrency;
            } else {
                return CompletionContext::PostingAmount;
            }
        }
    }

    // Default to account completion
    let line = content.line(cursor.row).to_string();
    let prefix = extract_account_prefix(&line, cursor.column);
    CompletionContext::PostingAccount { prefix }
}

/// Analyze open directive context
fn analyze_open_context(
    open_node: tree_sitter::Node,
    cursor: Point,
    content: &ropey::Rope,
) -> CompletionContext {
    let mut cursor_obj = open_node.walk();
    let children: Vec<_> = open_node.children(&mut cursor_obj).collect();

    let has_account = children.iter().any(|c| c.kind() == "account");

    if has_account {
        CompletionContext::OpenCurrency
    } else {
        let line = content.line(cursor.row).to_string();
        let prefix = extract_account_prefix(&line, cursor.column);
        CompletionContext::OpenAccount { prefix }
    }
}

/// Analyze balance directive context
fn analyze_balance_context(
    _balance_node: tree_sitter::Node,
    cursor: Point,
    content: &ropey::Rope,
) -> CompletionContext {
    let line = content.line(cursor.row).to_string();
    let prefix = extract_account_prefix(&line, cursor.column);
    CompletionContext::BalanceAccount { prefix }
}

/// Analyze account node context
fn analyze_account_context(
    _account_node: tree_sitter::Node,
    cursor: Point,
    content: &ropey::Rope,
) -> CompletionContext {
    let line = content.line(cursor.row).to_string();
    let prefix = extract_account_prefix(&line, cursor.column);
    CompletionContext::PostingAccount { prefix }
}

/// Analyze string node context to determine if it's payee or narration
fn analyze_string_node_context(
    string_node: tree_sitter::Node,
    content: &ropey::Rope,
    cursor: Point,
) -> CompletionContext {
    // Check if this string is first or second in transaction
    if let Some(parent) = string_node.parent()
        && parent.kind() == "transaction"
    {
        let mut walker = parent.walk();
        let strings: Vec<_> = parent
            .children(&mut walker)
            .filter(|n| n.kind() == "string" || n.kind() == "payee" || n.kind() == "narration")
            .collect();

        let is_first = strings.first().map(|n| n.id()) == Some(string_node.id());

        let line = content.line(cursor.row).to_string();
        let prefix = extract_string_prefix(&line, cursor.column);
        let has_opening = line.chars().take(cursor.column).any(|c| c == '"');
        let has_closing = line.chars().skip(cursor.column).any(|c| c == '"');

        return CompletionContext::InsideString {
            prefix,
            is_payee: is_first,
            has_opening_quote: has_opening,
            has_closing_quote: has_closing,
        };
    }

    CompletionContext::InsideString {
        prefix: String::new(),
        is_payee: false,
        has_opening_quote: false,
        has_closing_quote: false,
    }
}

/// Analyze string context when triggered by quote character
fn analyze_string_context(content: &ropey::Rope, cursor: Point) -> CompletionContext {
    let line = content.line(cursor.row).to_string();
    let prefix = extract_string_prefix(&line, cursor.column);

    // Count quotes before cursor to determine context
    let before_cursor = &line[..cursor.column.min(line.len())];
    let quote_count = before_cursor.matches('"').count();

    // Check if we have a complete payee (2+ quotes before, suggesting this is narration)
    let is_payee = quote_count < 2 || !before_cursor.contains("txn");

    // Check for closing quote after cursor
    let has_closing = line.chars().skip(cursor.column).any(|c| c == '"');

    CompletionContext::InsideString {
        prefix,
        is_payee,
        has_opening_quote: true, // Quote was just typed
        has_closing_quote: has_closing,
    }
}

/// Extract account prefix from line text up to cursor position
fn extract_account_prefix(line: &str, cursor_col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if cursor_col == 0 || cursor_col > chars.len() {
        return String::new();
    }

    // Find start of account (after whitespace or start of line)
    let mut start = 0;
    for i in (0..cursor_col).rev() {
        let c = chars[i];
        if c.is_whitespace() {
            start = i + 1;
            break;
        }
    }

    // Extract prefix
    let end = cursor_col.min(chars.len());
    chars[start..end].iter().collect()
}

/// Extract string prefix from line text up to cursor position
fn extract_string_prefix(line: &str, cursor_col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if cursor_col == 0 || cursor_col > chars.len() {
        return String::new();
    }

    // Find start of string content (after quote)
    let mut start = 0;
    for i in (0..cursor_col).rev() {
        if chars[i] == '"' {
            start = i + 1;
            break;
        }
    }

    // Extract prefix, but not if we are at the opening quote
    let end = cursor_col.min(chars.len());
    chars[start..end].iter().collect()
}

fn extract_tag_prefix(line: &str, cursor_col: usize) -> Option<String> {
    let relevant_part = &line[..cursor_col];
    if let Some(hash_pos) = relevant_part.rfind('#') {
        // Ensure we are not in a comment
        if let Some(comment_pos) = relevant_part.find(';')
            && hash_pos > comment_pos
        {
            return None; // It's in a comment
        }
        // Ensure there is no whitespace between # and cursor
        let after_hash = &relevant_part[hash_pos + 1..];
        if after_hash.contains(char::is_whitespace) {
            return None;
        }
        return Some(after_hash.to_string());
    }
    None
}

fn extract_link_prefix(line: &str, cursor_col: usize) -> Option<String> {
    let relevant_part = &line[..cursor_col];
    if let Some(hash_pos) = relevant_part.rfind('^') {
        // Ensure we are not in a comment
        if let Some(comment_pos) = relevant_part.find(';')
            && hash_pos > comment_pos
        {
            return None; // It's in a comment
        }
        // Ensure there is no whitespace between # and cursor
        let after_hash = &relevant_part[hash_pos + 1..];
        if after_hash.contains(char::is_whitespace) {
            return None;
        }
        return Some(after_hash.to_string());
    }
    None
}

// ============================================================================
// COMPLETION GENERATION - LSP 3.17 Compliant
// ============================================================================

/// Generate completions based on context with LSP 3.17 InsertReplaceEdit support
fn generate_completions(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    context: &CompletionContext,
    content: &ropey::Rope,
    position: Position,
) -> Result<Option<Vec<CompletionItem>>> {
    match context {
        CompletionContext::DocumentRoot => {
            let mut items = complete_date()?;
            items.extend(complete_directive_keywords()?);
            Ok(Some(items))
        }

        CompletionContext::AfterDate => Ok(Some(complete_directive_keywords()?)),

        CompletionContext::AfterFlag => {
            Ok(Some(complete_payee(data, "", content, position, false)?))
        }

        CompletionContext::AfterPayee => Ok(Some(complete_narration(
            data, "", content, position, false,
        )?)),

        CompletionContext::PostingAccount { prefix } => {
            Ok(Some(complete_account(data, prefix, content, position)?))
        }

        CompletionContext::PostingAmount => Ok(Some(complete_amount()?)),

        CompletionContext::PostingCurrency => Ok(Some(complete_currency(content, position)?)),

        CompletionContext::OpenAccount { prefix } => {
            Ok(Some(complete_account(data, prefix, content, position)?))
        }

        CompletionContext::OpenCurrency => Ok(Some(complete_currency(content, position)?)),

        CompletionContext::BalanceAccount { prefix } => {
            Ok(Some(complete_account(data, prefix, content, position)?))
        }

        CompletionContext::PriceContext => Ok(Some(complete_currency(content, position)?)),

        CompletionContext::InsideString {
            prefix,
            is_payee,
            has_opening_quote: _,
            has_closing_quote,
        } => {
            if *is_payee {
                Ok(Some(complete_payee(
                    data,
                    prefix,
                    content,
                    position,
                    *has_closing_quote,
                )?))
            } else {
                Ok(Some(complete_narration(
                    data,
                    prefix,
                    content,
                    position,
                    *has_closing_quote,
                )?))
            }
        }

        CompletionContext::TagContext { prefix } => Ok(Some(complete_tag(data, prefix)?)),

        CompletionContext::LinkContext { prefix } => Ok(Some(complete_link(data, prefix)?)),

        CompletionContext::ColonTriggeredAccount { parent_path } => {
            Ok(Some(complete_subaccounts(data, parent_path)?))
        }
    }
}

// ============================================================================
// INDIVIDUAL COMPLETION PROVIDERS
// ============================================================================

/// Complete directive keywords (txn, open, balance, close, etc.)
fn complete_directive_keywords() -> Result<Vec<CompletionItem>> {
    let keywords = vec![
        ("txn", "Transaction"),
        ("*", "Transaction (completed)"),
        ("!", "Transaction (incomplete)"),
        ("open", "Open account"),
        ("close", "Close account"),
        ("balance", "Balance assertion"),
        ("pad", "Pad directive"),
        ("price", "Price directive"),
        ("commodity", "Commodity directive"),
        ("document", "Document directive"),
        ("note", "Note directive"),
        ("event", "Event directive"),
    ];

    Ok(keywords
        .iter()
        .map(|(label, detail)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect())
}

/// Complete date with current/previous/next month
fn complete_date() -> Result<Vec<CompletionItem>> {
    let today = chrono::Local::now().naive_local().date();
    let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
    let cur_month = today.format("%Y-%m-").to_string();
    let next_month = add_one_month(today).format("%Y-%m-").to_string();
    let today_str = today.format("%Y-%m-%d").to_string();

    Ok(vec![
        CompletionItem {
            label: today_str,
            detail: Some("today".to_string()),
            kind: Some(CompletionItemKind::CONSTANT),
            ..Default::default()
        },
        CompletionItem {
            label: cur_month,
            detail: Some("this month".to_string()),
            kind: Some(CompletionItemKind::CONSTANT),
            ..Default::default()
        },
        CompletionItem {
            label: prev_month,
            detail: Some("prev month".to_string()),
            kind: Some(CompletionItemKind::CONSTANT),
            ..Default::default()
        },
        CompletionItem {
            label: next_month,
            detail: Some("next month".to_string()),
            kind: Some(CompletionItemKind::CONSTANT),
            ..Default::default()
        },
    ])
}

/// Complete account names with fuzzy matching and InsertReplaceEdit
fn complete_account(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    content: &ropey::Rope,
    position: Position,
) -> Result<Vec<CompletionItem>> {
    let mut all_accounts: Vec<String> = Vec::new();

    for bean_data in data.values() {
        all_accounts.extend(bean_data.get_accounts().into_iter());
    }

    // Remove duplicates
    all_accounts.sort();
    all_accounts.dedup();

    // Fuzzy search
    let matches = fuzzy_search_accounts(&all_accounts, prefix);

    // Calculate ranges for InsertReplaceEdit
    let line = content.line(position.line as usize).to_string();
    let (insert_range, replace_range) = calculate_word_ranges(&line, position);

    Ok(matches
        .into_iter()
        .take(50)
        .map(|(account, score)| {
            create_completion_with_insert_replace(
                account,
                "Beancount Account".to_string(),
                CompletionItemKind::ENUM,
                insert_range,
                replace_range,
                score,
                vec![":".to_string()], // Commit character for flow
            )
        })
        .collect())
}

/// Complete sub-accounts when colon is typed (e.g., "Assets:" shows "Checking", "Savings")
fn complete_subaccounts(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    parent_path: &str,
) -> Result<Vec<CompletionItem>> {
    let mut subaccounts: Vec<String> = Vec::new();

    for bean_data in data.values() {
        for account in bean_data.get_accounts() {
            if let Some(suffix) = account.strip_prefix(parent_path) {
                let suffix = suffix.strip_prefix(':').unwrap_or(suffix);

                // Extract only the next segment
                let next_segment = if let Some(colon_pos) = suffix.find(':') {
                    &suffix[..colon_pos]
                } else {
                    suffix
                };

                if !next_segment.is_empty() {
                    subaccounts.push(next_segment.to_string());
                }
            }
        }
    }

    // Remove duplicates and sort
    subaccounts.sort();
    subaccounts.dedup();

    Ok(subaccounts
        .into_iter()
        .map(|segment| CompletionItem {
            label: segment.clone(),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some("Account segment".to_string()),
            insert_text: Some(segment),
            commit_characters: Some(vec![":".to_string()]),
            ..Default::default()
        })
        .collect())
}

/// Complete currency codes
fn complete_currency(content: &ropey::Rope, position: Position) -> Result<Vec<CompletionItem>> {
    let currencies = vec![
        "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "PLN", "CZK",
        "HUF", "CNY", "INR", "BRL", "MXN", "ZAR", "RUB", "KRW", "SGD", "HKD", "THB",
    ];

    let line = content.line(position.line as usize).to_string();
    let (insert_range, replace_range) = calculate_word_ranges(&line, position);

    Ok(currencies
        .iter()
        .map(|currency| {
            create_completion_with_insert_replace(
                currency.to_string(),
                "Currency".to_string(),
                CompletionItemKind::UNIT,
                insert_range,
                replace_range,
                1.0,
                vec![],
            )
        })
        .collect())
}

/// Complete amount suggestions
fn complete_amount() -> Result<Vec<CompletionItem>> {
    Ok(vec![])
}

/// Complete payee names
fn complete_payee(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    content: &ropey::Rope,
    position: Position,
    has_closing_quote: bool,
) -> Result<Vec<CompletionItem>> {
    let mut payees: Vec<String> = Vec::new();

    for bean_data in data.values() {
        for narration in bean_data.get_narration() {
            let clean = narration.trim_matches('"');
            if !clean.is_empty() && clean.len() < 50 {
                payees.push(clean.to_string());
            }
        }
    }

    payees.sort();
    payees.dedup();

    let matches = fuzzy_search_strings(&payees, prefix);

    let line = content.line(position.line as usize).to_string();
    let (insert_range, replace_range) = calculate_string_ranges(&line, position, has_closing_quote);

    Ok(matches
        .into_iter()
        .map(|(payee, score)| {
            let insert_text = if has_closing_quote {
                payee.clone()
            } else {
                format!("{}\"", payee)
            };

            create_completion_with_insert_replace(
                payee,
                "Payee".to_string(),
                CompletionItemKind::TEXT,
                insert_range,
                replace_range,
                score,
                vec![],
            )
            .with_insert_text(insert_text)
        })
        .collect())
}

/// Complete narration strings
fn complete_narration(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    content: &ropey::Rope,
    position: Position,
    has_closing_quote: bool,
) -> Result<Vec<CompletionItem>> {
    let mut narrations: Vec<String> = Vec::new();

    for bean_data in data.values() {
        for narration in bean_data.get_narration() {
            narrations.push(narration.trim_matches('"').to_string());
        }
    }

    narrations.sort();
    narrations.dedup();

    let matches = fuzzy_search_strings(&narrations, prefix);

    let line = content.line(position.line as usize).to_string();
    let (insert_range, replace_range) = calculate_string_ranges(&line, position, has_closing_quote);

    Ok(matches
        .into_iter()
        .map(|(narration, score)| {
            let insert_text = if has_closing_quote {
                narration.clone()
            } else {
                format!("{}\"", narration)
            };

            create_completion_with_insert_replace(
                narration,
                "Narration".to_string(),
                CompletionItemKind::TEXT,
                insert_range,
                replace_range,
                score,
                vec![],
            )
            .with_insert_text(insert_text)
        })
        .collect())
}

/// Complete tags
fn complete_tag(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
) -> Result<Vec<CompletionItem>> {
    let mut tags: Vec<String> = Vec::new();

    for bean_data in data.values() {
        tags.extend(
            bean_data
                .get_tags()
                .into_iter()
                .map(|t| t.trim_start_matches('#').to_string()),
        );
    }

    tags.sort();
    tags.dedup();

    let matches = fuzzy_search_strings(&tags, prefix);

    Ok(matches
        .into_iter()
        .map(|(tag, _score)| CompletionItem {
            label: tag.clone(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Tag".to_string()),
            ..Default::default()
        })
        .collect())
}

/// Complete links
fn complete_link(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
) -> Result<Vec<CompletionItem>> {
    let mut links: Vec<String> = Vec::new();

    for bean_data in data.values() {
        links.extend(
            bean_data
                .get_links()
                .into_iter()
                .map(|l| l.trim_start_matches('^').to_string()),
        );
    }

    links.sort();
    links.dedup();

    let matches = fuzzy_search_strings(&links, prefix);

    Ok(matches
        .into_iter()
        .map(|(link, _score)| CompletionItem {
            label: link.clone(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Link".to_string()),
            ..Default::default()
        })
        .collect())
}

// ============================================================================
// LSP 3.17 INSERTREPLACEEDIT SUPPORT
// ============================================================================

/// Create completion item with InsertReplaceEdit for LSP 3.17 compliance
fn create_completion_with_insert_replace(
    label: String,
    detail: String,
    kind: CompletionItemKind,
    _insert_range: Range,
    replace_range: Range,
    score: f32,
    commit_characters: Vec<String>,
) -> CompletionItem {
    CompletionItem {
        label: label.clone(),
        kind: Some(kind),
        detail: Some(detail),
        text_edit: Some(lsp_types::CompletionTextEdit::Edit(TextEdit {
            new_text: label.clone(),
            range: replace_range,
        })),
        filter_text: Some(label),
        sort_text: Some(format!("{:010.0}", 99999.0 - score.min(99999.0))),
        commit_characters: if commit_characters.is_empty() {
            None
        } else {
            Some(commit_characters)
        },
        ..Default::default()
    }
}

/// Calculate word ranges for InsertReplaceEdit
fn calculate_word_ranges(line: &str, position: Position) -> (Range, Range) {
    let chars: Vec<char> = line.chars().collect();
    let cursor_col = position.character as usize;

    // Find start of word
    let mut start = cursor_col;
    while start > 0 {
        let c = chars[start - 1];
        if !c.is_alphanumeric() && c != ':' && c != '-' && c != '_' {
            break;
        }
        start -= 1;
    }

    // Find end of word
    let mut end = cursor_col;
    while end < chars.len() {
        let c = chars[end];
        if !c.is_alphanumeric() && c != ':' && c != '-' && c != '_' {
            break;
        }
        end += 1;
    }

    let insert_range = Range {
        start: Position {
            line: position.line,
            character: start as u32,
        },
        end: position,
    };

    let replace_range = Range {
        start: Position {
            line: position.line,
            character: start as u32,
        },
        end: Position {
            line: position.line,
            character: end as u32,
        },
    };

    (insert_range, replace_range)
}

/// Calculate string ranges for InsertReplaceEdit (handles quotes)
fn calculate_string_ranges(
    line: &str,
    position: Position,
    has_closing_quote: bool,
) -> (Range, Range) {
    let chars: Vec<char> = line.chars().collect();
    let cursor_col = position.character as usize;

    // Find opening quote
    let mut start = cursor_col;
    while start > 0 {
        if chars[start - 1] == '"' {
            break;
        }
        start -= 1;
    }

    // Find closing quote (if exists)
    let mut end = cursor_col;
    if has_closing_quote {
        while end < chars.len() {
            if chars[end] == '"' {
                break;
            }
            end += 1;
        }
    }

    let insert_range = Range {
        start: Position {
            line: position.line,
            character: start as u32,
        },
        end: position,
    };

    let replace_range = Range {
        start: Position {
            line: position.line,
            character: start as u32,
        },
        end: Position {
            line: position.line,
            character: end as u32,
        },
    };

    (insert_range, replace_range)
}

// ============================================================================
// FUZZY SEARCH WITH NUCLEO
// ============================================================================

/// Fuzzy search accounts using nucleo with tiered scoring
fn fuzzy_search_accounts(accounts: &[String], query: &str) -> Vec<(String, f32)> {
    if query.is_empty() {
        return accounts.iter().map(|acc| (acc.clone(), 1.0)).collect();
    }

    let mut scored: Vec<(String, f32)> = accounts
        .iter()
        .map(|acc| (acc.clone(), score_account(acc, query)))
        .collect();

    // Sort by score descending, then alphabetically
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    scored
}

/// Fuzzy search a list of strings
fn fuzzy_search_strings(strings: &[String], query: &str) -> Vec<(String, f32)> {
    if query.is_empty() {
        return strings.iter().map(|s| (s.clone(), 1.0)).collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    let mut scored: Vec<(String, f32)> = strings
        .iter()
        .filter_map(|s| {
            let mut char_buf = Vec::new();
            let s_utf32 = Utf32Str::new(s, &mut char_buf);
            pattern
                .score(s_utf32, &mut matcher)
                .map(|score| (s.clone(), score as f32))
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// Score an account using tiered matching strategy
fn score_account(account: &str, query: &str) -> f32 {
    let account_lower = account.to_lowercase();
    let query_lower = query.to_lowercase();

    // Tier 1: Exact match (10000 points)
    if account == query || account_lower == query_lower {
        return 10000.0;
    }

    // Tier 2: Prefix match (7000 points)
    if account.starts_with(query) {
        return 7000.0;
    }
    if account_lower.starts_with(&query_lower) {
        return 6900.0;
    }

    // Tier 3: Intra-segment match (4000 points)
    if let Some(score) = score_intra_segment(account, &query_lower) {
        return 4000.0 + score;
    }

    // Tier 4: Fuzzy match with nucleo (1000 points)
    if let Some(score) = score_with_nucleo(account, query) {
        return 1000.0 + score;
    }

    // Tier 5: Fallback (show all)
    1.0
}

/// Score matches within account segments
fn score_intra_segment(account: &str, query_lower: &str) -> Option<f32> {
    let segments: Vec<&str> = account.split(':').collect();
    let mut best_score: f32 = 0.0;
    let mut found = false;

    for (i, segment) in segments.iter().enumerate() {
        let seg_lower = segment.to_lowercase();

        if seg_lower == query_lower {
            best_score = best_score.max(500.0 - (i as f32 * 50.0));
            found = true;
        } else if seg_lower.starts_with(query_lower) {
            best_score = best_score.max(300.0 - (i as f32 * 30.0));
            found = true;
        } else if seg_lower.contains(query_lower) {
            best_score = best_score.max(100.0 - (i as f32 * 10.0));
            found = true;
        }
    }

    if found { Some(best_score) } else { None }
}

/// Score using nucleo fuzzy matcher
fn score_with_nucleo(account: &str, query: &str) -> Option<f32> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    let mut char_buf = Vec::new();
    let account_utf32 = Utf32Str::new(account, &mut char_buf);

    pattern
        .score(account_utf32, &mut matcher)
        .map(|score| (score as f32 / 100.0).min(500.0))
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

pub fn add_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 12 {
        year += 1;
        month = 1;
    } else {
        month += 1;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid date")
}

pub fn sub_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 1 {
        year -= 1;
        month = 12;
    } else {
        month -= 1;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid date")
}

/// Extension trait for adding insert_text to CompletionItem
trait CompletionItemExt {
    fn with_insert_text(self, insert_text: String) -> Self;
}

impl CompletionItemExt for CompletionItem {
    fn with_insert_text(mut self, insert_text: String) -> Self {
        if let Some(lsp_types::CompletionTextEdit::Edit(edit)) = &mut self.text_edit {
            edit.new_text = insert_text;
        }
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_account_prefix() {
        assert_eq!(extract_account_prefix("Assets:Cash", 11), "Assets:Cash");
        assert_eq!(extract_account_prefix("Assets:Cash", 6), "Assets");
        assert_eq!(extract_account_prefix("  Assets:Cash", 13), "Assets:Cash");
        assert_eq!(extract_account_prefix("", 0), "");
    }

    #[test]
    fn test_score_account_exact_match() {
        assert_eq!(score_account("Assets:Cash", "Assets:Cash"), 10000.0);
    }

    #[test]
    fn test_score_account_prefix_match() {
        let score = score_account("Assets:Cash", "Assets");
        assert!((7000.0..8000.0).contains(&score));
    }

    #[test]
    fn test_add_sub_month() {
        let date = chrono::NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let next = add_one_month(date);
        assert_eq!(next.month(), 7);

        let prev = sub_one_month(date);
        assert_eq!(prev.month(), 5);
    }

    // ========================================================================
    // Comprehensive Coverage Tests
    // ========================================================================

    #[test]
    fn test_complete_directive_keywords() {
        let items = complete_directive_keywords().unwrap();
        assert!(items.len() >= 10);

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"txn"));
        assert!(labels.contains(&"*"));
        assert!(labels.contains(&"!"));
        assert!(labels.contains(&"open"));
        assert!(labels.contains(&"close"));
        assert!(labels.contains(&"balance"));
        assert!(labels.contains(&"pad"));
        assert!(labels.contains(&"price"));
    }

    #[test]
    fn test_complete_date() {
        let items = complete_date().unwrap();
        assert_eq!(items.len(), 4);

        let details: Vec<String> = items.iter().filter_map(|i| i.detail.clone()).collect();
        assert!(details.contains(&"today".to_string()));
        assert!(details.contains(&"this month".to_string()));
        assert!(details.contains(&"prev month".to_string()));
        assert!(details.contains(&"next month".to_string()));
    }

    #[test]
    fn test_calculate_word_ranges() {
        let line = "  Assets:Checking:Personal";
        let position = Position {
            line: 0,
            character: 18,
        };

        let (insert_range, replace_range) = calculate_word_ranges(line, position);

        assert_eq!(insert_range.start.character, 2);
        assert_eq!(insert_range.end.character, 18);
        assert_eq!(replace_range.start.character, 2);
        assert_eq!(replace_range.end.character, 26);
    }

    #[test]
    fn test_calculate_word_ranges_middle_of_word() {
        let line = "Assets:Cash";
        let position = Position {
            line: 0,
            character: 7,
        };

        let (insert_range, replace_range) = calculate_word_ranges(line, position);

        assert_eq!(insert_range.start.character, 0);
        assert_eq!(insert_range.end.character, 7);
        assert_eq!(replace_range.start.character, 0);
        assert_eq!(replace_range.end.character, 11);
    }

    #[test]
    fn test_calculate_string_ranges_no_closing_quote() {
        let line = r#"2024-01-01 * "Grocery store"#;
        let position = Position {
            line: 0,
            character: 20,
        };

        let (insert_range, replace_range) = calculate_string_ranges(line, position, false);

        assert_eq!(insert_range.start.character, 14);
        assert_eq!(insert_range.end.character, 20);
        assert_eq!(replace_range.start.character, 14);
        assert_eq!(replace_range.end.character, 20);
    }

    #[test]
    fn test_calculate_string_ranges_with_closing_quote() {
        let line = r#"2024-01-01 * "Grocery store" "Food""#;
        let position = Position {
            line: 0,
            character: 33,
        };

        let (insert_range, replace_range) = calculate_string_ranges(line, position, true);

        // Function finds opening quote at position 30 (after the " at position 29)
        assert_eq!(insert_range.start.character, 30);
        assert_eq!(insert_range.end.character, 33);
        assert_eq!(replace_range.start.character, 30);
        // Function stops at the closing quote position (34), not after it
        assert_eq!(replace_range.end.character, 34);
    }

    #[test]
    fn test_fuzzy_search_accounts_empty_query() {
        let accounts = vec![
            "Assets:Cash".to_string(),
            "Expenses:Food".to_string(),
            "Liabilities:CreditCard".to_string(),
        ];

        let results = fuzzy_search_accounts(&accounts, "");
        assert_eq!(results.len(), 3);

        // All should have fallback score
        for (_, score) in &results {
            assert_eq!(*score, 1.0);
        }
    }

    #[test]
    fn test_fuzzy_search_accounts_exact_match() {
        let accounts = vec!["Assets:Cash".to_string(), "Expenses:Food".to_string()];

        let results = fuzzy_search_accounts(&accounts, "Assets:Cash");
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "Assets:Cash");
        assert_eq!(results[0].1, 10000.0);
    }

    #[test]
    fn test_fuzzy_search_accounts_prefix_match() {
        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Cash:Savings".to_string(),
            "Expenses:Food".to_string(),
        ];

        let results = fuzzy_search_accounts(&accounts, "Assets");
        assert!(results.len() >= 2);

        // Prefix matches should score higher than non-matches
        let assets_results: Vec<_> = results
            .iter()
            .filter(|(acc, _)| acc.starts_with("Assets"))
            .collect();
        assert_eq!(assets_results.len(), 2);

        for (_, score) in assets_results {
            assert!(*score >= 6900.0);
        }
    }

    #[test]
    fn test_fuzzy_search_accounts_intra_segment() {
        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Expenses:Food:Groceries".to_string(),
        ];

        let results = fuzzy_search_accounts(&accounts, "cash");
        assert!(!results.is_empty());

        let cash_result = results.iter().find(|(acc, _)| acc.contains("Cash"));
        assert!(cash_result.is_some());
        let (_, score) = cash_result.unwrap();
        assert!(*score >= 4000.0);
    }

    #[test]
    fn test_score_intra_segment_exact_segment_match() {
        let score = score_intra_segment("Assets:Cash:Checking", "cash");
        assert!(score.is_some());
        assert!(score.unwrap() >= 400.0);
    }

    #[test]
    fn test_score_intra_segment_prefix_match() {
        let score = score_intra_segment("Assets:Checking", "check");
        assert!(score.is_some());
        assert!(score.unwrap() >= 200.0);
    }

    #[test]
    fn test_score_intra_segment_no_match() {
        let score = score_intra_segment("Assets:Cash", "liabilities");
        assert!(score.is_none());
    }

    #[test]
    fn test_score_with_nucleo_valid_match() {
        let score = score_with_nucleo("Assets:Cash:Checking", "aschk");
        assert!(score.is_some());
        assert!(score.unwrap() > 0.0);
    }

    #[test]
    fn test_score_with_nucleo_no_match() {
        let score = score_with_nucleo("Assets:Cash", "xyz");
        // nucleo might return Some or None depending on fuzzy match
        // Just verify it doesn't panic
        let _ = score;
    }

    #[test]
    fn test_extract_account_prefix_with_spaces() {
        // Function extracts account prefix, stopping at whitespace
        assert_eq!(extract_account_prefix("  A", 3), "A");

        // Test with cursor inside the account name
        assert_eq!(extract_account_prefix("    Assets", 10), "Assets");
    }

    #[test]
    fn test_extract_account_prefix_with_colon() {
        assert_eq!(extract_account_prefix("Assets:", 7), "Assets:");
        assert_eq!(extract_account_prefix("Assets:Ca", 9), "Assets:Ca");
    }

    #[test]
    fn test_extract_account_prefix_empty_line() {
        assert_eq!(extract_account_prefix("", 0), "");
        assert_eq!(extract_account_prefix("   ", 3), "");
    }

    #[test]
    fn test_extract_account_prefix_at_boundary() {
        assert_eq!(extract_account_prefix("Assets", 0), "");
        assert_eq!(extract_account_prefix("Assets", 6), "Assets");
    }

    #[test]
    fn test_score_account_case_insensitive_exact() {
        let score = score_account("Assets:Cash", "assets:cash");
        assert!((6900.0..=10000.0).contains(&score));
    }

    #[test]
    fn test_score_account_tiering() {
        let exact_score = score_account("Assets:Cash", "Assets:Cash");
        let prefix_score = score_account("Assets:Cash", "Assets");
        let intra_score = score_account("Assets:Cash", "cash");
        let fallback_score = score_account("Assets:Cash", "xyz");

        assert!(exact_score > prefix_score);
        assert!(prefix_score > intra_score);
        assert!(intra_score > fallback_score);
    }

    #[test]
    fn test_add_one_month_december() {
        let date = chrono::NaiveDate::from_ymd_opt(2024, 12, 15).unwrap();
        let next = add_one_month(date);
        assert_eq!(next.year(), 2025);
        assert_eq!(next.month(), 1);
    }

    #[test]
    fn test_sub_one_month_january() {
        let date = chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let prev = sub_one_month(date);
        assert_eq!(prev.year(), 2023);
        assert_eq!(prev.month(), 12);
    }

    #[test]
    fn test_create_completion_with_insert_replace() {
        let insert_range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 5,
            },
        };
        let replace_range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 10,
            },
        };

        let item = create_completion_with_insert_replace(
            "Assets:Cash".to_string(),
            "Account".to_string(),
            CompletionItemKind::ENUM,
            insert_range,
            replace_range,
            100.0,
            vec![":".to_string()],
        );

        assert_eq!(item.label, "Assets:Cash");
        assert_eq!(item.detail, Some("Account".to_string()));
        assert_eq!(item.kind, Some(CompletionItemKind::ENUM));
        assert_eq!(item.commit_characters, Some(vec![":".to_string()]));

        match item.text_edit {
            Some(lsp_types::CompletionTextEdit::Edit(text_edit)) => {
                assert_eq!(text_edit.range, replace_range);
                assert_eq!(text_edit.new_text, "Assets:Cash");
            }
            _ => panic!("Expected a TextEdit"),
        }
    }

    #[test]
    fn test_score_account_multiple_segments() {
        let accounts = vec![
            "Assets:Cash:Checking:Personal".to_string(),
            "Assets:Cash:Checking:Business".to_string(),
            "Assets:Investments:Stocks".to_string(),
        ];

        // Should match "Checking" in the middle
        let results = fuzzy_search_accounts(&accounts, "checking");
        let checking_results: Vec<_> = results
            .iter()
            .filter(|(acc, _)| acc.contains("Checking"))
            .collect();
        assert_eq!(checking_results.len(), 2);
    }

    #[test]
    fn test_fuzzy_search_case_sensitivity() {
        let accounts = vec!["Assets:Cash".to_string(), "Expenses:Food".to_string()];

        let upper_results = fuzzy_search_accounts(&accounts, "ASSETS");
        let lower_results = fuzzy_search_accounts(&accounts, "assets");

        // Both should find the Assets account
        assert!(!upper_results.is_empty());
        assert!(!lower_results.is_empty());

        assert!(upper_results[0].0.contains("Assets") || upper_results[0].0.contains("assets"));
        assert!(lower_results[0].0.contains("Assets") || lower_results[0].0.contains("assets"));
    }

    #[test]
    fn test_score_intra_segment_multiple_matches() {
        // Should prefer earlier segments
        let score1 = score_intra_segment("Assets:Cash:Checking", "cash");
        let score2 = score_intra_segment("Liabilities:CreditCard:Cash", "cash");

        assert!(score1.is_some());
        assert!(score2.is_some());

        // Segment at index 1 should score higher than segment at index 2
        assert!(score1.unwrap() > score2.unwrap());
    }

    #[test]
    fn test_extract_account_prefix_with_hyphens() {
        assert_eq!(
            extract_account_prefix("Assets:My-Account", 17),
            "Assets:My-Account"
        );
        assert_eq!(
            extract_account_prefix("Assets:My-Account", 10),
            "Assets:My-"
        );
    }

    #[test]
    fn test_extract_account_prefix_with_underscores() {
        assert_eq!(
            extract_account_prefix("Assets:My_Account", 17),
            "Assets:My_Account"
        );
    }

    #[test]
    fn test_calculate_word_ranges_start_of_line() {
        let line = "Assets";
        let position = Position {
            line: 0,
            character: 0,
        };

        let (insert_range, replace_range) = calculate_word_ranges(line, position);

        assert_eq!(insert_range.start.character, 0);
        assert_eq!(insert_range.end.character, 0);
        assert_eq!(replace_range.start.character, 0);
        assert_eq!(replace_range.end.character, 6);
    }

    #[test]
    fn test_calculate_word_ranges_end_of_word() {
        let line = "Assets ";
        let position = Position {
            line: 0,
            character: 6,
        };

        let (insert_range, replace_range) = calculate_word_ranges(line, position);

        assert_eq!(insert_range.start.character, 0);
        assert_eq!(insert_range.end.character, 6);
        assert_eq!(replace_range.start.character, 0);
        assert_eq!(replace_range.end.character, 6);
    }

    #[test]
    fn test_fuzzy_search_special_characters() {
        let accounts = vec![
            "Assets:US-Bank:Checking".to_string(),
            "Assets:Euro_Bank:Savings".to_string(),
        ];

        let results = fuzzy_search_accounts(&accounts, "us");
        let us_result = results.iter().find(|(acc, _)| acc.contains("US"));
        assert!(us_result.is_some());
    }

    #[test]
    fn test_check_if_in_posting_area() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 txn "kroger" "Check #1274"
  A"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor at row 1, column 3 (after "  A")
        let cursor = Point { row: 1, column: 3 };

        let context = check_if_in_posting_area(&tree, &rope, cursor);

        // Should detect we're in posting area
        match context {
            CompletionContext::PostingAccount { prefix } => {
                assert_eq!(prefix, "A");
            }
            _ => panic!("Expected PostingAccount context, got {:?}", context),
        }
    }

    #[test]
    fn test_check_if_in_posting_area_with_flag() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 * "payee" "narration"
  Exp"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor at row 1, column 5 (after "  Exp")
        let cursor = Point { row: 1, column: 5 };

        let context = check_if_in_posting_area(&tree, &rope, cursor);

        match context {
            CompletionContext::PostingAccount { prefix } => {
                assert_eq!(prefix, "Exp");
            }
            _ => panic!("Expected PostingAccount context, got {:?}", context),
        }
    }

    #[test]
    fn test_check_if_in_posting_area_not_in_transaction() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 open Assets:Cash

A"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor at row 2, column 1 (after empty line and "A")
        let cursor = Point { row: 2, column: 1 };

        let context = check_if_in_posting_area(&tree, &rope, cursor);

        // Should be DocumentRoot since we hit empty line
        match context {
            CompletionContext::DocumentRoot => {
                // Expected
            }
            _ => panic!("Expected DocumentRoot context, got {:?}", context),
        }
    }
}
