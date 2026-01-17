/// Beancount data extraction using tree-sitter queries
///
/// This module extracts structured data from parsed beancount files using tree-sitter's
/// query system. Queries are preferred over manual tree walking for better performance
/// and more declarative code.
///
/// # Query Patterns Used
///
/// ## Field Queries (Preferred)
/// Field queries extract specific named fields from nodes:
/// - `(open account: (account) @account)` - Extract account names from open directives
/// - `(transaction payee: (string) @payee)` - Extract payees from transactions
/// - `(transaction narration: (string) @narration)` - Extract narrations from transactions
///
/// ## Nested Queries
/// Queries that specify parent-child relationships:
/// - `(txn (flag) @flag)` - Extract transaction flags
/// - `(open (currency) @currency)` - Extract currencies from open directives
/// - `(commodity (currency) @currency)` - Extract currencies from commodity directives
///
/// ## Simple Node Queries
/// Match any occurrence of a node type:
/// - `(tag) @tag` - Extract all tags
/// - `(link) @link` - Extract all links
/// - `(currency) @currency` - Extract all currencies
///
/// # StreamingIterator Pattern
/// Tree-sitter queries return `QueryMatches` which implements `StreamingIterator`,
/// not the standard Rust `Iterator`. The pattern is:
/// ```ignore
/// let mut matches = cursor.matches(&query, root_node, text);
/// while let Some(qmatch) = matches.next() {
///     for capture in qmatch.captures {
///         // process capture.node
///     }
/// }
/// ```
///
/// # Performance Considerations
/// - Queries are compiled once and can be reused
/// - Field queries are more efficient than manual field access
/// - StreamingIterator avoids allocating a Vec of all matches
use crate::treesitter_utils::text_for_tree_sitter_node;
use std::sync::{Arc, OnceLock};
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

/// Static compiled queries for beancount data extraction.
/// Compiled once on first use and reused for all subsequent parses.
static UNIFIED_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();
static CURRENCY_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();
static NOTE_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();

/// Get or compile the unified query (tags, links, flags, accounts, transactions)
pub(crate) fn get_unified_query() -> &'static tree_sitter::Query {
    UNIFIED_QUERY.get_or_init(|| {
        let query_string = r#"
            (tag) @tag
            (link) @link
            (flag) @flag
            (open account: (account) @account)
            (transaction) @transaction
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .expect("Failed to compile unified query")
    })
}

/// Get or compile the currency query (open, commodity, all currencies)
fn get_currency_query() -> &'static tree_sitter::Query {
    CURRENCY_QUERY.get_or_init(|| {
        let query_string = r#"
            (open (currency) @currency)
            (commodity (currency) @currency)
            (currency) @currency
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .expect("Failed to compile currency query")
    })
}

/// Get or compile the note query (note directives with account and string)
fn get_note_query() -> &'static tree_sitter::Query {
    NOTE_QUERY.get_or_init(|| {
        let query_string = r#"
            (note account: (account) @account (string) @note)
            (note (account) @account (string) @note)
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .expect("Failed to compile note query")
    })
}

#[derive(Clone, Debug)]
pub struct FlaggedEntry {
    _file: String,
    pub line: u32,
}

//impl FlaggedEntry {
//    pub fn new(file: String, line: u32) -> Self {
//        Self { file, line }
//    }
//}

#[derive(Clone, Debug)]
pub struct BeancountData {
    accounts: Arc<Vec<String>>,
    payees: Arc<Vec<String>>,
    narration: Arc<Vec<String>>,
    pub flagged_entries: Vec<FlaggedEntry>,
    account_notes: Arc<std::collections::HashMap<String, Vec<String>>>,
    tags: Arc<Vec<String>>,
    links: Arc<Vec<String>>,
    commodities: Arc<Vec<String>>,
}

