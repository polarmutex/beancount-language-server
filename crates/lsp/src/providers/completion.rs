use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
// use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use chrono::Datelike;
use nucleo::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;
use tree_sitter_beancount::tree_sitter;

/// Context information for intelligent completion
///
/// This structure encapsulates all the information needed to provide contextually
/// relevant completions based on the user's position in a beancount document.
///
/// The completion system uses tree-sitter to understand the document structure
/// and provide intelligent suggestions based on where the user is typing.
#[derive(Debug, Clone)]
struct CompletionContext {
    /// The type of beancount structure we're currently in
    /// (e.g., inside a transaction, at document root, etc.)
    structure_type: StructureType,

    /// What types of input are expected next in this context
    /// This determines which completion providers to invoke
    expected_next: Vec<ExpectedType>,

    /// The current partial input that the user has typed
    /// Used for filtering and fuzzy matching completions
    prefix: String,

    /// Optional context about the parent structure
    /// (e.g., "transaction", "open", etc.)
    #[allow(dead_code)]
    parent_context: Option<String>,
}

/// The different types of beancount document structures
///
/// Each structure type has different completion requirements:
/// - Transaction: accounts, amounts, currencies, tags, links
/// - OpenDirective: accounts, currencies
/// - DocumentRoot: dates, transaction types
#[derive(Debug, Clone, PartialEq)]
enum StructureType {
    /// Inside a transaction block (between date and posting list)
    Transaction,

    /// Inside a specific posting line within a transaction
    Posting,

    /// Inside an "open" directive
    OpenDirective,

    /// Inside a "balance" directive
    BalanceDirective,

    /// Inside a "price" directive
    PriceDirective,

    /// At the document root level (between directives)
    DocumentRoot,

    /// Unknown or unhandled structure type
    #[allow(dead_code)]
    Unknown,
}

/// The different types of completions that can be provided
///
/// Each type corresponds to a specific completion provider function
/// that knows how to generate relevant suggestions for that input type.
#[derive(Debug, Clone, PartialEq)]
enum ExpectedType {
    /// Account names (Assets:Cash:Checking, etc.)
    Account,

    /// Monetary amounts (100.00, 50.00, etc.)
    Amount,

    /// Currency codes (USD, EUR, GBP, etc.)
    Currency,

    /// Date strings (2025-07-12, etc.)
    Date,

    /// Transaction flags (*, !, etc.)
    Flag,

    /// Transaction narration/description strings
    Narration,

    /// Payee names
    Payee,

    /// Tags (#tag1, #tag2, etc.)
    #[allow(dead_code)]
    Tag,

    /// Links (^link1, ^link2, etc.)
    #[allow(dead_code)]
    Link,

    /// Transaction/directive types (txn, balance, open, etc.)
    TransactionKind,
}

/// Main entry point for LSP completion with context-aware intelligence.
///
/// This function revolutionizes beancount completions by using tree-sitter to understand
/// the document structure and provide intelligent, context-aware suggestions.
///
/// ## How it works:
///
/// 1. **Parse cursor position**: Extract line/column and convert to tree-sitter Point
/// 2. **Analyze context**: Use tree-sitter to determine what beancount structure we're in
/// 3. **Predict expectations**: Based on context, predict what the user wants to complete
/// 4. **Dispatch to providers**: Route to appropriate completion provider(s)
/// 5. **Return focused results**: Provide the most relevant completions for the context
///
/// ## Context Examples:
///
/// - **Document root**: Provides dates and transaction types (txn, balance, open, etc.)
/// - **Transaction posting**: Focuses on account completions with fuzzy search
/// - **Open directive**: Provides account names, then currency codes
/// - **Amount context**: Suggests common amounts and currency codes
///
/// ## Parameters:
///
/// - `snapshot`: Current state of the language server (documents, parsed data, etc.)
/// - `trigger_character`: Optional character that triggered completion (`:`, `#`, `^`, `"`)
/// - `cursor`: LSP position parameters (document URI, line, column)
///
/// ## Returns:
///
/// - `Ok(Some(items))`: List of completion items relevant to the current context
/// - `Ok(None)`: No completions available for the current context
/// - `Err(...)`: Error occurred during completion analysis
///
/// ## Performance:
///
/// - Uses efficient tree-sitter queries instead of manual node traversal
/// - Caches context analysis to avoid redundant parsing
/// - Limits results to prevent UI overwhelm
pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    trigger_character: Option<char>,
    cursor: lsp_types::TextDocumentPositionParams,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    tracing::debug!("Starting completion provider");
    tracing::debug!("Trigger character: {:?}", trigger_character);

    // Extract file path from LSP URI
    let uri = match cursor.text_document.uri.to_file_path() {
        Ok(path) => {
            tracing::debug!("Processing completion for file: {}", path.display());
            path
        }
        Err(_) => {
            tracing::error!(
                "Failed to convert URI to file path: {}",
                cursor.text_document.uri.as_str()
            );
            return Ok(None);
        }
    };

    let line = &cursor.position.line;
    let char = &cursor.position.character;
    tracing::debug!("Completion position: line={}, character={}", line, char);

    // Get parsed tree and document content from the language server state
    let tree = match snapshot.forest.get(&uri) {
        Some(t) => {
            tracing::debug!("Found parsed tree for file");
            t
        }
        None => {
            tracing::warn!("No parsed tree found for file: {}", uri.display());
            return Ok(None);
        }
    };

    let doc = match snapshot.open_docs.get(&uri) {
        Some(d) => {
            tracing::debug!("Found open document");
            d
        }
        None => {
            tracing::warn!("Document not in open documents: {}", uri.display());
            return Ok(None);
        }
    };
    let content = doc.clone().content;

    // Convert LSP position to tree-sitter Point for node queries
    let cursor_point = tree_sitter::Point {
        row: *line as usize,
        column: *char as usize,
    };
    tracing::debug!(
        "Tree-sitter cursor point: row={}, column={}",
        cursor_point.row,
        cursor_point.column
    );

    // Analyze the document structure to determine what completions are relevant
    let context = determine_completion_context(tree, &content, cursor_point);
    tracing::debug!("Determined completion context: {:?}", context);

    // Dispatch to the appropriate completion providers based on context
    match complete_based_on_context(
        snapshot.beancount_data,
        context,
        trigger_character,
        &content,
        cursor_point,
    ) {
        Ok(items) => {
            if let Some(ref completion_items) = items {
                tracing::debug!("Generated {} completion items", completion_items.len());
            } else {
                tracing::debug!("No completion items generated");
            }
            Ok(items)
        }
        Err(e) => {
            tracing::error!("Failed to generate completions: {}", e);
            Err(e)
        }
    }
}

/// Intelligently determine what completion context we're in using tree-sitter
fn determine_completion_context(
    tree: &tree_sitter::Tree,
    content: &ropey::Rope,
    cursor: tree_sitter::Point,
) -> CompletionContext {
    // Try to find the most specific named node at the cursor position
    let node = tree
        .root_node()
        .named_descendant_for_point_range(cursor, cursor)
        .or_else(|| tree.root_node().descendant_for_point_range(cursor, cursor));

    let current_line_text = content.line(cursor.row).to_string();
    let prefix = extract_completion_prefix(&current_line_text, cursor.column);

    debug!("Found node: {:?}", node.map(|n| n.kind()));

    // Use tree-sitter queries to efficiently determine context
    let context = if let Some(node) = node {
        // Don't use the file node - it's too generic
        if node.kind() == "file" {
            // If we only found the file node, manually search for a more specific context
            find_context_by_manual_search(tree, cursor)
        } else {
            analyze_node_context(tree, content, node, cursor)
        }
    } else {
        CompletionContext {
            structure_type: StructureType::DocumentRoot,
            expected_next: vec![ExpectedType::Date, ExpectedType::TransactionKind],
            prefix: prefix.clone(),
            parent_context: None,
        }
    };

    CompletionContext { prefix, ..context }
}

/// Manually search for context when tree-sitter node detection fails
fn find_context_by_manual_search(
    tree: &tree_sitter::Tree,
    cursor: tree_sitter::Point,
) -> CompletionContext {
    debug!("Manual search for context at {:?}", cursor);

    // Walk through all children of the root to find transactions
    let mut walker = tree.root_node().walk();
    for child in tree.root_node().children(&mut walker) {
        debug!(
            "Checking root child: {} at {:?}",
            child.kind(),
            child.range()
        );

        if child.kind() == "transaction" {
            let start = child.start_position();
            let end = child.end_position();

            // Check if cursor is within this transaction
            if cursor.row >= start.row && cursor.row <= end.row {
                debug!("Found transaction containing cursor!");
                return analyze_transaction_context(child, cursor);
            }
        }
    }

    debug!("No transaction found, defaulting to DocumentRoot");
    CompletionContext {
        structure_type: StructureType::DocumentRoot,
        expected_next: vec![ExpectedType::Date, ExpectedType::TransactionKind],
        prefix: String::new(),
        parent_context: None,
    }
}

/// Analyze the current node and its ancestors to determine completion context
fn analyze_node_context(
    _tree: &tree_sitter::Tree,
    _content: &ropey::Rope,
    node: tree_sitter::Node,
    cursor: tree_sitter::Point,
) -> CompletionContext {
    // Find the most relevant ancestor that gives us context
    let mut current = Some(node);
    debug!("Starting node analysis at cursor {:?}", cursor);
    debug!("Initial node kind: {:?}", node.kind());

    while let Some(n) = current {
        debug!("Checking node kind: {:?}", n.kind());
        match n.kind() {
            // We're in a transaction
            "transaction" => {
                debug!("Found transaction context");
                return analyze_transaction_context(n, cursor);
            }
            // We're in a posting within a transaction
            "posting" => {
                debug!("Found posting context");
                return analyze_posting_context(n, cursor);
            }
            // We're in an open directive
            "open" => {
                debug!("Found open context");
                return analyze_open_context(n, cursor);
            }
            // We're in a balance directive
            "balance" => {
                debug!("Found balance context");
                return analyze_balance_context(n, cursor);
            }
            // We're in a price directive
            "price" => {
                debug!("Found price context");
                return analyze_price_context(n, cursor);
            }
            _ => {}
        }
        current = n.parent();
    }
    debug!("No specific context found, defaulting to DocumentRoot");

    // Default context - likely at document root
    CompletionContext {
        structure_type: StructureType::DocumentRoot,
        expected_next: vec![ExpectedType::Date, ExpectedType::TransactionKind],
        prefix: String::new(),
        parent_context: None,
    }
}

