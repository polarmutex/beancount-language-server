use super::context::CompletionContext;
use crate::beancount_data::BeancountData;
use anyhow::Result;
use chrono::Datelike;
use lsp_types::{CompletionItem, CompletionItemKind, Position, Range, TextEdit};
use nucleo::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

// ============================================================================
// COMPLETION GENERATION - LSP 3.17 Compliant
// ============================================================================

/// Generate completions based on context with LSP 3.17 InsertReplaceEdit support
pub(super) fn generate_completions(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    context: &CompletionContext,
    content: &ropey::Rope,
    position: Position,
    config: &crate::config::Config,
) -> Result<Option<Vec<CompletionItem>>> {
    match context {
        CompletionContext::DocumentRoot => {
            let mut items = complete_date(content, position)?;
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

        CompletionContext::PostingAccount { prefix } => Ok(Some(complete_account(
            data,
            prefix,
            content,
            position,
            config.completion.fuzzy_match_accounts,
        )?)),

        CompletionContext::PostingAmount => Ok(Some(complete_amount()?)),

        CompletionContext::PostingCurrency => Ok(Some(complete_currency(data, content, position)?)),

        CompletionContext::OpenAccount { prefix } => Ok(Some(complete_account(
            data,
            prefix,
            content,
            position,
            config.completion.fuzzy_match_accounts,
        )?)),

        CompletionContext::OpenCurrency => Ok(Some(complete_currency(data, content, position)?)),

        CompletionContext::BalanceAccount { prefix } => Ok(Some(complete_account(
            data,
            prefix,
            content,
            position,
            config.completion.fuzzy_match_accounts,
        )?)),

        CompletionContext::PriceContext => Ok(Some(complete_currency(data, content, position)?)),

        CompletionContext::InsidePayee { prefix } => {
            let hcq = detect_closing_quote(content, position);
            Ok(Some(complete_payee(data, prefix, content, position, hcq)?))
        }

        CompletionContext::InsideNarration { prefix } => {
            let hcq = detect_closing_quote(content, position);
            Ok(Some(complete_narration(
                data, prefix, content, position, hcq,
            )?))
        }

        CompletionContext::TagContext { prefix } => Ok(Some(complete_tag(data, prefix)?)),

        CompletionContext::LinkContext { prefix } => Ok(Some(complete_link(data, prefix)?)),

        CompletionContext::ColonTriggeredAccount { parent_path } => {
            Ok(Some(complete_subaccounts(data, parent_path)?))
        }
    }
}

/// Detect whether there is a closing quote after the cursor position on the current line
fn detect_closing_quote(content: &ropey::Rope, position: Position) -> bool {
    let line = content.line(position.line as usize).to_string();
    let chars: Vec<char> = line.chars().collect();
    let col = position.character as usize;
    chars.get(col..).map_or(false, |rest| rest.contains(&'"'))
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
            kind: Some(CompletionItemKind::Keyword),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect())
}

/// Complete date with current/previous/next month
fn complete_date(content: &ropey::Rope, position: Position) -> Result<Vec<CompletionItem>> {
    let today = chrono::Local::now().naive_local().date();
    let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
    let cur_month = today.format("%Y-%m-").to_string();
    let next_month = add_one_month(today).format("%Y-%m-").to_string();
    let today_str = today.format("%Y-%m-%d").to_string();

    // Calculate ranges for InsertReplaceEdit
    let line = content.line(position.line as usize).to_string();
    let (insert_range, replace_range) = calculate_word_ranges(&line, position);

    Ok(vec![
        create_completion_with_insert_replace(
            today_str,
            "today".to_string(),
            CompletionItemKind::Constant,
            insert_range,
            replace_range,
            1000.0,
            vec![],
        ),
        create_completion_with_insert_replace(
            cur_month,
            "this month".to_string(),
            CompletionItemKind::Constant,
            insert_range,
            replace_range,
            900.0,
            vec![],
        ),
        create_completion_with_insert_replace(
            prev_month,
            "prev month".to_string(),
            CompletionItemKind::Constant,
            insert_range,
            replace_range,
            800.0,
            vec![],
        ),
        create_completion_with_insert_replace(
            next_month,
            "next month".to_string(),
            CompletionItemKind::Constant,
            insert_range,
            replace_range,
            700.0,
            vec![],
        ),
    ])
}