impl BeancountData {
    pub fn new(tree: &tree_sitter::Tree, content: &ropey::Rope) -> Self {
        let mut accounts = vec![];
        let mut payees = vec![];
        let mut narration = vec![];
        let mut flagged_entries = vec![];
        let mut account_notes: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        // Optimize string allocation - convert content to string once and reuse
        let content_str = content.to_string();
        let content_bytes = content_str.as_bytes();

        // Use unified query to extract accounts, transactions, tags, links, and flags in a single pass
        tracing::debug!("beancount_data:: executing unified query");
        let unified_query = get_unified_query();
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut matches = cursor_qry.matches(unified_query, tree.root_node(), content_bytes);

        // Get capture indices for efficient dispatch
        let tag_idx = unified_query
            .capture_index_for_name("tag")
            .expect("query should have 'tag' capture");
        let link_idx = unified_query
            .capture_index_for_name("link")
            .expect("query should have 'link' capture");
        let flag_idx = unified_query
            .capture_index_for_name("flag")
            .expect("query should have 'flag' capture");
        let account_idx = unified_query
            .capture_index_for_name("account")
            .expect("query should have 'account' capture");
        let transaction_idx = unified_query
            .capture_index_for_name("transaction")
            .expect("query should have 'transaction' capture");

        // Collections for frequency tracking
        let mut tags_set = std::collections::HashSet::new();
        let mut links_set = std::collections::HashSet::new();
        let mut payee_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut narration_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        // Process all matches in single tree traversal
        while let Some(qmatch) = matches.next() {
            for capture in qmatch.captures {
                match capture.index {
                    idx if idx == tag_idx => {
                        tags_set.insert(text_for_tree_sitter_node(content, &capture.node));
                    }
                    idx if idx == link_idx => {
                        links_set.insert(text_for_tree_sitter_node(content, &capture.node));
                    }
                    idx if idx == flag_idx => {
                        tracing::debug!("adding flag entry: {:?}", capture.node);
                        flagged_entries.push(FlaggedEntry {
                            _file: "".to_string(),
                            line: capture.node.start_position().row as u32,
                        });
                    }
                    idx if idx == account_idx => {
                        let account = text_for_tree_sitter_node(content, &capture.node);
                        accounts.push(account);
                    }
                    idx if idx == transaction_idx => {
                        // Extract payee/narration with same logic as before
                        let transaction = capture.node;
                        let mut txn_cursor = transaction.walk();

                        let mut payee_node = None;
                        let mut narration_node = None;

                        for child in transaction.children(&mut txn_cursor) {
                            match child.kind() {
                                "payee" => payee_node = Some(child),
                                "narration" => narration_node = Some(child),
                                _ => {}
                            }
                        }

                        // Process payee (with fallback to narration if no payee)
                        if let Some(payee) = payee_node {
                            let text = text_for_tree_sitter_node(content, &payee)
                                .trim()
                                .to_string();
                            if !text.is_empty() {
                                *payee_count.entry(text).or_insert(0) += 1;
                            }
                        } else if let Some(narration) = narration_node {
                            let text = text_for_tree_sitter_node(content, &narration)
                                .trim()
                                .to_string();
                            if !text.is_empty() {
                                *payee_count.entry(text).or_insert(0) += 1;
                            }
                        }

                        // Process narration
                        if let Some(narration) = narration_node {
                            let text = text_for_tree_sitter_node(content, &narration)
                                .trim()
                                .to_string();
                            if !text.is_empty() {
                                *narration_count.entry(text).or_insert(0) += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Convert sets to sorted vecs
        tracing::debug!("beancount_data:: processing tags and links");
        let mut tags: Vec<String> = tags_set.into_iter().collect();
        tags.sort();

        let mut links: Vec<String> = links_set.into_iter().collect();
        links.sort();

        // Sort payees and narrations by frequency
        tracing::debug!("beancount_data:: processing payees and narrations");
        let mut payee_vec: Vec<(String, usize)> = payee_count.into_iter().collect();
        payee_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        payees = payee_vec.into_iter().map(|(text, _)| text).collect();

        let mut narration_vec: Vec<(String, usize)> = narration_count.into_iter().collect();
        narration_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        narration = narration_vec.into_iter().map(|(text, _)| text).collect();

        // Extract commodities using unified currency query
        tracing::debug!("beancount_data:: get commodities");
        let currency_query = get_currency_query();
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut matches = cursor_qry.matches(currency_query, tree.root_node(), content_bytes);

        let mut commodities_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        while let Some(qmatch) = matches.next() {
            for capture in qmatch.captures {
                let commodity = text_for_tree_sitter_node(content, &capture.node)
                    .trim()
                    .to_string();

                if !commodity.is_empty() {
                    *commodities_count.entry(commodity).or_insert(0) += 1;
                }
            }
        }

        // Sort by frequency (most used first), then alphabetically
        let mut commodities: Vec<(String, usize)> = commodities_count.into_iter().collect();
        commodities.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let commodities: Vec<String> = commodities.into_iter().map(|(name, _)| name).collect();

        // Extract notes associated with accounts
        tracing::debug!("beancount_data:: get account notes");
        let note_query = get_note_query();
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut matches = cursor_qry.matches(note_query, tree.root_node(), content_bytes);

        let account_idx = note_query
            .capture_index_for_name("account")
            .expect("note query should have 'account' capture");
        let note_idx = note_query
            .capture_index_for_name("note")
            .expect("note query should have 'note' capture");

        while let Some(qmatch) = matches.next() {
            let mut account: Option<String> = None;
            let mut note: Option<String> = None;

            for capture in qmatch.captures {
                if capture.index == account_idx {
                    account = Some(text_for_tree_sitter_node(content, &capture.node));
                } else if capture.index == note_idx {
                    let raw = text_for_tree_sitter_node(content, &capture.node);
                    note = Some(clean_note_text(&raw));
                }
            }

            if let (Some(account), Some(note)) = (account, note)
                && !note.is_empty()
            {
                account_notes.entry(account).or_default().push(note);
            }
        }

        Self {
            accounts: Arc::new(accounts),
            payees: Arc::new(payees),
            narration: Arc::new(narration),
            flagged_entries,
            account_notes: Arc::new(account_notes),
            tags: Arc::new(tags),
            links: Arc::new(links),
            commodities: Arc::new(commodities),
        }
    }

    pub fn get_accounts(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.accounts)
    }

    pub fn get_payees(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.payees)
    }

    pub fn get_narration(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.narration)
    }

    pub fn get_account_notes(&self) -> Arc<std::collections::HashMap<String, Vec<String>>> {
        Arc::clone(&self.account_notes)
    }

    pub fn get_tags(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.tags)
    }

    pub fn get_links(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.links)
    }

    pub fn get_commodities(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.commodities)
    }
}