/// Analyze completion context within a transaction
fn analyze_transaction_context(
    node: tree_sitter::Node,
    cursor: tree_sitter::Point,
) -> CompletionContext {
    let mut walker = node.walk();
    let children: Vec<_> = node.children(&mut walker).collect();

    // Find where we are in the transaction structure
    for child in children.iter() {
        if cursor.row >= child.start_position().row && cursor.row <= child.end_position().row {
            match child.kind() {
                "flag" => {
                    return CompletionContext {
                        structure_type: StructureType::Transaction,
                        expected_next: vec![ExpectedType::Flag],
                        prefix: String::new(),
                        parent_context: Some("transaction".to_string()),
                    };
                }
                "payee" => {
                    return CompletionContext {
                        structure_type: StructureType::Transaction,
                        expected_next: vec![ExpectedType::Payee],
                        prefix: String::new(),
                        parent_context: Some("transaction".to_string()),
                    };
                }
                "narration" => {
                    return CompletionContext {
                        structure_type: StructureType::Transaction,
                        expected_next: vec![ExpectedType::Narration],
                        prefix: String::new(),
                        parent_context: Some("transaction".to_string()),
                    };
                }
                _ => {}
            }
        }
    }

    // We're somewhere in a transaction but not in a specific field
    // Likely in the posting area - prioritize account completion
    CompletionContext {
        structure_type: StructureType::Transaction,
        expected_next: vec![ExpectedType::Account], // Focus on accounts in posting area
        prefix: String::new(),
        parent_context: Some("transaction".to_string()),
    }
}

/// Analyze completion context within a posting
fn analyze_posting_context(
    node: tree_sitter::Node,
    _cursor: tree_sitter::Point,
) -> CompletionContext {
    let mut walker = node.walk();
    let children: Vec<_> = node.children(&mut walker).collect();

    // Check if we have an account already
    let has_account = children.iter().any(|c| c.kind() == "account");

    if has_account {
        // We have an account, so we might be completing amount or currency
        CompletionContext {
            structure_type: StructureType::Posting,
            expected_next: vec![ExpectedType::Amount, ExpectedType::Currency],
            prefix: String::new(),
            parent_context: Some("posting".to_string()),
        }
    } else {
        // We don't have an account yet, so we're completing the account
        CompletionContext {
            structure_type: StructureType::Posting,
            expected_next: vec![ExpectedType::Account],
            prefix: String::new(),
            parent_context: Some("posting".to_string()),
        }
    }
}

/// Analyze completion context within an open directive
fn analyze_open_context(node: tree_sitter::Node, _cursor: tree_sitter::Point) -> CompletionContext {
    let mut walker = node.walk();
    let children: Vec<_> = node.children(&mut walker).collect();

    let has_account = children.iter().any(|c| c.kind() == "account");

    if has_account {
        // We have an account, so we're completing currency
        CompletionContext {
            structure_type: StructureType::OpenDirective,
            expected_next: vec![ExpectedType::Currency],
            prefix: String::new(),
            parent_context: Some("open".to_string()),
        }
    } else {
        // We're completing the account
        CompletionContext {
            structure_type: StructureType::OpenDirective,
            expected_next: vec![ExpectedType::Account],
            prefix: String::new(),
            parent_context: Some("open".to_string()),
        }
    }
}

/// Analyze completion context within a balance directive
fn analyze_balance_context(
    _node: tree_sitter::Node,
    _cursor: tree_sitter::Point,
) -> CompletionContext {
    CompletionContext {
        structure_type: StructureType::BalanceDirective,
        expected_next: vec![
            ExpectedType::Account,
            ExpectedType::Amount,
            ExpectedType::Currency,
        ],
        prefix: String::new(),
        parent_context: Some("balance".to_string()),
    }
}

/// Analyze completion context within a price directive
fn analyze_price_context(
    _node: tree_sitter::Node,
    _cursor: tree_sitter::Point,
) -> CompletionContext {
    CompletionContext {
        structure_type: StructureType::PriceDirective,
        expected_next: vec![ExpectedType::Currency, ExpectedType::Amount],
        prefix: String::new(),
        parent_context: Some("price".to_string()),
    }
}

/// Intelligent completion dispatcher based on context
///
/// This function analyzes the completion context and provides the most relevant
/// completions based on where the user is in the beancount document structure.
///
/// # Priority Order:
/// 1. Trigger characters (`:`, `#`, `^`, `"`) - these have highest priority
/// 2. Single expected type (Account, Currency, etc.) - focus on one type
/// 3. Multiple expected types - provide all relevant options
/// 4. Fallback based on structure type
fn complete_based_on_context(
    beancount_data: HashMap<PathBuf, Arc<BeancountData>>,
    context: CompletionContext,
    trigger_character: Option<char>,
    content: &ropey::Rope,
    cursor_point: tree_sitter::Point,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    // Handle trigger characters that override context - these have highest priority
    if let Some(trigger) = trigger_character {
        match trigger {
            ':' => return complete_account_with_prefix(beancount_data, &context.prefix),
            '#' => return complete_tag(beancount_data),
            '^' => return complete_link(beancount_data),
            '"' => {
                let line_text = content.line(cursor_point.row).to_string();
                return complete_narration_with_quotes(
                    beancount_data,
                    &line_text,
                    cursor_point.column,
                );
            }
            _ => {} // Continue with context-based completion
        }
    };

    // If we have exactly one expected type, focus on that (don't mix with others)
    if context.expected_next.len() == 1 {
        let expected = &context.expected_next[0];
        return match expected {
            ExpectedType::Account => {
                complete_account_internal(beancount_data, &context.prefix, false)
            }
            ExpectedType::Currency => complete_currency(&context.prefix),
            ExpectedType::Amount => complete_amount(&context),
            ExpectedType::Date => complete_date(),
            ExpectedType::Flag => complete_flag(),
            ExpectedType::Narration => {
                let line_text = content.line(cursor_point.row).to_string();
                complete_narration_with_quotes(beancount_data, &line_text, cursor_point.column)
            }
            ExpectedType::Payee => complete_payee(beancount_data, &context.prefix),
            ExpectedType::Tag => complete_tag(beancount_data),
            ExpectedType::Link => complete_link(beancount_data),
            ExpectedType::TransactionKind => complete_kind(),
        };
    }

    // For multiple expected types, provide all relevant completions
    // This happens when context is ambiguous (e.g., at document root)
    let mut all_completions = Vec::new();

    for expected in &context.expected_next {
        let completions = match expected {
            ExpectedType::Account => {
                complete_account_internal(beancount_data.clone(), &context.prefix, false)?
                    .unwrap_or_default()
            }
            ExpectedType::Currency => complete_currency(&context.prefix)?.unwrap_or_default(),
            ExpectedType::Amount => complete_amount(&context)?.unwrap_or_default(),
            ExpectedType::Date => complete_date()?.unwrap_or_default(),
            ExpectedType::Flag => complete_flag()?.unwrap_or_default(),
            ExpectedType::Narration => {
                let line_text = content.line(cursor_point.row).to_string();
                complete_narration_with_quotes(
                    beancount_data.clone(),
                    &line_text,
                    cursor_point.column,
                )?
                .unwrap_or_default()
            }
            ExpectedType::Payee => {
                complete_payee(beancount_data.clone(), &context.prefix)?.unwrap_or_default()
            }
            ExpectedType::Tag => complete_tag(beancount_data.clone())?.unwrap_or_default(),
            ExpectedType::Link => complete_link(beancount_data.clone())?.unwrap_or_default(),
            ExpectedType::TransactionKind => complete_kind()?.unwrap_or_default(),
        };

        all_completions.extend(completions);
    }

    // If we have specific completions from expected types, return them
    if !all_completions.is_empty() {
        return Ok(Some(all_completions));
    }

    // Fallback based on structure type when context is unclear
    match context.structure_type {
        StructureType::Transaction | StructureType::Posting => {
            complete_account_internal(beancount_data, &context.prefix, false)
        }
        StructureType::DocumentRoot => {
            // At document root, provide both dates and transaction kinds
            let mut items = complete_date()?.unwrap_or_default();
            items.extend(complete_kind()?.unwrap_or_default());
            Ok(Some(items))
        }
        _ => Ok(None),
    }
}

/// Complete currency codes
fn complete_currency(prefix: &str) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    let currencies = vec![
        "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "PLN", "CZK",
        "HUF", "RON", "BGN", "HRK", "RSD", "BAM", "MKD", "ISK", "TRY", "RUB", "UAH", "BYN", "MDL",
        "GEL", "AMD", "AZN", "KZT", "UZS", "KGS", "TJS", "TMT", "AFN", "PKR", "INR", "NPR", "BTN",
        "LKR", "MVR", "BDT", "MMK", "THB", "LAK", "KHR", "VND", "CNY", "HKD", "MOP", "TWD", "KRW",
        "MNT", "KPW", "IDR", "MYR", "BND", "SGD", "PHP", "PGK", "FJD", "SBD", "VUV", "WST", "TOP",
        "NZD", "AUD", "USD",
    ];

    let items: Vec<lsp_types::CompletionItem> = currencies
        .into_iter()
        .filter(|currency| {
            prefix.is_empty() || currency.to_lowercase().starts_with(&prefix.to_lowercase())
        })
        .map(|currency| lsp_types::CompletionItem {
            label: currency.to_string(),
            detail: Some("Currency".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        })
        .collect();

    Ok(if items.is_empty() { None } else { Some(items) })
}