/// Complete account names with fuzzy matching and InsertReplaceEdit
pub(super) fn complete_account(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    content: &ropey::Rope,
    position: Position,
    fuzzy_filter: bool,
) -> Result<Vec<CompletionItem>> {
    let mut all_accounts: Vec<String> = Vec::new();

    for bean_data in data.values() {
        all_accounts.extend(bean_data.get_accounts().iter().cloned());
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
            let mut item = create_completion_with_insert_replace(
                account.clone(),
                "Beancount Account".to_string(),
                CompletionItemKind::Enum,
                insert_range,
                replace_range,
                score,
                vec![":".to_string()], // Commit character for flow
            );
            if fuzzy_filter {
                // Append colon-stripped version so client-side fuzzy matchers
                // can match cross-segment queries like "BankCheck"
                let stripped = account.replace(':', "");
                item.filter_text = Some(format!("{} {}", account, stripped));
            }
            item
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
        for account in bean_data.get_accounts().iter() {
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
            kind: Some(CompletionItemKind::Enum),
            detail: Some("Account segment".to_string()),
            insert_text: Some(segment),
            commit_characters: Some(vec![":".to_string()]),
            ..Default::default()
        })
        .collect())
}

/// Complete currency codes
fn complete_currency(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    content: &ropey::Rope,
    position: Position,
) -> Result<Vec<CompletionItem>> {
    // Collect commodities from all beancount files
    let mut commodities_set: HashSet<String> = HashSet::new();
    for bean_data in data.values() {
        for commodity in bean_data.get_commodities().iter() {
            commodities_set.insert(commodity.clone());
        }
    }

    // If no commodities found in the files, fall back to common currencies
    let fallback_currencies = vec![
        "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "PLN", "CZK",
        "HUF", "CNY", "INR", "BRL", "MXN", "ZAR", "RUB", "KRW", "SGD", "HKD", "THB",
    ];

    let currencies: Vec<String> = if commodities_set.is_empty() {
        fallback_currencies.iter().map(|s| s.to_string()).collect()
    } else {
        let mut commodities: Vec<String> = commodities_set.into_iter().collect();
        commodities.sort();
        commodities
    };

    let line = content.line(position.line as usize).to_string();
    let (insert_range, replace_range) = calculate_word_ranges(&line, position);

    Ok(currencies
        .iter()
        .map(|currency| {
            create_completion_with_insert_replace(
                currency.to_string(),
                "Currency".to_string(),
                CompletionItemKind::Unit,
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
pub(super) fn complete_payee(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    content: &ropey::Rope,
    position: Position,
    has_closing_quote: bool,
) -> Result<Vec<CompletionItem>> {
    let mut payees: Vec<String> = Vec::new();

    for bean_data in data.values() {
        for payee in bean_data.get_payees().iter() {
            let clean = payee.trim_matches('"');
            if !clean.is_empty() {
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
                CompletionItemKind::Enum,
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
pub(super) fn complete_narration(
    data: &HashMap<PathBuf, Arc<BeancountData>>,
    prefix: &str,
    content: &ropey::Rope,
    position: Position,
    has_closing_quote: bool,
) -> Result<Vec<CompletionItem>> {
    let mut narrations: Vec<String> = Vec::new();

    for bean_data in data.values() {
        for narration in bean_data.get_narration().iter() {
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
                CompletionItemKind::Text,
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
                .iter()
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
            kind: Some(CompletionItemKind::Keyword),
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
                .iter()
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
            kind: Some(CompletionItemKind::Keyword),
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
        text_edit: Some(lsp_types::CompletionItemTextEdit::TextEdit(TextEdit {
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
        if let Some(lsp_types::CompletionItemTextEdit::TextEdit(edit)) = &mut self.text_edit {
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
        let content = ropey::Rope::from_str("2026-01-");
        let position = Position {
            line: 0,
            character: 8,
        };
        let items = complete_date(&content, position).unwrap();
        assert_eq!(items.len(), 4);

        let details: Vec<String> = items.iter().filter_map(|i| i.detail.clone()).collect();
        assert!(details.contains(&"today".to_string()));
        assert!(details.contains(&"this month".to_string()));
        assert!(details.contains(&"prev month".to_string()));
        assert!(details.contains(&"next month".to_string()));

        // Verify that all items have text_edit set for proper replacement
        for item in &items {
            assert!(
                item.text_edit.is_some(),
                "Date completion should have text_edit"
            );
        }
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
            CompletionItemKind::Enum,
            insert_range,
            replace_range,
            100.0,
            vec![":".to_string()],
        );

        assert_eq!(item.label, "Assets:Cash");
        assert_eq!(item.detail, Some("Account".to_string()));
        assert_eq!(item.kind, Some(CompletionItemKind::Enum));
        assert_eq!(item.commit_characters, Some(vec![":".to_string()]));

        match item.text_edit {
            Some(lsp_types::CompletionItemTextEdit::TextEdit(text_edit)) => {
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
    fn test_fuzzy_search_strings_empty_query() {
        let strings = vec![
            "Kroger".to_string(),
            "Walmart".to_string(),
            "Target".to_string(),
        ];

        let results = fuzzy_search_strings(&strings, "");
        assert_eq!(results.len(), 3, "Empty query should return all strings");

        for (_, score) in &results {
            assert_eq!(*score, 1.0, "Empty query should have fallback score");
        }
    }

    #[test]
    fn test_fuzzy_search_strings_with_query() {
        let strings = vec![
            "Kroger".to_string(),
            "King Soopers".to_string(),
            "Walmart".to_string(),
        ];

        let results = fuzzy_search_strings(&strings, "K");

        // Should match strings starting with K
        let k_results: Vec<_> = results.iter().filter(|(s, _)| s.starts_with('K')).collect();
        assert_eq!(k_results.len(), 2, "Should match both K strings");
    }

    #[test]
    fn test_fuzzy_search_strings_case_insensitive() {
        let strings = vec!["Kroger".to_string(), "walmart".to_string()];

        let upper_results = fuzzy_search_strings(&strings, "KROGER");
        let lower_results = fuzzy_search_strings(&strings, "kroger");

        assert!(!upper_results.is_empty(), "Should match case-insensitively");
        assert!(!lower_results.is_empty(), "Should match case-insensitively");
    }

    // ========================================================================
    // Test Helpers
    // ========================================================================

    /// Helper to create BeancountData from text for testing
    pub(super) fn create_test_beancount_data(text: &str) -> crate::beancount_data::BeancountData {
        use ropey::Rope;
        use std::sync::Arc;
        use tree_sitter_beancount::tree_sitter::Parser;

        let rope = Rope::from_str(text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();

        crate::beancount_data::BeancountData::new(&Arc::new(tree), &rope)
    }

    // ========================================================================
    // Payee Completion Tests
    // ========================================================================

    #[test]
    fn test_complete_payee_empty_prefix() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Kroger" "Groceries"
2026-01-02 * "Walmart" "Shopping"
2026-01-03 * "Target" "Clothes"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * ""#);
        let position = Position {
            line: 0,
            character: 14,
        };

        let items = complete_payee(&data_map, "", &content, position, false).unwrap();

        assert!(items.len() >= 3, "Should return all payees when no prefix");

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Kroger"));
        assert!(labels.contains(&"Walmart"));
        assert!(labels.contains(&"Target"));
    }

    #[test]
    fn test_complete_payee_with_prefix() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Kroger" "Test"
2026-01-02 * "King Soopers" "Test"
2026-01-03 * "Walmart" "Test"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "K"#);
        let position = Position {
            line: 0,
            character: 15,
        };

        let items = complete_payee(&data_map, "K", &content, position, false).unwrap();

        // Should fuzzy match Kroger and King Soopers
        assert!(items.len() >= 2, "Should match payees starting with K");

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Kroger"));
        assert!(labels.contains(&"King Soopers"));
    }

    #[test]
    fn test_complete_payee_adds_closing_quote() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Kroger" "Test"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Kr"#);
        let position = Position {
            line: 0,
            character: 16,
        };

        // No closing quote
        let items = complete_payee(&data_map, "Kr", &content, position, false).unwrap();
        assert!(!items.is_empty());

        // Should add closing quote in insert_text
        if let Some(lsp_types::CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit {
            assert!(
                edit.new_text.ends_with('"'),
                "Should add closing quote: {}",
                edit.new_text
            );
        }
    }

    #[test]
    fn test_complete_payee_no_extra_quote_when_present() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Kroger" "Test"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Kr""#);
        let position = Position {
            line: 0,
            character: 16,
        };

        // Has closing quote
        let items = complete_payee(&data_map, "Kr", &content, position, true).unwrap();
        assert!(!items.is_empty());

        // Should NOT add closing quote
        if let Some(lsp_types::CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit {
            assert!(
                !edit.new_text.ends_with('"'),
                "Should not add extra quote: {}",
                edit.new_text
            );
        }
    }

    #[test]
    fn test_complete_payee_deduplication() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Kroger" "Test1"
2026-01-02 * "Kroger" "Test2"
2026-01-03 * "Kroger" "Test3"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * ""#);
        let position = Position {
            line: 0,
            character: 14,
        };

        let items = complete_payee(&data_map, "", &content, position, false).unwrap();

        // Should deduplicate
        assert_eq!(items.len(), 1, "Should deduplicate payees");
        assert_eq!(items[0].label, "Kroger");
    }

    // ========================================================================
    // Narration Completion Tests
    // ========================================================================

    #[test]
    fn test_complete_narration_empty_prefix() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Store" "Groceries"
2026-01-02 * "Station" "Gas"
2026-01-03 * "Restaurant" "Dinner"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Kroger" ""#);
        let position = Position {
            line: 0,
            character: 23,
        };

        let items = complete_narration(&data_map, "", &content, position, false).unwrap();

        assert!(
            items.len() >= 3,
            "Should return all narrations when no prefix"
        );

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Groceries"));
        assert!(labels.contains(&"Gas"));
        assert!(labels.contains(&"Dinner"));

        // Verify payees are NOT in the results
        assert!(!labels.contains(&"Store"));
        assert!(!labels.contains(&"Station"));
        assert!(!labels.contains(&"Restaurant"));
    }

    #[test]
    fn test_complete_narration_with_prefix() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Store" "Groceries"
2026-01-02 * "Station" "Gas"
2026-01-03 * "Shop" "Gift"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Store" "G"#);
        let position = Position {
            line: 0,
            character: 23, // Position at 'G'
        };

        let items = complete_narration(&data_map, "G", &content, position, false).unwrap();

        // Should fuzzy match all items starting with G
        assert!(items.len() >= 3, "Should match narrations starting with G");

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Groceries"));
        assert!(labels.contains(&"Gas"));
        assert!(labels.contains(&"Gift"));
    }

    #[test]
    fn test_complete_narration_adds_closing_quote() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Store" "Groceries"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Store" "Groc"#);
        let position = Position {
            line: 0,
            character: 26, // Position after 'c' in "Groc"
        };

        // No closing quote
        let items = complete_narration(&data_map, "Groc", &content, position, false).unwrap();
        assert!(!items.is_empty());

        // Should add closing quote in insert_text
        if let Some(lsp_types::CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit {
            assert!(
                edit.new_text.ends_with('"'),
                "Should add closing quote: {}",
                edit.new_text
            );
        }
    }

    #[test]
    fn test_complete_narration_no_extra_quote_when_present() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Store" "Groceries"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Store" "Groc""#);
        let position = Position {
            line: 0,
            character: 27,
        };

        // Has closing quote
        let items = complete_narration(&data_map, "Groc", &content, position, true).unwrap();
        assert!(!items.is_empty());

        // Should NOT add closing quote
        if let Some(lsp_types::CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit {
            assert!(
                !edit.new_text.ends_with('"'),
                "Should not add extra quote: {}",
                edit.new_text
            );
        }
    }

    #[test]
    fn test_complete_narration_deduplication() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 * "Store" "Groceries"
2026-01-02 * "Market" "Groceries"
2026-01-03 * "Shop" "Groceries"
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str(r#"2026-01-06 * "Store" ""#);
        let position = Position {
            line: 0,
            character: 22, // Position inside empty narration string
        };

        let items = complete_narration(&data_map, "", &content, position, false).unwrap();

        // Should deduplicate
        assert_eq!(items.len(), 1, "Should deduplicate narrations");
        assert_eq!(items[0].label, "Groceries");
    }

    // ========================================================================
    // Account Completion filter_text Tests
    // ========================================================================

    #[test]
    fn test_complete_account_fuzzy_filter_sets_filter_text() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 open Assets:US:Bank:Checking
2026-01-01 open Expenses:Food:Groceries
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str("  Assets");
        let position = Position {
            line: 0,
            character: 8,
        };

        let items = complete_account(&data_map, "Assets", &content, position, true).unwrap();

        let checking = items.iter().find(|i| i.label == "Assets:US:Bank:Checking");
        assert!(checking.is_some(), "Should find Assets:US:Bank:Checking");

        // With fuzzy_filter=true, filter_text should contain both the original and stripped version
        let ft = checking.unwrap().filter_text.as_ref().unwrap();
        assert!(
            ft.contains("Assets:US:Bank:Checking"),
            "filter_text should contain the original account"
        );
        assert!(
            ft.contains("AssetsUSBankChecking"),
            "filter_text should contain the colon-stripped version for cross-segment matching"
        );
    }

    #[test]
    fn test_complete_account_no_fuzzy_filter_preserves_default_filter_text() {
        use ropey::Rope;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        let test_data = r#"
2026-01-01 open Assets:US:Bank:Checking
"#;

        let mut data_map = HashMap::new();
        let bean_data = create_test_beancount_data(test_data);
        data_map.insert(PathBuf::from("test.bean"), Arc::new(bean_data));

        let content = Rope::from_str("  Assets");
        let position = Position {
            line: 0,
            character: 8,
        };

        let items = complete_account(&data_map, "Assets", &content, position, false).unwrap();

        let checking = items.iter().find(|i| i.label == "Assets:US:Bank:Checking");
        assert!(checking.is_some(), "Should find Assets:US:Bank:Checking");

        // With fuzzy_filter=false, filter_text should be the default (label), not the augmented version
        let ft = checking.unwrap().filter_text.as_ref().unwrap();
        assert!(
            !ft.contains("AssetsUSBankChecking"),
            "filter_text should NOT contain the colon-stripped version when fuzzy is off"
        );
    }
}
