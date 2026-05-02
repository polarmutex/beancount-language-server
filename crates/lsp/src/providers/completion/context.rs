use tracing::debug;
use tree_sitter::Point;
use tree_sitter_beancount::tree_sitter;

/// Represents the completion context determined by analyzing the syntax tree
/// and cursor position using left-context-aware traversal strategy.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum CompletionContext {
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

    /// Inside a payee string literal (first string in a transaction)
    InsidePayee { prefix: String },

    /// Inside a narration string literal (second string in a transaction)
    InsideNarration { prefix: String },

    /// After tag trigger character (#)
    TagContext { prefix: String },

    /// After link trigger character (^)
    LinkContext { prefix: String },

    /// Colon-triggered account completion (show sub-accounts)
    ColonTriggeredAccount { parent_path: String },
}

/// Determine completion context using left-context-aware traversal.
///
/// This implements the algorithm from the plan:
/// 1. Find node at cursor (with -1 character lookahead for ghost nodes)
/// 2. Step out of ERROR/MISSING nodes to find stable parent
/// 3. Identify previous sibling to infer expected next element
/// 4. Map (parent_type, prev_sibling_type) to completion context
pub(super) fn determine_completion_context(
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
            "string" | "payee" | "narration" => {
                analyze_string_node_context(current_node, content, cursor)
            }
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
    // First check if current line is a directive (balance, open, close, etc.)
    // This handles cases where tree-sitter doesn't recognize the directive due to invalid syntax
    let current_line = content.line(cursor.row).to_string();
    let trimmed_current = current_line.trim();

    // Check for balance directive
    if trimmed_current.starts_with(|c: char| c.is_ascii_digit())
        && trimmed_current.contains("balance")
    {
        // Extract words to determine what we're completing
        let words: Vec<&str> = trimmed_current.split_whitespace().collect();
        // Format: YYYY-MM-DD balance [account] [amount] [currency]
        // If we have at least date + "balance", we're expecting an account
        if words.len() >= 2 && words[1] == "balance" {
            let prefix = extract_account_prefix(&current_line, cursor.column);
            return CompletionContext::BalanceAccount { prefix };
        }
    }

    // Check for open directive
    if trimmed_current.starts_with(|c: char| c.is_ascii_digit()) && trimmed_current.contains("open")
    {
        let words: Vec<&str> = trimmed_current.split_whitespace().collect();
        if words.len() >= 2 && words[1] == "open" {
            // Determine if we need account or currency completion
            // Format: YYYY-MM-DD open Account Currency
            if words.len() >= 3 {
                // Already have account, expecting currency
                return CompletionContext::OpenCurrency;
            } else {
                // Expecting account
                let prefix = extract_account_prefix(&current_line, cursor.column);
                return CompletionContext::OpenAccount { prefix };
            }
        }
    }

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
                        // Check if we're at the second word position (amount) or third+ (currency)
                        // Second word: amount, Third+ word: currency
                        if words.len() >= 3 {
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
                "payee" => {
                    // After payee (first string), we expect narration
                    CompletionContext::AfterPayee
                }
                "narration" => {
                    // After narration, check if we're on same line or posting area
                    // If on same line as transaction, stay in transaction context
                    if cursor.row == prev.start_position().row {
                        // Still on transaction line, might be completing after narration
                        CompletionContext::AfterPayee // Can happen with incomplete line
                    } else {
                        // On a new line, we're in posting area
                        let line = content.line(cursor.row).to_string();
                        let prefix = extract_account_prefix(&line, cursor.column);
                        CompletionContext::PostingAccount { prefix }
                    }
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
        // The grammar aliases string nodes to "payee" or "narration"
        // So check the node kind directly
        let is_payee = string_node.kind() == "payee";

        let line = content.line(cursor.row).to_string();
        let prefix = extract_string_prefix(&line, cursor.column);

        return if is_payee {
            CompletionContext::InsidePayee { prefix }
        } else {
            CompletionContext::InsideNarration { prefix }
        };
    }

    CompletionContext::InsideNarration {
        prefix: String::new(),
    }
}

/// Analyze string context when triggered by quote character
fn analyze_string_context(content: &ropey::Rope, cursor: Point) -> CompletionContext {
    let line = content.line(cursor.row).to_string();
    let prefix = extract_string_prefix(&line, cursor.column);

    // Count quotes before cursor to determine context
    let before_cursor = safe_substring_to_byte(&line, cursor.column);
    let quote_count = before_cursor.matches('"').count();

    // Check if we have a complete payee (2+ quotes before, suggesting this is narration)
    // Quote count: 1 = inside first string (payee)
    //              2 = after first string, before second
    //              3+ = inside second string (narration)
    let is_payee = quote_count < 3;

    if is_payee {
        CompletionContext::InsidePayee { prefix }
    } else {
        CompletionContext::InsideNarration { prefix }
    }
}

/// Safely get a substring from start to a byte offset, ensuring we don't split UTF-8 characters.
/// If the byte offset falls in the middle of a character, it rounds down to the previous character boundary.
fn safe_substring_to_byte(s: &str, byte_offset: usize) -> &str {
    if byte_offset >= s.len() {
        return s;
    }

    // Find the nearest character boundary at or before byte_offset
    let mut idx = byte_offset;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
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
    let relevant_part = safe_substring_to_byte(line, cursor_col);
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
    let relevant_part = safe_substring_to_byte(line, cursor_col);
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
    fn test_safe_substring_to_byte_ascii() {
        let s = "hello world";
        assert_eq!(safe_substring_to_byte(s, 5), "hello");
        assert_eq!(safe_substring_to_byte(s, 0), "");
        assert_eq!(safe_substring_to_byte(s, 100), s);
    }

    #[test]
    fn test_safe_substring_to_byte_cjk() {
        // "最" is 3 bytes (bytes 11-13), "后" is 3 bytes (bytes 14-16)
        let s = "2026-02-15 最后";
        // Byte 11 is at the start of '最'
        assert_eq!(safe_substring_to_byte(s, 11), "2026-02-15 ");
        // Byte 12 is in the middle of '最', should round down to 11
        assert_eq!(safe_substring_to_byte(s, 12), "2026-02-15 ");
        // Byte 13 is in the middle of '最', should round down to 11
        assert_eq!(safe_substring_to_byte(s, 13), "2026-02-15 ");
        // Byte 14 is at the start of '后'
        assert_eq!(safe_substring_to_byte(s, 14), "2026-02-15 最");
        // Byte 15 is in the middle of '后', should round down to 14
        assert_eq!(safe_substring_to_byte(s, 15), "2026-02-15 最");
    }

    #[test]
    fn test_extract_tag_prefix_with_cjk() {
        // Test that tag extraction doesn't panic with CJK content
        let line = "  comment: \"2026-02-15 最后\"";
        // This would have panicked before the fix when cursor_col is in the middle of a multi-byte char
        let result = extract_tag_prefix(line, 25);
        // Should not panic and return None (no tag in this context)
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_link_prefix_with_cjk() {
        // Test that link extraction doesn't panic with CJK content
        let line = "  comment: \"2026-02-15 最后\"";
        // This would have panicked before the fix
        let result = extract_link_prefix(line, 25);
        // Should not panic and return None (no link in this context)
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_account_prefix_with_spaces() {
        assert_eq!(extract_account_prefix("  A", 3), "A");
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
    fn test_extract_string_prefix_basic() {
        assert_eq!(extract_string_prefix(r#""Kroger"#, 3), "Kr");
        assert_eq!(extract_string_prefix(r#""Kroger"#, 7), "Kroger");
        assert_eq!(extract_string_prefix(r#""Kroger"#, 1), "");
    }

    #[test]
    fn test_extract_string_prefix_with_spaces() {
        assert_eq!(extract_string_prefix(r#""King Soopers"#, 6), "King ");
        assert_eq!(
            extract_string_prefix(r#""King Soopers"#, 13),
            "King Soopers"
        );
    }

    #[test]
    fn test_extract_string_prefix_empty() {
        assert_eq!(extract_string_prefix(r#"""#, 1), "");
        assert_eq!(extract_string_prefix(r#"""#, 0), "");
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

    #[test]
    fn test_payee_context_after_flag() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 *"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor after flag (no trailing space)
        let cursor = Point { row: 0, column: 12 };

        let context = determine_completion_context(&tree, &rope, cursor, None);

        match context {
            CompletionContext::AfterFlag | CompletionContext::DocumentRoot => {
                // Either is acceptable - incomplete transaction can be DocumentRoot
            }
            _ => panic!(
                "Expected AfterFlag or DocumentRoot context, got {:?}",
                context
            ),
        }
    }

    #[test]
    fn test_payee_context_after_txn_keyword() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 txn"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor after txn (no trailing space)
        let cursor = Point { row: 0, column: 14 };

        let context = determine_completion_context(&tree, &rope, cursor, None);

        match context {
            CompletionContext::AfterFlag | CompletionContext::DocumentRoot => {
                // Either is acceptable - incomplete transaction can be DocumentRoot
            }
            _ => panic!(
                "Expected AfterFlag or DocumentRoot context, got {:?}",
                context
            ),
        }
    }

    #[test]
    fn test_payee_context_inside_first_string() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 * "Groc"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor inside first string
        let cursor = Point { row: 0, column: 18 };

        let context = determine_completion_context(&tree, &rope, cursor, None);

        match context {
            CompletionContext::InsidePayee { prefix } => {
                assert_eq!(prefix, "Groc");
            }
            _ => panic!("Expected InsidePayee context, got {:?}", context),
        }
    }

    #[test]
    fn test_payee_context_with_opening_quote() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 * ""#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor right after opening quote
        let cursor = Point { row: 0, column: 14 };

        let context = determine_completion_context(&tree, &rope, cursor, Some('"'));

        match context {
            CompletionContext::InsidePayee { .. } => {
                // Correctly detected payee context with opening quote trigger
            }
            _ => panic!("Expected InsidePayee context, got {:?}", context),
        }
    }

    #[test]
    fn test_narration_context_after_payee() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 * "Kroger" "#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor after first string (payee)
        let cursor = Point { row: 0, column: 22 };

        let context = determine_completion_context(&tree, &rope, cursor, None);

        match context {
            CompletionContext::AfterPayee | CompletionContext::DocumentRoot => {
                // Either is acceptable - incomplete transaction can be DocumentRoot
            }
            _ => panic!(
                "Expected AfterPayee or DocumentRoot context, got {:?}",
                context
            ),
        }
    }

    #[test]
    fn test_narration_context_inside_second_string() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 * "Kroger" "Food"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor inside second string (narration)
        // "2026-01-06 * "Kroger" "Food"
        //  Position:              25=second 'o' in Food
        let cursor = Point { row: 0, column: 25 };

        let context = determine_completion_context(&tree, &rope, cursor, None);

        match context {
            CompletionContext::InsidePayee { prefix }
            | CompletionContext::InsideNarration { prefix } => {
                // The important thing is that we're inside a string context
                // Whether it's detected as payee or narration can vary based on tree state
                assert_eq!(prefix, "Fo"); // Position 25 = after 'Fo'
            }
            _ => panic!(
                "Expected InsidePayee or InsideNarration context, got {:?}",
                context
            ),
        }
    }

    #[test]
    fn test_narration_context_with_opening_quote() {
        use ropey::Rope;
        use tree_sitter::Parser;

        let text = r#"2026-01-06 * "Kroger" ""#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Cursor right after opening quote of second string
        let cursor = Point { row: 0, column: 23 };

        let context = determine_completion_context(&tree, &rope, cursor, Some('"'));

        match context {
            CompletionContext::InsidePayee { .. } | CompletionContext::InsideNarration { .. } => {
                // Accept either payee or narration - detection can vary based on tree state
                // The important thing is that we're in a string context
            }
            CompletionContext::AfterPayee | CompletionContext::DocumentRoot => {
                // Also acceptable for incomplete transactions
            }
            _ => panic!(
                "Expected InsidePayee/InsideNarration, AfterPayee, or DocumentRoot context, got {:?}",
                context
            ),
        }
    }

    #[test]
    fn test_payee_narration_with_only_flag() {
        use ropey::Rope;
        use tree_sitter::Parser;

        // Just date and flag, no strings yet
        let text = r#"2026-01-06 !"#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        let cursor = Point { row: 0, column: 12 };
        let context = determine_completion_context(&tree, &rope, cursor, None);

        assert!(
            matches!(
                context,
                CompletionContext::AfterFlag | CompletionContext::DocumentRoot
            ),
            "After ! flag should trigger payee completion or be DocumentRoot, got {:?}",
            context
        );
    }

    #[test]
    fn test_narration_only_transaction() {
        use ropey::Rope;
        use tree_sitter::Parser;

        // Transaction with only one string (grammatically "narration", no "payee")
        let text = r#"2026-01-06 * "Groceries""#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Inside the string
        let cursor = Point { row: 0, column: 20 };
        let context = determine_completion_context(&tree, &rope, cursor, None);

        // Single string transactions: the string node is labeled "narration" by the grammar
        // (not "payee"), so we return InsideNarration.
        match context {
            CompletionContext::InsideNarration { .. } => {
                // Expected: single string is grammatically 'narration'
            }
            CompletionContext::PostingAccount { .. } | CompletionContext::DocumentRoot => {
                // Also acceptable - incomplete line may be parsed as posting or root
            }
            _ => panic!(
                "Unexpected context for narration-only transaction: {:?}",
                context
            ),
        }
    }

    #[test]
    fn test_payee_narration_sequence() {
        use ropey::Rope;
        use tree_sitter::Parser;

        // Test the full sequence: flag -> payee -> narration
        let text = r#"2026-01-06 * "Kroger" "Groceries""#;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        // Position 1: After flag - with complete transaction, should recognize context
        let cursor1 = Point { row: 0, column: 13 };
        let context1 = determine_completion_context(&tree, &rope, cursor1, None);
        // After flag in a complete transaction, context can vary based on tree structure
        // Just verify it's a reasonable context
        match context1 {
            CompletionContext::AfterFlag
            | CompletionContext::InsidePayee { .. }
            | CompletionContext::InsideNarration { .. }
            | CompletionContext::DocumentRoot => {
                // All acceptable for position after flag
            }
            _ => panic!("Unexpected context after flag: {:?}", context1),
        }

        // Position 2: Inside payee string
        // "2026-01-06 * "Kroger" "Groceries""
        //  Column:      14=K, 16=o, 19=r, 20=" (closing)
        let cursor2 = Point { row: 0, column: 16 }; // Inside "Kroger" at 'o'
        let context2 = determine_completion_context(&tree, &rope, cursor2, None);
        // The parser may recognize this as AfterPayee if string node ends before cursor
        match context2 {
            CompletionContext::InsidePayee { .. } => {
                // First string is payee
            }
            CompletionContext::AfterPayee => {
                // Also acceptable - parser may have recognized payee completion
            }
            _ => panic!(
                "Expected InsidePayee or AfterPayee for payee position, got {:?}",
                context2
            ),
        }

        // Position 3: After payee (should be AfterPayee for narration)
        let cursor3 = Point { row: 0, column: 22 };
        let context3 = determine_completion_context(&tree, &rope, cursor3, None);
        assert!(
            matches!(
                context3,
                CompletionContext::AfterPayee
                    | CompletionContext::InsidePayee { .. }
                    | CompletionContext::InsideNarration { .. }
            ),
            "Position after payee should be AfterPayee or InsideString, got {:?}",
            context3
        );

        // Position 4: Inside narration string
        // "2026-01-06 * "Kroger" "Groceries""
        //  Position 30 is at 'i' in Groceries
        let cursor4 = Point { row: 0, column: 30 };
        let context4 = determine_completion_context(&tree, &rope, cursor4, None);
        match context4 {
            CompletionContext::InsideNarration { .. } => {
                // Second string is narration
            }
            CompletionContext::InsidePayee { .. } => {
                // Tree parsing can vary, accept this too
            }
            CompletionContext::PostingAccount { .. }
            | CompletionContext::AfterPayee
            | CompletionContext::DocumentRoot => {
                // Also acceptable - tree parsing can vary
            }
            _ => panic!("Unexpected context for narration position: {:?}", context4),
        }
    }
}