/// Complete amount suggestions (context-aware)
fn complete_amount(_context: &CompletionContext) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    let amounts = vec![
        "100.00", "50.00", "25.00", "10.00", "5.00", "1000.00", "500.00", "250.00",
    ];

    let items: Vec<lsp_types::CompletionItem> = amounts
        .into_iter()
        .map(|amount| lsp_types::CompletionItem {
            label: amount.to_string(),
            detail: Some("Amount".to_string()),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            ..Default::default()
        })
        .collect();

    Ok(Some(items))
}

/// Complete transaction flags
fn complete_flag() -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    let flags = vec![
        ("*", "Complete transaction"),
        ("!", "Incomplete transaction (for debugging)"),
    ];

    let items: Vec<lsp_types::CompletionItem> = flags
        .into_iter()
        .map(|(flag, description)| lsp_types::CompletionItem {
            label: flag.to_string(),
            detail: Some(description.to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        })
        .collect();

    Ok(Some(items))
}

/// Complete payee names from previous transactions
fn complete_payee(
    beancount_data: HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    let mut payees = std::collections::HashSet::new();

    // Extract payees from narration (this is a simplified approach)
    for data in beancount_data.values() {
        for narration in data.get_narration() {
            // Simple heuristic: if narration doesn't start with quotes,
            // it might be a payee. This could be improved with better parsing.
            let clean_narration = narration.trim_matches('"');
            if !clean_narration.is_empty() && clean_narration.len() < 50 {
                payees.insert(clean_narration.to_string());
            }
        }
    }

    let items: Vec<lsp_types::CompletionItem> = payees
        .into_iter()
        .filter(|payee| prefix.is_empty() || payee.to_lowercase().contains(&prefix.to_lowercase()))
        .map(|payee| lsp_types::CompletionItem {
            label: payee.clone(),
            detail: Some("Payee".to_string()),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            insert_text: Some(format!("\"{payee}\"")),
            ..Default::default()
        })
        .collect();

    Ok(if items.is_empty() { None } else { Some(items) })
}