fn clean_note_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_query_compiled_once() {
        // Verify that get_unified_query returns the same query instance
        let q1 = get_unified_query();
        let q2 = get_unified_query();
        assert!(
            std::ptr::eq(q1, q2),
            "Unified query should be compiled once and reused"
        );
    }

    #[test]
    fn test_currency_query_compiled_once() {
        // Verify that get_currency_query returns the same query instance
        let q1 = get_currency_query();
        let q2 = get_currency_query();
        assert!(
            std::ptr::eq(q1, q2),
            "Currency query should be compiled once and reused"
        );
    }

    #[test]
    fn test_unified_query_has_all_captures() {
        // Verify all expected captures are present in unified query
        let query = get_unified_query();
        assert!(
            query.capture_index_for_name("tag").is_some(),
            "Unified query should have 'tag' capture"
        );
        assert!(
            query.capture_index_for_name("link").is_some(),
            "Unified query should have 'link' capture"
        );
        assert!(
            query.capture_index_for_name("flag").is_some(),
            "Unified query should have 'flag' capture"
        );
        assert!(
            query.capture_index_for_name("account").is_some(),
            "Unified query should have 'account' capture"
        );
        assert!(
            query.capture_index_for_name("transaction").is_some(),
            "Unified query should have 'transaction' capture"
        );
    }

    #[test]
    fn test_currency_query_has_currency_capture() {
        // Verify currency query has the currency capture
        let query = get_currency_query();
        assert!(
            query.capture_index_for_name("currency").is_some(),
            "Currency query should have 'currency' capture"
        );
    }

    #[test]
    fn test_beancount_data_extraction() {
        // Regression test: verify data extraction still works correctly
        let sample = r#"
2024-01-01 * "Payee" "Narration" #tag ^link
    Assets:Checking  100.00 USD
    Expenses:Food

2024-01-02 ! "Important transaction"
    Assets:Checking  50.00 EUR

2024-01-03 open Assets:Checking USD
2024-01-04 commodity EUR
        "#;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(sample, None).unwrap();
        let content = ropey::Rope::from_str(sample);

        let data = BeancountData::new(&tree, &content);

        // Verify accounts
        let accounts = data.get_accounts();
        assert!(
            accounts.contains(&"Assets:Checking".to_string()),
            "Should extract account from open directive"
        );

        // Verify tags
        let tags = data.get_tags();
        assert!(tags.contains(&"#tag".to_string()), "Should extract tags");

        // Verify links
        let links = data.get_links();
        assert!(links.contains(&"^link".to_string()), "Should extract links");

        // Verify flagged entries (one '!' flag)
        assert_eq!(
            data.flagged_entries.len(),
            1,
            "Should extract flagged entry"
        );
        assert_eq!(
            data.flagged_entries[0].line, 5,
            "Flagged entry should be on line 5"
        );

        // Verify commodities
        let commodities = data.get_commodities();
        assert!(
            commodities.contains(&"USD".to_string()),
            "Should extract USD from transactions and open"
        );
        assert!(
            commodities.contains(&"EUR".to_string()),
            "Should extract EUR from transactions and commodity"
        );

        // Verify payees
        let payees = data.get_payees();
        assert!(
            payees.contains(&"\"Payee\"".to_string())
                || payees.contains(&"\"Important transaction\"".to_string()),
            "Should extract payees"
        );

        // Verify narrations
        let narrations = data.get_narration();
        assert!(
            narrations.contains(&"\"Narration\"".to_string())
                || narrations.contains(&"\"Important transaction\"".to_string()),
            "Should extract narrations"
        );
    }

    #[test]
    fn test_arc_sharing() {
        // Verify that Arc::clone returns the same underlying data (pointer equality)
        let sample = r#"
2024-01-01 open Assets:Checking USD
2024-01-02 * "Payee" "Narration" #tag ^link
    Assets:Checking  100.00 USD
        "#;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(sample, None).unwrap();
        let content = ropey::Rope::from_str(sample);

        let data = BeancountData::new(&tree, &content);

        // Get the same data twice
        let accounts1 = data.get_accounts();
        let accounts2 = data.get_accounts();

        // Verify Arc pointer equality (same underlying data, no clone)
        assert!(
            Arc::ptr_eq(&accounts1, &accounts2),
            "Arc::clone should return the same underlying data (zero-copy)"
        );

        // Verify the data is correct
        assert_eq!(*accounts1, *accounts2, "Data should be identical");
    }

    #[test]
    fn test_all_getters_return_arc() {
        // Verify all getters work with Arc
        let sample = r#"
2024-01-01 open Assets:Checking USD
2024-01-02 * "Payee" "Narration" #tag ^link
    Assets:Checking  100.00 USD
2024-01-03 commodity EUR
        "#;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(sample, None).unwrap();
        let content = ropey::Rope::from_str(sample);

        let data = BeancountData::new(&tree, &content);

        // Verify all getters return Arc and can be dereferenced
        let accounts = data.get_accounts();
        assert!(!accounts.is_empty(), "Accounts should not be empty");

        let payees = data.get_payees();
        assert!(!payees.is_empty(), "Payees should not be empty");

        let narrations = data.get_narration();
        assert!(!narrations.is_empty(), "Narrations should not be empty");

        let tags = data.get_tags();
        assert!(!tags.is_empty(), "Tags should not be empty");

        let links = data.get_links();
        assert!(!links.is_empty(), "Links should not be empty");

        let commodities = data.get_commodities();
        assert!(!commodities.is_empty(), "Commodities should not be empty");
    }
}