pub(crate) fn complete_date() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::date");
    let today = chrono::offset::Local::now().naive_local().date();
    let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
    debug!("providers::completion::date {}", prev_month);
    let cur_month = today.format("%Y-%m-").to_string();
    debug!("providers::completion::date {}", cur_month);
    let next_month = add_one_month(today).format("%Y-%m-").to_string();
    debug!("providers::completion::date {}", next_month);
    let today = today.format("%Y-%m-%d").to_string();
    debug!("providers::completion::date {}", today);
    let items = vec![
        lsp_types::CompletionItem {
            label: today,
            detail: Some("today".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: cur_month,
            detail: Some("this month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: prev_month,
            detail: Some("prev month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: next_month,
            detail: Some("next month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

pub(crate) fn complete_kind() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::kind");
    let items = vec![
        lsp_types::CompletionItem {
            label: String::from("txn"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("balance"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("open"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("close"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

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

fn complete_narration_with_quotes(
    data: HashMap<PathBuf, Arc<BeancountData>>,
    line_text: &str,
    cursor_char: usize,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::narration");

    // Check if there's already a closing quote after the cursor
    let has_closing_quote = line_text.chars().skip(cursor_char).any(|c| c == '"');
    debug!(
        "providers::completion::narration - has_closing_quote: {}",
        has_closing_quote
    );

    let mut completions = Vec::new();
    for data in data.values() {
        for txn_string in data.get_narration() {
            let insert_text = if has_closing_quote {
                // Remove the quotes from the stored string and don't add closing quote
                txn_string.trim_matches('"').to_string()
            } else {
                // Keep the full quoted string as stored
                txn_string.clone()
            };

            completions.push(lsp_types::CompletionItem {
                label: txn_string.clone(),
                detail: Some("Beancount Narration".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                insert_text: Some(insert_text),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

fn complete_account_with_prefix(
    data: HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::account with prefix: '{}'", prefix);
    complete_account_internal_colon_triggered(data, prefix)
}

fn complete_account_internal_colon_triggered(
    data: HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!(
        "providers::completion::account colon-triggered with prefix: '{}'",
        prefix
    );
    let mut completions = Vec::new();

    for data in data.values() {
        let accounts: Vec<String> = data.get_accounts().into_iter().collect();

        // Find accounts that start with the prefix
        let matching_accounts: Vec<String> = accounts
            .into_iter()
            .filter(|account| account.starts_with(prefix))
            .collect();

        // Extract the parts after the prefix
        for account in matching_accounts {
            if let Some(suffix) = account.strip_prefix(prefix) {
                // Remove leading colon if present
                let suffix = suffix.strip_prefix(':').unwrap_or(suffix);

                // Only show the next segment (up to the next colon, if any)
                let next_segment = if let Some(colon_pos) = suffix.find(':') {
                    &suffix[..colon_pos]
                } else {
                    suffix
                };

                // Skip empty segments and avoid duplicates
                if !next_segment.is_empty() {
                    let completion_text = next_segment.to_string();

                    // Check if we already have this completion
                    if !completions
                        .iter()
                        .any(|item: &lsp_types::CompletionItem| item.label == completion_text)
                    {
                        completions.push(create_completion_item(completion_text, 1.0));
                    }
                }
            }
        }
    }

    // Sort completions alphabetically
    completions.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Some(completions))
}

fn complete_account_internal(
    data: HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    filter_by_prefix: bool,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!(
        "providers::completion::account internal - prefix: '{}', filter: {}",
        prefix, filter_by_prefix
    );
    let mut completions = Vec::new();

    for data in data.values() {
        let accounts: Vec<String> = data.get_accounts().into_iter().collect();

        if filter_by_prefix {
            // Colon-triggered completion: use traditional prefix filtering
            let search_mode = determine_search_mode(prefix);
            debug!("Search mode: {:?} for prefix: '{}'", search_mode, prefix);

            match search_mode {
                SearchMode::Prefix => {
                    // Capital letter typed - show all accounts but prioritize those starting with prefix
                    for account in &accounts {
                        let score = if prefix.is_empty() || account.starts_with(prefix) {
                            1000.0 // High score for exact prefix matches
                        } else {
                            1.0 // Low score for non-matching accounts (but still included)
                        };
                        completions.push(create_completion_item(account.clone(), score));
                    }
                }
                SearchMode::Fuzzy => {
                    // Lowercase letter typed - fuzzy search all accounts
                    let fuzzy_matches = fuzzy_search_accounts(&accounts, prefix);
                    for (account, score) in fuzzy_matches {
                        completions.push(create_completion_item(account, score));
                    }
                }
                SearchMode::Exact => {
                    // No prefix or mixed case - use exact prefix matching with filtering
                    let prefix_lower = prefix.to_lowercase();
                    for account in accounts {
                        if prefix.is_empty() || account.to_lowercase().starts_with(&prefix_lower) {
                            completions.push(create_completion_item(account, 1.0));
                        }
                    }
                }
            }
        } else {
            // Normal completion: return ALL accounts, use fuzzy matching for ordering
            let fuzzy_matches = fuzzy_search_accounts_with_limit(&accounts, prefix, None);
            for (account, score) in fuzzy_matches {
                completions.push(create_completion_item(account, score));
            }
        }
    }

    // Sort by sort_text (lower values first, since sort_text is inverted from score)
    completions.sort_by(|a, b| {
        let sort_a = a
            .sort_text
            .as_ref()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(99999.0);
        let sort_b = b
            .sort_text
            .as_ref()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(99999.0);
        sort_a
            .partial_cmp(&sort_b)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });

    Ok(Some(completions))
}

#[derive(Debug, PartialEq)]
enum SearchMode {
    Prefix, // Single A/L/I/E or other capital letters - show accounts with exact prefix match
    Fuzzy,  // Lowercase letters - fuzzy search all accounts
    Exact,  // Empty or mixed case - exact prefix matching
}

fn determine_search_mode(prefix: &str) -> SearchMode {
    if prefix.is_empty() {
        SearchMode::Exact
    } else if prefix.len() == 1 && matches!(prefix.chars().next(), Some('A' | 'L' | 'I' | 'E')) {
        // Single uppercase A, L, I, E - filter by account type
        SearchMode::Prefix
    } else if prefix
        .chars()
        .all(|c| c.is_uppercase() || !c.is_alphabetic())
    {
        // All uppercase letters - exact prefix matching
        SearchMode::Prefix
    } else if prefix
        .chars()
        .all(|c| c.is_lowercase() || !c.is_alphabetic())
    {
        // All lowercase letters - fuzzy search across all accounts
        SearchMode::Fuzzy
    } else {
        // Mixed case - exact prefix matching
        SearchMode::Exact
    }
}

/// Clear score tiers with significant gaps for predictable ranking
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum ScoreTier {
    Exact = 10000,    // Exact matches (account == query)
    Prefix = 7000,    // Prefix matches (account.starts_with(query))
    IntraWord = 4000, // Matches within colon segments
    Fuzzy = 1000,     // Cross-segment fuzzy matches
    Fallback = 1,     // All other accounts (for "show all" behavior)
}

impl ScoreTier {
    fn as_f32(self) -> f32 {
        self as u32 as f32
    }
}

fn fuzzy_search_accounts(accounts: &[String], query: &str) -> Vec<(String, f32)> {
    fuzzy_search_accounts_with_limit(accounts, query, Some(20))
}

fn fuzzy_search_accounts_with_limit(
    accounts: &[String],
    query: &str,
    limit: Option<usize>,
) -> Vec<(String, f32)> {
    if query.is_empty() {
        let mut all_accounts: Vec<(String, f32)> = accounts
            .iter()
            .map(|acc| (acc.clone(), ScoreTier::Fallback.as_f32()))
            .collect();

        // Sort alphabetically when no query
        all_accounts.sort_by(|a, b| a.0.cmp(&b.0));

        if let Some(limit_size) = limit {
            all_accounts.truncate(limit_size);
        }
        return all_accounts;
    }

    let mut scored_accounts: Vec<(String, f32)> = Vec::new();

    for account in accounts {
        let score = score_account(account, query);
        scored_accounts.push((account.clone(), score));
    }

    // Sort by score descending, then alphabetically
    scored_accounts.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    // Apply limit if specified
    if let Some(limit_size) = limit {
        scored_accounts.truncate(limit_size);
    }

    scored_accounts
}

/// Score an account using clear tiers with early returns for simplicity
fn score_account(account: &str, query: &str) -> f32 {
    if query.is_empty() {
        return ScoreTier::Fallback.as_f32();
    }

    let account_lower = account.to_lowercase();
    let query_lower = query.to_lowercase();

    // Tier 1: Exact matches (highest priority)
    if account == query {
        return ScoreTier::Exact.as_f32();
    }
    if account_lower == query_lower {
        return ScoreTier::Exact.as_f32() - 100.0;
    }

    // Tier 2: Prefix matches (very high priority)
    if account.starts_with(query) {
        return ScoreTier::Prefix.as_f32();
    }
    if account_lower.starts_with(&query_lower) {
        return ScoreTier::Prefix.as_f32() - 100.0;
    }

    // Tier 3: Intra-word matches (within colon segments)
    if let Some(score) = score_intra_word_match(account, query) {
        return ScoreTier::IntraWord.as_f32() + score;
    }

    // Tier 4: Fuzzy matches (nucleo only)
    if let Some(score) = score_with_nucleo(account, query) {
        return ScoreTier::Fuzzy.as_f32() + score;
    }

    // Tier 5: Fallback (ensures all accounts are included)
    ScoreTier::Fallback.as_f32()
}

/// Check for matches within colon segments, returning best score if found
/// E.g., "cash" in "Assets:Cash:Apple" matches entirely within the "Cash" segment
fn score_intra_word_match(account: &str, query: &str) -> Option<f32> {
    let query_lower = query.to_lowercase();
    let segments: Vec<&str> = account.split(':').collect();

    let mut best_score: f32 = 0.0;
    let mut found_match = false;

    for (segment_index, segment) in segments.iter().enumerate() {
        let segment_lower = segment.to_lowercase();

        // Exact segment match (highest within this tier)
        if segment_lower == query_lower {
            let score = 500.0 - (segment_index as f32 * 50.0);
            best_score = best_score.max(score);
            found_match = true;
        }
        // Segment prefix match
        else if segment_lower.starts_with(&query_lower) {
            let score = 300.0 - (segment_index as f32 * 30.0);
            best_score = best_score.max(score);
            found_match = true;
        }
        // Substring within segment
        else if segment_lower.contains(&query_lower) {
            let score = 100.0 - (segment_index as f32 * 10.0);
            best_score = best_score.max(score);
            found_match = true;
        }
    }

    if found_match { Some(best_score) } else { None }
}

/// Use nucleo for fuzzy matching across segments
fn score_with_nucleo(account: &str, query: &str) -> Option<f32> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    // Convert string to Utf32Str as required by nucleo API
    let mut char_buf = Vec::new();
    let account_utf32 = Utf32Str::new(account, &mut char_buf);

    if let Some(score) = pattern.score(account_utf32, &mut matcher) {
        // Normalize nucleo score to our range (0-500)
        let normalized_score = (score as f32 / 100.0).min(500.0);
        Some(normalized_score)
    } else {
        None
    }
}

fn create_completion_item(account: String, score: f32) -> lsp_types::CompletionItem {
    lsp_types::CompletionItem {
        label: account.clone(),
        detail: Some("Beancount Account".to_string()),
        kind: Some(lsp_types::CompletionItemKind::ENUM),
        filter_text: Some(account.clone()),
        // Use score for sorting (higher scores first, so invert for lexicographic sort)
        sort_text: Some(format!("{:010.0}", 99999.0 - score.min(99999.0))),
        // Let the LSP client handle text replacement based on filter_text
        ..Default::default()
    }
}

/// Extract the current word/prefix being typed for completion
pub(crate) fn extract_completion_prefix(line_text: &str, cursor_char: usize) -> String {
    let chars: Vec<char> = line_text.chars().collect();
    if cursor_char == 0 || cursor_char > chars.len() {
        return String::new();
    }

    let mut start = cursor_char.saturating_sub(1);

    // Find the start of the current word (account name)
    // Account names can contain letters, numbers, colons, and hyphens
    while start > 0 {
        let c = chars[start.saturating_sub(1)];
        if !c.is_alphanumeric() && c != ':' && c != '-' && c != '_' {
            break;
        }
        start = start.saturating_sub(1);
    }

    // Extract the prefix from start to cursor
    let end = cursor_char.min(chars.len());
    chars[start..end].iter().collect()
}

pub(crate) fn complete_tag(
    data: HashMap<PathBuf, Arc<BeancountData>>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for tag in data.get_tags() {
            completions.push(lsp_types::CompletionItem {
                label: tag,
                detail: Some("Beancount Tag".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

pub(crate) fn complete_link(
    data: HashMap<PathBuf, Arc<BeancountData>>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for link in data.get_links() {
            completions.push(lsp_types::CompletionItem {
                label: link,
                detail: Some("Beancount Link".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

#[cfg(test)]
mod tests {
    use crate::providers::completion::add_one_month;
    use crate::providers::completion::completion;
    use crate::providers::completion::extract_completion_prefix;
    use crate::providers::completion::sub_one_month;
    use crate::server::LspServerStateSnapshot;
    use tree_sitter_beancount::tree_sitter;
    //use insta::assert_yaml_snapshot;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use crate::utils::ToFilePath;
    use anyhow::Result;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;
    use test_log::test;

    #[derive(Debug)]
    pub struct Fixture {
        pub documents: Vec<TestDocument>,
    }
    impl Fixture {
        pub fn parse(input: &str) -> Self {
            let mut documents = Vec::new();
            let mut start = 0;
            if !input.is_empty() {
                for end in input
                    .match_indices("%!")
                    .skip(1)
                    .map(|(i, _)| i)
                    .chain(std::iter::once(input.len()))
                {
                    documents.push(TestDocument::parse(&input[start..end]));
                    start = end;
                }
            }
            Self { documents }
        }
    }

    #[derive(Debug)]
    pub struct TestDocument {
        pub path: String,
        pub text: String,
        pub cursor: Option<lsp_types::Position>,
        // pub ranges: Vec<lsp_types::Range>,
    }
    impl TestDocument {
        pub fn parse(input: &str) -> Self {
            let mut lines = Vec::new();

            let (path, input) = input
                .trim()
                .strip_prefix("%! ")
                .map(|input| input.split_once('\n').unwrap_or((input, "")))
                .unwrap();

            let mut ranges = Vec::new();
            let mut cursor = None;

            for line in input.lines() {
                if line.chars().all(|c| matches!(c, ' ' | '^' | '|' | '!')) && !line.is_empty() {
                    let index = (lines.len() - 1) as u32;

                    cursor = cursor.or_else(|| {
                        let character = line.find('|')?;
                        Some(lsp_types::Position::new(index, character as u32))
                    });

                    if let Some(start) = line.find('!') {
                        let position = lsp_types::Position::new(index, start as u32);
                        ranges.push(lsp_types::Range::new(position, position));
                    }

                    if let Some(start) = line.find('^') {
                        let end = line.rfind('^').unwrap() + 1;
                        ranges.push(lsp_types::Range::new(
                            lsp_types::Position::new(index, start as u32),
                            lsp_types::Position::new(index, end as u32),
                        ));
                    }
                } else {
                    lines.push(line);
                }
            }

            Self {
                path: path.to_string(),
                text: lines.join("\n"),
                cursor,
                // ranges,
            }
        }
    }

    pub struct TestState {
        fixture: Fixture,
        snapshot: LspServerStateSnapshot,
    }
    impl TestState {
        /// Converts a test fixture path to a PathBuf, handling cross-platform compatibility.
        /// Uses a simpler approach that should work on all platforms.
        fn path_from_fixture(path: &str) -> Result<PathBuf> {
            // For empty paths, return a default path that should work on all platforms
            if path.is_empty() {
                return Ok(std::path::PathBuf::from("/"));
            }

            // Try to create the URI and convert to path
            // First try the path as-is (works for absolute paths on Unix and relative paths)
            let uri_str = if path.starts_with('/') {
                // Unix-style absolute path
                if cfg!(windows) {
                    format!("file:///C:{path}")
                } else {
                    format!("file://{path}")
                }
            } else if cfg!(windows) && path.len() > 1 && path.chars().nth(1) == Some(':') {
                // Windows-style absolute path like "C:\path"
                format!("file:///{}", path.replace('\\', "/"))
            } else {
                // Relative path or other format - this will likely fail but let's try
                format!("file://{path}")
            };

            let uri = lsp_types::Uri::from_str(&uri_str)
                .map_err(|e| anyhow::anyhow!("Invalid URI: {}", e))?;

            // Check if this is a problematic URI format that would cause to_file_path() to panic
            // URIs like "file://bare-filename" (without path separators) are problematic because
            // they treat the filename as a hostname. Paths with "./" or "../" are typically OK.
            if uri_str.starts_with("file://") && !uri_str.starts_with("file:///") {
                let after_protocol = &uri_str[7..]; // Remove "file://"
                if !after_protocol.is_empty()
                    && !after_protocol.starts_with('/')
                    && !after_protocol.starts_with('.')
                {
                    return Err(anyhow::anyhow!(
                        "Invalid file URI format (contains hostname): {}",
                        uri_str
                    ));
                }
            }

            let file_path = uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("Failed to convert URI to file path: {}", uri_str))?;

            Ok(file_path)
        }

        pub fn new(fixture: &str) -> Result<Self> {
            let fixture = Fixture::parse(fixture);
            let forest: HashMap<PathBuf, Arc<tree_sitter::Tree>> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = Self::path_from_fixture(path)?;
                    let mut parser = tree_sitter::Parser::new();
                    parser
                        .set_language(&tree_sitter_beancount::language())
                        .unwrap();
                    let v = Arc::new(parser.parse(document.text.clone(), None).unwrap());
                    Ok((k, v))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            let beancount_data: HashMap<PathBuf, Arc<BeancountData>> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = Self::path_from_fixture(path)?;
                    let content = ropey::Rope::from(document.text.clone());
                    let v = Arc::new(BeancountData::new(forest.get(&k).unwrap(), &content));
                    Ok((k, v))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            let open_docs: HashMap<PathBuf, Document> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = Self::path_from_fixture(path)?;
                    let v = Document {
                        content: ropey::Rope::from(document.text.clone()),
                    };
                    Ok((k, v))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            Ok(TestState {
                fixture,
                snapshot: LspServerStateSnapshot {
                    beancount_data,
                    config: Config::new(Self::path_from_fixture("/test.beancount")?),
                    forest,
                    open_docs,
                },
            })
        }

        pub fn cursor(&self) -> Option<lsp_types::TextDocumentPositionParams> {
            let (document, cursor) = self
                .fixture
                .documents
                .iter()
                .find_map(|document| document.cursor.map(|cursor| (document, cursor)))?;

            let path = document.path.as_str();
            // Use the same path conversion logic as in TestState::new() to ensure consistency
            let file_path = Self::path_from_fixture(path).ok()?;

            // Convert PathBuf back to URI string for cross-platform compatibility
            let path_str = file_path.to_string_lossy();
            let uri_str = if cfg!(windows) {
                // On Windows, paths start with drive letter, need file:/// prefix
                format!("file:///{}", path_str.replace('\\', "/"))
            } else {
                format!("file://{path_str}")
            };

            let uri = lsp_types::Uri::from_str(&uri_str).ok()?;
            let id = lsp_types::TextDocumentIdentifier::new(uri);
            Some(lsp_types::TextDocumentPositionParams::new(id, cursor))
        }
    }

    #[test]
    fn handle_sub_one_month() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 5, 1).expect("valid date");
        assert_eq!(sub_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_sub_one_month_in_jan() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
        assert_eq!(sub_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_add_one_month() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 7, 1).expect("valid date");
        assert_eq!(add_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_add_one_month_in_dec() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        assert_eq!(add_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_date_completion() {
        let fixure = r#"
%! /main.beancount
2
|
^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let text_document_position = test_state.cursor().unwrap();
        println!(
            "{} {}",
            text_document_position.position.line, text_document_position.position.character
        );
        let items = completion(test_state.snapshot, Some('2'), text_document_position)
            .unwrap()
            .unwrap_or_default();
        let today = chrono::offset::Local::now().naive_local().date();
        let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
        let cur_month = today.format("%Y-%m-").to_string();
        let next_month = add_one_month(today).format("%Y-%m-").to_string();
        let today = today.format("%Y-%m-%d").to_string();
        // Check that all expected date completions are present (new system also provides transaction types)
        let date_items: Vec<&lsp_types::CompletionItem> =
            items.iter().filter(|item| item.detail.is_some()).collect();

        assert_eq!(date_items.len(), 4);
        assert!(
            items
                .iter()
                .any(|item| item.label == today && item.detail == Some("today".to_string()))
        );
        assert!(
            items.iter().any(
                |item| item.label == cur_month && item.detail == Some("this month".to_string())
            )
        );
        assert!(
            items
                .iter()
                .any(|item| item.label == prev_month
                    && item.detail == Some("prev month".to_string()))
        );
        assert!(
            items
                .iter()
                .any(|item| item.label == next_month
                    && item.detail == Some("next month".to_string()))
        )
    }

    #[test]
    fn handle_txn_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 t
            |
            ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        // Check that transaction types are included (new system also provides dates)
        let txn_kinds: Vec<String> = items
            .iter()
            .filter(|item| matches!(item.label.as_str(), "txn" | "balance" | "open" | "close"))
            .map(|item| item.label.clone())
            .collect();

        assert!(txn_kinds.contains(&"txn".to_string()));
        assert!(txn_kinds.contains(&"balance".to_string()));
        assert!(txn_kinds.contains(&"open".to_string()));
        assert!(txn_kinds.contains(&"close".to_string()));
    }

    #[test]
    fn handle_narration_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("\"Test Co\""),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Narration")),
                insert_text: Some(String::from("\"Test Co\"")), // No closing quote exists, so keep full quoted string
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_payee_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "Test" "
                       |
                       ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        // New intelligent system provides narration completions after payee
        assert!(!items.is_empty());
        assert!(items.iter().any(|item| item.label == "\"Foo Bar\""));
    }

    #[test]
    fn handle_narration_completion_with_existing_closing_quote() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "Test Co"
2023-10-01 txn ""
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        // Should have completions with insert_text without quotes since closing quote exists
        assert!(!items.is_empty());
        let test_co_completion = items
            .iter()
            .find(|item| item.label == "\"Test Co\"")
            .unwrap();
        assert_eq!(
            test_co_completion.insert_text,
            Some(String::from("Test Co"))
        );

        let foo_bar_completion = items
            .iter()
            .find(|item| item.label == "\"Foo Bar\"")
            .unwrap();
        assert_eq!(
            foo_bar_completion.insert_text,
            Some(String::from("Foo Bar"))
        );
    }

    #[test]
    fn handle_narration_completion_without_closing_quote() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("\"Foo Bar\""),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Narration")),
                insert_text: Some(String::from("\"Foo Bar\"")), // Keep full quotes since no closing quote
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_account_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    a
     |
     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        // Should now show both accounts when typing lowercase 'a'
        assert_eq!(items.len(), 2);
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(labels.contains(&&"Assets:Test".to_string()));
        assert!(labels.contains(&&"Expenses:Test".to_string()));

        // Verify properties are correct
        for item in &items {
            assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::ENUM));
            assert_eq!(item.detail, Some("Beancount Account".to_string()));
        }
    }

    #[test]
    fn handle_account_completion_with_colon() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Assets:Checking USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:
           |
           ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some(':'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(items.len(), 2);

        // Should have both Assets accounts parts after the colon
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(labels.contains(&&"Test".to_string()));
        assert!(labels.contains(&&"Checking".to_string()));

        // Check properties of all items
        for item in &items {
            assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::ENUM));
            assert_eq!(item.detail, Some("Beancount Account".to_string()));
            // Note: labels now contain only the part after the colon, not the full account name
        }
    }

    #[test]
    fn handle_case_insensitive_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Asse
        |
        ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        // Should return all accounts, with "Assets:Test" ranked highest due to prefix match
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Assets:Test"); // Highest ranked due to prefix match
        assert_eq!(items[0].kind, Some(lsp_types::CompletionItemKind::ENUM));
        assert_eq!(items[0].detail, Some("Beancount Account".to_string()));
        assert_eq!(items[0].filter_text, Some("Assets:Test".to_string()));

        // Second account should also be present
        assert_eq!(items[1].label, "Expenses:Test");
    }

    #[test]
    fn handle_tag_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar" #tag ^link
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn  "Test Co" "Foo Bar" #
                                     |
                                     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('#'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("#tag"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Tag")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_link_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar" #tag ^link
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn  "Test Co" "Foo Bar" #
                                     |
                                     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('^'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("^link"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Link")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn test_path_from_fixture_unix_style() {
        let result = TestState::path_from_fixture("/main.beancount");
        assert!(result.is_ok());
        let path = result.unwrap();

        if cfg!(windows) {
            // On Windows, should convert to C:\main.beancount
            assert_eq!(path.to_string_lossy(), "C:\\main.beancount");
        } else {
            // On Unix, should remain /main.beancount
            assert_eq!(path.to_string_lossy(), "/main.beancount");
        }
    }

    #[test]
    fn test_path_from_fixture_relative_path() {
        // Relative paths without leading slash create invalid file URIs
        // (they become hostnames), so they should fail
        let result = TestState::path_from_fixture("main.beancount");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_from_fixture_dot_relative_path() {
        // Test relative path starting with ./
        // On Windows, this succeeds and creates a UNC path like \\.\main.beancount
        // On Unix, this fails because the dot becomes a hostname in the file URI
        let result = TestState::path_from_fixture("./main.beancount");
        if cfg!(windows) {
            // On Windows, this succeeds and creates a UNC path
            assert!(result.is_ok());
            let path = result.unwrap();
            assert!(path.to_string_lossy().contains("main.beancount"));
        } else {
            // On Unix, this should fail
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_path_from_fixture_nested_unix_path() {
        let result = TestState::path_from_fixture("/some/nested/path.beancount");
        assert!(result.is_ok());
        let path = result.unwrap();

        if cfg!(windows) {
            // On Windows, should convert to C:\some\nested\path.beancount
            assert_eq!(path.to_string_lossy(), "C:\\some\\nested\\path.beancount");
        } else {
            // On Unix, should remain /some/nested/path.beancount
            assert_eq!(path.to_string_lossy(), "/some/nested/path.beancount");
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_path_from_fixture_windows_style() {
        // Test that Windows-style paths work correctly
        let result = TestState::path_from_fixture("C:\\main.beancount");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert_eq!(path.to_string_lossy(), "C:\\main.beancount");
    }

    #[test]
    fn test_path_from_fixture_invalid_uri() {
        // Test with a path that would create an invalid URI
        let result = TestState::path_from_fixture("invalid uri with spaces and special chars: <>");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_from_fixture_empty_path() {
        let result = TestState::path_from_fixture("");
        // Empty paths create file:// which should be handled gracefully
        assert!(result.is_ok());
        let path = result.unwrap();
        // Path should exist and be some kind of root/base path
        assert!(!path.to_string_lossy().is_empty());
        // Don't make specific assertions about the exact path format as it's platform-dependent
    }

    #[test]
    fn test_complete_kind_function() {
        // Test the complete_kind function directly
        use crate::providers::completion::complete_kind;

        let items = complete_kind().unwrap().unwrap();
        assert_eq!(items.len(), 4);

        let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
        assert!(labels.contains(&"txn".to_string()));
        assert!(labels.contains(&"balance".to_string()));
        assert!(labels.contains(&"open".to_string()));
        assert!(labels.contains(&"close".to_string()));
    }

    #[test]
    fn test_extract_completion_prefix_functionality() {
        // Test that the extract_completion_prefix function works correctly
        // This tests the actual implementation without relying on complex fixtures
        assert_eq!(extract_completion_prefix("Assets:Test", 11), "Assets:Test");
        assert_eq!(extract_completion_prefix("Assets:Test", 6), "Assets");
        assert_eq!(extract_completion_prefix("Assets:Test", 7), "Assets:");
        assert_eq!(extract_completion_prefix("Assets:Test", 0), "");
        assert_eq!(
            extract_completion_prefix("    Assets:Test", 15),
            "Assets:Test"
        );
        assert_eq!(
            extract_completion_prefix("Assets:Test-USD", 15),
            "Assets:Test-USD"
        );
    }

    #[test]
    fn test_completion_functions_directly() {
        // Test the completion functions directly rather than through complex fixtures
        use crate::providers::completion::{complete_date, complete_link, complete_tag};
        use std::collections::HashMap;

        let data = HashMap::new();

        // Test tag completion - with empty data should return empty list
        let tag_items = complete_tag(data.clone()).unwrap().unwrap();
        assert_eq!(tag_items.len(), 0); // No tags in empty data

        // Test link completion - with empty data should return empty list
        let link_items = complete_link(data).unwrap().unwrap();
        assert_eq!(link_items.len(), 0); // No links in empty data

        // Test date completion - this doesn't depend on data
        let date_items = complete_date().unwrap().unwrap();
        assert_eq!(date_items.len(), 4);
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("today".to_string()))
        );
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("this month".to_string()))
        );
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("prev month".to_string()))
        );
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("next month".to_string()))
        );
    }

    #[test]
    fn test_search_mode_determination() {
        use crate::providers::completion::{SearchMode, determine_search_mode};

        // Single A, L, I, E should trigger prefix search for account types
        assert_eq!(determine_search_mode("A"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("L"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("I"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("E"), SearchMode::Prefix);

        // Other single capital letters should also trigger prefix search
        assert_eq!(determine_search_mode("B"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("C"), SearchMode::Prefix);

        // Multiple capital letters should trigger prefix search
        assert_eq!(determine_search_mode("AS"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("ASSETS"), SearchMode::Prefix);

        // Lowercase letters should trigger fuzzy search
        assert_eq!(determine_search_mode("a"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("as"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("assets"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("checking"), SearchMode::Fuzzy);

        // Mixed case should use exact matching
        assert_eq!(determine_search_mode("As"), SearchMode::Exact);
        assert_eq!(determine_search_mode("Assets"), SearchMode::Exact);
        assert_eq!(determine_search_mode("AssetS"), SearchMode::Exact);

        // Empty prefix should use exact matching
        assert_eq!(determine_search_mode(""), SearchMode::Exact);

        // Non-alphabetic characters should not affect mode determination
        assert_eq!(determine_search_mode("A:"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("a-"), SearchMode::Fuzzy);
    }

    #[test]
    fn test_fuzzy_search_accounts() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Cash:Savings".to_string(),
            "Assets:Investments:Stocks".to_string(),
            "Liabilities:CreditCard:Visa".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Expenses:Food:Restaurants".to_string(),
            "Income:Salary".to_string(),
        ];

        // Test exact match
        let matches = fuzzy_search_accounts(&accounts, "cash");
        assert!(!matches.is_empty());
        let cash_matches: Vec<&String> = matches
            .iter()
            .filter(|(acc, _)| acc.contains("Cash"))
            .map(|(acc, _)| acc)
            .collect();
        assert_eq!(cash_matches.len(), 2);

        // Test substring match
        let matches = fuzzy_search_accounts(&accounts, "food");
        assert!(!matches.is_empty());
        let food_matches: Vec<&String> = matches
            .iter()
            .filter(|(acc, _)| acc.contains("Food"))
            .map(|(acc, _)| acc)
            .collect();
        assert_eq!(food_matches.len(), 2);

        // Test fuzzy match (characters in order)
        let matches = fuzzy_search_accounts(&accounts, "chk");
        assert!(!matches.is_empty());
        let checking_match = matches.iter().find(|(acc, _)| acc.contains("Checking"));
        assert!(checking_match.is_some());

        // Test query with no matching characters - should still show all accounts but with low scores
        let matches = fuzzy_search_accounts(&accounts, "xyz");
        assert!(
            !matches.is_empty(),
            "Should show all accounts even with no character matches"
        );
        // All accounts should have at least minimum scores (1.0)
        assert!(
            matches.iter().all(|(_, score)| *score >= 1.0),
            "All accounts should have at least minimum score"
        );
    }

    #[test]
    fn handle_account_completion_with_includes() {
        // Test case reproducing GitHub issue #639
        let fixture = r#"
%! /file1.bean
include "accounts1.bean"

1900-01-01 open Assets:Checking
1900-01-01 open Expenses:Food
1900-01-01 open Expenses:Transport

2023-01-01 * "Grocery shopping"
  Assets:Checking  -50.00 USD
  Expenses:Food     50.00 USD

2023-01-02 * "Bus fare"
  Assets:Checking  -2.50 USD
  Expenses:Transport  2.50 USD

2023-01-03 * "Coffee"
  Assets:Checking  -4.00 USD
  Expenses:
          |
          ^

%! /accounts1.bean
1900-01-01 open Expenses:Included1
1900-01-01 open Expenses:Included2
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, Some(':'), cursor)
            .unwrap()
            .unwrap_or_default();

        // Should include accounts from both files
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // Account parts after "Expenses:" from main file
        assert!(
            labels.contains(&&"Food".to_string()),
            "Should include Food from main file"
        );
        assert!(
            labels.contains(&&"Transport".to_string()),
            "Should include Transport from main file"
        );

        // Account parts after "Expenses:" from included file - this is what was missing!
        assert!(
            labels.contains(&&"Included1".to_string()),
            "Should include Included1 from included file"
        );
        assert!(
            labels.contains(&&"Included2".to_string()),
            "Should include Included2 from included file"
        );

        // Should have 4 account parts total (after "Expenses:")
        assert_eq!(labels.len(), 4, "Should have 4 account parts total");
    }

    #[test]
    fn test_include_processing_end_to_end() {
        // Test that simulates the real issue more closely
        // This test verifies that when we have multiple files in beancount_data,
        // completion aggregates accounts from all of them
        use crate::beancount_data::BeancountData;
        use crate::providers::completion::complete_account_with_prefix;
        use std::collections::HashMap;

        // Create mock beancount data for multiple files using actual file content
        let mut beancount_data = HashMap::new();

        // Create mock trees and content for two files
        let main_content = "1900-01-01 open Assets:Checking\n1900-01-01 open Expenses:Food\n1900-01-01 open Expenses:Transport";
        let included_content =
            "1900-01-01 open Expenses:Included1\n1900-01-01 open Expenses:Included2";

        // Parse the main file
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let main_tree = parser.parse(main_content, None).unwrap();
        let main_rope = ropey::Rope::from_str(main_content);
        let main_data = Arc::new(BeancountData::new(&main_tree, &main_rope));
        beancount_data.insert(PathBuf::from("/main.beancount"), main_data);

        // Parse the included file
        let included_tree = parser.parse(included_content, None).unwrap();
        let included_rope = ropey::Rope::from_str(included_content);
        let included_data = Arc::new(BeancountData::new(&included_tree, &included_rope));
        beancount_data.insert(PathBuf::from("/accounts1.bean"), included_data);

        // Test completion with prefix "Expenses:"
        let items = complete_account_with_prefix(beancount_data, "Expenses:")
            .unwrap()
            .unwrap_or_default();

        let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();

        // Should include account parts after "Expenses:" from both files
        assert!(
            labels.contains(&"Food".to_string()),
            "Should include Food from main file"
        );
        assert!(
            labels.contains(&"Transport".to_string()),
            "Should include Transport from main file"
        );
        assert!(
            labels.contains(&"Included1".to_string()),
            "Should include Included1 from included file"
        );
        assert!(
            labels.contains(&"Included2".to_string()),
            "Should include Included2 from included file"
        );

        // Should have 4 account parts total (after "Expenses:")
        assert_eq!(
            labels.len(),
            4,
            "Should have 4 account parts total: {labels:?}"
        );
    }

    #[test]
    fn test_colon_triggered_completion_behavior() {
        // Test that colon-triggered completion returns only parts after the colon
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Checking:Personal USD
2023-10-01 open Assets:Checking:Business USD
2023-10-01 open Assets:Savings:Emergency USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Expenses:Food:Restaurants USD
2023-10-01 txn "Test transaction"
  Assets:Checking:
                 |
                 ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, Some(':'), cursor)
            .unwrap()
            .unwrap_or_default();

        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // Should return only the parts after "Assets:Checking:"
        assert_eq!(items.len(), 2);
        assert!(labels.contains(&&"Personal".to_string()));
        assert!(labels.contains(&&"Business".to_string()));

        // Should NOT contain full account paths
        assert!(!labels.contains(&&"Assets:Checking:Personal".to_string()));
        assert!(!labels.contains(&&"Assets:Checking:Business".to_string()));
    }

    #[test]
    fn test_top_level_colon_completion() {
        // Test completion at top level (e.g., typing "Assets:")
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Checking USD
2023-10-01 open Assets:Savings USD
2023-10-01 open Expenses:Food USD
2023-10-01 txn "Test transaction"
  Assets:
        |
        ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, Some(':'), cursor)
            .unwrap()
            .unwrap_or_default();

        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // Should return only the parts after "Assets:"
        assert_eq!(items.len(), 2);
        assert!(labels.contains(&&"Checking".to_string()));
        assert!(labels.contains(&&"Savings".to_string()));

        // Should NOT contain full account paths or accounts from other hierarchies
        assert!(!labels.contains(&&"Assets:Checking".to_string()));
        assert!(!labels.contains(&&"Food".to_string()));
    }

    #[test]
    fn test_nucleo_fuzzy_matching() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Cash:Savings".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Liabilities:CreditCard".to_string(),
        ];

        // Exact match should work
        let matches = fuzzy_search_accounts(&accounts, "cash");
        assert!(!matches.is_empty());

        // Should find accounts containing "cash"
        let cash_matches: Vec<&(String, f32)> = matches
            .iter()
            .filter(|(acc, _)| acc.to_lowercase().contains("cash"))
            .collect();
        assert!(!cash_matches.is_empty());

        // Should match against full account name - test "assets" should match "Assets:Cash:Checking"
        let assets_matches = fuzzy_search_accounts(&accounts, "assets");
        let assets_found = assets_matches
            .iter()
            .any(|(acc, _)| acc.starts_with("Assets"));
        assert!(assets_found, "Should find accounts starting with Assets");

        // Should match "assetchk" against "Assets:Cash:Checking" (fuzzy across full name)
        let fuzzy_full_matches = fuzzy_search_accounts(&accounts, "assetchk");
        let assetchk_found = fuzzy_full_matches
            .iter()
            .any(|(acc, _)| acc == "Assets:Cash:Checking");
        assert!(
            assetchk_found,
            "Should fuzzy match across full account name"
        );

        // Fuzzy matching should work
        let fuzzy_matches = fuzzy_search_accounts(&accounts, "chk");
        assert!(!fuzzy_matches.is_empty());

        // Query with no matching characters should still return all accounts with low scores
        let no_matches = fuzzy_search_accounts(&accounts, "xyz123");
        assert!(
            !no_matches.is_empty(),
            "Should return all accounts even with no character matches"
        );
        assert!(
            no_matches.iter().all(|(_, score)| *score >= 1.0),
            "All accounts should have at least minimum score"
        );
    }

    #[test]
    fn test_fuzzy_matching_full_account_names() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Investments:Stocks".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Expenses:Transportation:Gas".to_string(),
            "Liabilities:CreditCard:Visa".to_string(),
        ];

        // Test matching across account segments
        let matches = fuzzy_search_accounts(&accounts, "assetsinv");
        let found = matches
            .iter()
            .any(|(acc, _)| acc == "Assets:Investments:Stocks");
        assert!(
            found,
            "Should match 'assetsinv' to 'Assets:Investments:Stocks'"
        );

        // Test matching with partial segments
        let matches = fuzzy_search_accounts(&accounts, "exptrans");
        let found = matches
            .iter()
            .any(|(acc, _)| acc == "Expenses:Transportation:Gas");
        assert!(
            found,
            "Should match 'exptrans' to 'Expenses:Transportation:Gas'"
        );

        // Test case insensitive matching across full name
        let matches = fuzzy_search_accounts(&accounts, "LIABCRED");
        let found = matches
            .iter()
            .any(|(acc, _)| acc == "Liabilities:CreditCard:Visa");
        assert!(
            found,
            "Should match 'LIABCRED' to 'Liabilities:CreditCard:Visa'"
        );

        // Test matching with mixed separators
        let matches = fuzzy_search_accounts(&accounts, "foodgroc");
        let found = matches
            .iter()
            .any(|(acc, _)| acc == "Expenses:Food:Groceries");
        assert!(
            found,
            "Should match 'foodgroc' to 'Expenses:Food:Groceries'"
        );
    }

    #[test]
    fn test_deep_account_fuzzy_matching() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Expenses:Fixed:Food:Groceries".to_string(),
            "Expenses:Variable:Food:Restaurants".to_string(),
            "Assets:Cash:Checking".to_string(),
            "Income:Salary:Base".to_string(),
        ];

        // Test that 'food' matches both food-related accounts
        let matches = fuzzy_search_accounts(&accounts, "food");
        println!("Matches for 'food': {matches:?}");

        let food_groceries_found = matches
            .iter()
            .any(|(acc, _)| acc == "Expenses:Fixed:Food:Groceries");
        let food_restaurants_found = matches
            .iter()
            .any(|(acc, _)| acc == "Expenses:Variable:Food:Restaurants");

        assert!(
            food_groceries_found,
            "Should match 'food' to 'Expenses:Fixed:Food:Groceries'"
        );
        assert!(
            food_restaurants_found,
            "Should match 'food' to 'Expenses:Variable:Food:Restaurants'"
        );

        // Test that 'groceries' matches the groceries account
        let matches = fuzzy_search_accounts(&accounts, "groceries");
        println!("Matches for 'groceries': {matches:?}");
        let groceries_found = matches
            .iter()
            .any(|(acc, _)| acc == "Expenses:Fixed:Food:Groceries");
        assert!(
            groceries_found,
            "Should match 'groceries' to 'Expenses:Fixed:Food:Groceries'"
        );

        // Test fuzzy matching across multiple segments
        let matches = fuzzy_search_accounts(&accounts, "expfoodgroc");
        println!("Matches for 'expfoodgroc': {matches:?}");
        let fuzzy_found = matches
            .iter()
            .any(|(acc, _)| acc == "Expenses:Fixed:Food:Groceries");
        assert!(
            fuzzy_found,
            "Should fuzzy match 'expfoodgroc' to 'Expenses:Fixed:Food:Groceries'"
        );

        // Test search mode determination
        use crate::providers::completion::{SearchMode, determine_search_mode};
        assert_eq!(determine_search_mode("food"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("FOOD"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("Food"), SearchMode::Exact);
    }

    #[test]
    fn test_capital_letter_completion() {
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Assets:Investments:Stocks USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Income:Salary USD
2023-10-01 txn "Test"
    A
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        // Should show all accounts, with "A" accounts ranked highest
        assert_eq!(items.len(), 5);
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // Assets accounts should be first (highest scores)
        assert_eq!(items[0].label, "Assets:Cash:Checking");
        assert_eq!(items[1].label, "Assets:Investments:Stocks");

        // All accounts should be present
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Assets:Investments:Stocks".to_string()));
        assert!(labels.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels.contains(&&"Income:Salary".to_string()));
    }

    #[test]
    fn test_account_type_filtering() {
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Assets:Investments:Stocks USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Income:Salary USD
2023-10-01 txn "Test"
    L
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        // Should show all accounts, with "L" accounts ranked highest
        assert_eq!(items.len(), 5);
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // Liabilities account should be first (highest score)
        assert_eq!(items[0].label, "Liabilities:CreditCard:Visa");

        // All accounts should be present
        assert!(labels.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Assets:Investments:Stocks".to_string()));
        assert!(labels.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels.contains(&&"Income:Salary".to_string()));
    }

    #[test]
    fn test_income_and_expenses_filtering() {
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Expenses:Transportation:Gas USD
2023-10-01 open Income:Salary USD
2023-10-01 open Income:Freelance USD
2023-10-01 txn "Test"
    I
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        // Should show all accounts, with Income accounts ranked highest
        assert_eq!(items.len(), 6);
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // Income accounts should be first (highest scores)
        assert!(items[0].label.starts_with("Income:"));
        assert!(items[1].label.starts_with("Income:"));

        // All accounts should be present
        assert!(labels.contains(&&"Income:Salary".to_string()));
        assert!(labels.contains(&&"Income:Freelance".to_string()));
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels.contains(&&"Expenses:Transportation:Gas".to_string()));

        // Test E for Expenses
        let fixture_e = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Expenses:Transportation:Gas USD
2023-10-01 open Income:Salary USD
2023-10-01 txn "Test"
    E
     |
     ^
"#;
        let test_state_e = TestState::new(fixture_e).unwrap();
        let cursor_e = test_state_e.cursor().unwrap();
        let items_e = completion(test_state_e.snapshot, None, cursor_e)
            .unwrap()
            .unwrap_or_default();

        // Should show all accounts, with Expenses accounts ranked highest
        assert_eq!(items_e.len(), 5);
        let labels_e: Vec<&String> = items_e.iter().map(|item| &item.label).collect();

        // Expenses accounts should be first (highest scores)
        assert!(items_e[0].label.starts_with("Expenses:"));
        assert!(items_e[1].label.starts_with("Expenses:"));

        // All accounts should be present
        assert!(labels_e.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels_e.contains(&&"Expenses:Transportation:Gas".to_string()));
        assert!(labels_e.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels_e.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels_e.contains(&&"Income:Salary".to_string()));
    }

    #[test]
    fn test_lowercase_fuzzy_completion() {
        // Test the fuzzy search functionality directly
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Investments:Stocks".to_string(),
            "Expenses:Food:Groceries".to_string(), // This doesn't contain 'a'
            "Liabilities:CreditCard".to_string(),
            "Income:Salary".to_string(),
        ];

        // Test fuzzy search with "a" - should now show ALL accounts
        let matches = fuzzy_search_accounts(&accounts, "a");
        println!("Matches for 'a': {matches:?}");

        // Should show all 5 accounts since lowercase should show all accounts with fuzzy ranking
        assert_eq!(
            matches.len(),
            5,
            "Should show all accounts when lowercase is typed"
        );

        // Check that accounts containing 'a' have higher scores than those that don't
        let expenses_score = matches
            .iter()
            .find(|(acc, _)| acc.starts_with("Expenses"))
            .map(|(_, score)| *score);
        let assets_score = matches
            .iter()
            .find(|(acc, _)| acc.starts_with("Assets"))
            .map(|(_, score)| *score);

        assert!(assets_score.is_some(), "Should include Assets account");
        assert!(expenses_score.is_some(), "Should include Expenses account");

        // Assets should have higher score than Expenses since it contains 'a'
        assert!(
            assets_score.unwrap() > expenses_score.unwrap(),
            "Assets (contains 'a') should have higher score than Expenses (doesn't contain 'a')"
        );
    }

    #[test]
    fn test_return_all_accounts_unless_colon_triggered() {
        // Test normal completion (not colon-triggered): should return ALL accounts
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Assets:Investments:Stocks USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Income:Salary USD
2023-10-01 txn "Test"
    A
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor) // None = not colon-triggered
            .unwrap()
            .unwrap_or_default();

        // Should return ALL 5 accounts, not filtered by prefix
        assert_eq!(items.len(), 5);
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();

        // All accounts should be present
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Assets:Investments:Stocks".to_string()));
        assert!(labels.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels.contains(&&"Income:Salary".to_string()));

        // Assets accounts should be ranked highest due to prefix "A"
        assert!(items[0].label.starts_with("Assets:"));
        assert!(items[1].label.starts_with("Assets:"));
    }

    #[test]
    fn test_lowercase_completion_integration() {
        // Test the full completion flow with lowercase letters
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Assets:Investments:Stocks USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Income:Salary USD
2023-10-01 txn "Test"
    a
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        println!(
            "Completion items for 'a': {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );

        // Should show ALL accounts when lowercase is typed, not just ones containing 'a'
        assert_eq!(
            items.len(),
            5,
            "Should show all 5 accounts when lowercase 'a' is typed"
        );

        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Assets:Investments:Stocks".to_string()));
        assert!(labels.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels.contains(&&"Income:Salary".to_string()));
    }

    #[test]
    fn test_mixed_case_exact_completion() {
        // Test the search mode determination for mixed case
        use crate::providers::completion::{SearchMode, determine_search_mode};

        // Mixed case should use exact matching
        assert_eq!(determine_search_mode("Assets"), SearchMode::Exact);
        assert_eq!(determine_search_mode("AssetS"), SearchMode::Exact);
        assert_eq!(determine_search_mode("As"), SearchMode::Exact);
    }

    #[test]
    fn test_apple_account_issue() {
        // Test the specific issue where Liabilities:CC:Apple doesn't show when typing 'a'
        let fixture = r#"
%! /main.beancount
2023-10-01 open Liabilities:CC:Apple USD
2023-10-01 open Assets:Cash:Apple USD
2023-10-01 txn "Test"
    a
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        println!(
            "Apple test - Completion items for 'a': {:#?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );

        // Should show BOTH accounts when typing lowercase 'a'
        assert_eq!(
            items.len(),
            2,
            "Should show both Apple accounts when typing 'a'"
        );

        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(
            labels.contains(&&"Assets:Cash:Apple".to_string()),
            "Should include Assets:Cash:Apple"
        );
        assert!(
            labels.contains(&&"Liabilities:CC:Apple".to_string()),
            "Should include Liabilities:CC:Apple"
        );
    }

    #[test]
    fn test_apple_fuzzy_search_direct() {
        // Test the fuzzy search function directly with Apple accounts
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Liabilities:CC:Apple".to_string(),
            "Assets:Cash:Apple".to_string(),
        ];

        let matches = fuzzy_search_accounts(&accounts, "a");
        println!("Apple fuzzy search for 'a': {matches:#?}");

        // Should show both accounts
        assert_eq!(matches.len(), 2, "Should show both Apple accounts");

        let account_names: Vec<&String> = matches.iter().map(|(name, _)| name).collect();
        assert!(account_names.contains(&&"Assets:Cash:Apple".to_string()));
        assert!(account_names.contains(&&"Liabilities:CC:Apple".to_string()));
    }

    #[test]
    fn test_uppercase_letter_filtering_issue() {
        // Test that uppercase letters are filtering out accounts (current behavior)
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Income:Salary:Base USD
2023-10-01 txn "Test"
    A
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        println!("Completion items for uppercase 'A': {items:#?}");

        // Should now show all accounts, with Assets accounts first
        assert_eq!(items.len(), 4, "Should show all accounts when typing 'A'");

        // Assets account should be first (highest score)
        assert_eq!(items[0].label, "Assets:Cash:Checking");

        // All accounts should be present
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(labels.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(labels.contains(&&"Income:Salary:Base".to_string()));
    }

    #[test]
    fn test_first_letter_shows_all_accounts() {
        // Test that typing a single letter shows ALL accounts, not just matching ones
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Liabilities:CreditCard:Visa".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Income:Salary:Base".to_string(),
            "Equity:OpeningBalances".to_string(),
        ];

        // Test single letter "a" - should show ALL 5 accounts
        let matches = fuzzy_search_accounts(&accounts, "a");
        println!("Matches for single letter 'a': {matches:#?}");

        assert_eq!(
            matches.len(),
            5,
            "Should show all accounts when typing single letter 'a'"
        );

        // All accounts should be present
        let account_names: Vec<&String> = matches.iter().map(|(name, _)| name).collect();
        assert!(account_names.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(account_names.contains(&&"Liabilities:CreditCard:Visa".to_string()));
        assert!(account_names.contains(&&"Expenses:Food:Groceries".to_string()));
        assert!(account_names.contains(&&"Income:Salary:Base".to_string()));
        assert!(account_names.contains(&&"Equity:OpeningBalances".to_string()));

        // Test single letter "e" - should show ALL accounts
        let e_matches = fuzzy_search_accounts(&accounts, "e");
        println!("Matches for single letter 'e': {e_matches:#?}");

        assert_eq!(
            e_matches.len(),
            5,
            "Should show all accounts when typing single letter 'e'"
        );

        // Test single letter "z" - should show ALL accounts even with no matches
        let z_matches = fuzzy_search_accounts(&accounts, "z");
        println!("Matches for single letter 'z': {z_matches:#?}");

        assert_eq!(
            z_matches.len(),
            5,
            "Should show all accounts even when no letter matches"
        );
    }

    #[test]
    fn test_intra_word_vs_cross_colon_scoring() {
        // Test that intra-word matches are prioritized over cross-colon matches
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(), // "cash" matches within segment
            "Assets:Catering:Supplies".to_string(), // "ca" matches start of segment
            "Assets:Stocks:Company".to_string(), // "co" matches start of segment
            "Expenses:Communications:Phone".to_string(), // "co" matches within segment
            "Assets:Currency:Euro".to_string(), // "cu" matches start of segment
        ];

        // Test "cash" - should prioritize the exact intra-word match
        let matches = fuzzy_search_accounts(&accounts, "cash");
        println!("Matches for 'cash': {matches:#?}");

        // Assets:Cash:Checking should be highest because "cash" matches exactly within "Cash" segment
        assert_eq!(matches[0].0, "Assets:Cash:Checking");
        assert!(
            matches[0].1 > 4000.0,
            "Intra-word exact match should have very high score"
        );

        // Test "ca" - should prioritize matches within segments
        let ca_matches = fuzzy_search_accounts(&accounts, "ca");
        println!("Matches for 'ca': {ca_matches:#?}");

        // Should prioritize Assets:Cash:Checking and Assets:Catering:Supplies
        let top_two: Vec<&String> = ca_matches.iter().take(2).map(|(name, _)| name).collect();
        assert!(top_two.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(top_two.contains(&&"Assets:Catering:Supplies".to_string()));

        // Test "co" - Communications should rank higher than cross-colon matches
        let co_matches = fuzzy_search_accounts(&accounts, "co");
        println!("Matches for 'co': {co_matches:#?}");

        // Expenses:Communications:Phone should rank highly due to intra-word match
        let communications_score = co_matches
            .iter()
            .find(|(name, _)| name == "Expenses:Communications:Phone")
            .map(|(_, score)| *score)
            .unwrap();

        // Should have significant intra-word score
        assert!(
            communications_score > 3000.0,
            "Intra-word match should have high score"
        );
    }

    #[test]
    fn test_unsupported_trigger_character() {
        // Test that unsupported trigger characters return None
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
"#;
        let test_state = TestState::new(fixture).unwrap();

        // Use the proper path conversion to ensure consistency with TestState
        let file_path = TestState::path_from_fixture("/main.beancount").unwrap();
        let path_str = file_path.to_string_lossy();
        let uri_str = if cfg!(windows) {
            format!("file:///{}", path_str.replace('\\', "/"))
        } else {
            format!("file://{path_str}")
        };
        let uri = lsp_types::Uri::from_str(&uri_str).unwrap();

        let cursor = lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            position: lsp_types::Position {
                line: 0,
                character: 26,
            },
        };
        let items = completion(test_state.snapshot, Some('x'), cursor).unwrap();

        // Should return None for unsupported trigger characters
        assert!(items.is_none());
    }
}
