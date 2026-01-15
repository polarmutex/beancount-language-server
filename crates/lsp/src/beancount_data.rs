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
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

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
    accounts: Vec<String>,
    payees: Vec<String>,
    narration: Vec<String>,
    pub flagged_entries: Vec<FlaggedEntry>,
    tags: Vec<String>,
    links: Vec<String>,
    commodities: Vec<String>,
}

impl BeancountData {
    pub fn new(tree: &tree_sitter::Tree, content: &ropey::Rope) -> Self {
        let mut accounts = vec![];
        let mut payees = vec![];
        let mut narration = vec![];
        let mut flagged_entries = vec![];

        // Update account opens using field query
        tracing::debug!("beancount_data:: get account nodes");
        let query_string = r#"
        (open account: (account) @account)
        "#;
        let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .unwrap_or_else(|_| panic!("Invalid query for accounts: {query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let mut matches = cursor_qry.matches(&query, tree.root_node(), binding.as_bytes());

        tracing::debug!("beancount_data:: update accounts");
        accounts.clear();

        while let Some(qmatch) = matches.next() {
            for capture in qmatch.captures {
                let account = text_for_tree_sitter_node(content, &capture.node);
                accounts.push(account);
            }
        }

        // Update payees and narration with frequency tracking
        // Note: Using manual tree walking here because we need per-transaction logic:
        // - Transactions with payee field: add payee only
        // - Transactions with only narration (single-string): add narration to both payees and narrations
        tracing::debug!("beancount_data:: get payee and narration nodes");
        let mut payee_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut narration_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        // Query for all transaction nodes
        let transaction_query_string = r#"
        (transaction) @transaction
        "#;
        let transaction_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), transaction_query_string)
                .unwrap_or_else(|_| {
                    panic!("Invalid query for transactions: {transaction_query_string}")
                });
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let mut transaction_matches =
            cursor_qry.matches(&transaction_query, tree.root_node(), binding.as_bytes());

        while let Some(qmatch) = transaction_matches.next() {
            for capture in qmatch.captures {
                let transaction = capture.node;
                let mut txn_cursor = transaction.walk();

                // Check for payee and narration nodes within this transaction
                let mut payee_node = None;
                let mut narration_node = None;

                for child in transaction.children(&mut txn_cursor) {
                    match child.kind() {
                        "payee" => payee_node = Some(child),
                        "narration" => narration_node = Some(child),
                        _ => {}
                    }
                }

                // When there's a payee field (two strings), use it
                if let Some(payee) = payee_node {
                    let payee_text = text_for_tree_sitter_node(content, &payee)
                        .trim()
                        .to_string();
                    if !payee_text.is_empty() {
                        *payee_count.entry(payee_text).or_insert(0) += 1;
                    }
                }
                // When there's only narration (one string), also add it to payees
                // since semantically it often represents the payee
                else if let Some(narration) = narration_node {
                    let narration_text = text_for_tree_sitter_node(content, &narration)
                        .trim()
                        .to_string();
                    if !narration_text.is_empty() {
                        // Add single-string transactions to payees for completion
                        *payee_count.entry(narration_text).or_insert(0) += 1;
                    }
                }

                // Always collect narration for narration completions
                if let Some(narration) = narration_node {
                    let narration_text = text_for_tree_sitter_node(content, &narration)
                        .trim()
                        .to_string();
                    if !narration_text.is_empty() {
                        *narration_count.entry(narration_text).or_insert(0) += 1;
                    }
                }
            }
        }

        tracing::debug!("beancount_data:: update payees");
        payees.clear();

        // Sort by frequency (most used first), then alphabetically
        let mut payee_vec: Vec<(String, usize)> = payee_count.into_iter().collect();
        payee_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        payees = payee_vec.into_iter().map(|(text, _)| text).collect();

        tracing::debug!("beancount_data:: update narration");
        narration.clear();

        // Sort by frequency (most used first), then alphabetically
        let mut narration_vec: Vec<(String, usize)> = narration_count.into_iter().collect();
        narration_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        narration = narration_vec.into_iter().map(|(text, _)| text).collect();

        // Update flagged entries using query
        tracing::debug!("beancount_data:: update flagged entries");
        flagged_entries.clear();

        let flag_query_string = r#"
        (flag) @flag
        "#;
        let flag_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), flag_query_string)
                .unwrap_or_else(|_| panic!("Invalid query for flags: {flag_query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut flag_matches =
            cursor_qry.matches(&flag_query, tree.root_node(), binding.as_bytes());

        while let Some(qmatch) = flag_matches.next() {
            for capture in qmatch.captures {
                tracing::debug!("adding flag entry: {:?}", capture.node);
                flagged_entries.push(FlaggedEntry {
                    _file: "".to_string(),
                    line: capture.node.start_position().row as u32,
                });
            }
        }

        // Update tags
        tracing::debug!("beancount_data:: get tags");
        let query_string = r#"
        (tag) @tag
        "#;
        let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .unwrap_or_else(|_| panic!("get_position_by_query invalid query {query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let mut matches = cursor_qry.matches(&query, tree.root_node(), binding.as_bytes());
        let mut tags: Vec<_> = {
            let mut results = Vec::new();
            while let Some(m) = matches.next() {
                for capture in m.captures {
                    results.push(text_for_tree_sitter_node(content, &capture.node));
                }
            }
            results
        };
        tags.sort();
        tags.dedup();

        // Update links
        tracing::debug!("beancount_data:: get tags");
        let query_string = r#"
        (link) @link
        "#;
        let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .unwrap_or_else(|_| panic!("get_position_by_query invalid query {query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let mut matches = cursor_qry.matches(&query, tree.root_node(), binding.as_bytes());
        let mut links: Vec<_> = {
            let mut results = Vec::new();
            while let Some(m) = matches.next() {
                for capture in m.captures {
                    results.push(text_for_tree_sitter_node(content, &capture.node));
                }
            }
            results
        };
        links.sort();
        links.dedup();

        // Update commodities with usage frequency
        tracing::debug!("beancount_data:: get commodities");
        let mut commodities_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        // Get commodities from open directives using query (count once per directive)
        let open_currency_query_string = r#"
        (open (currency) @currency)
        "#;
        let open_currency_query = tree_sitter::Query::new(
            &tree_sitter_beancount::language(),
            open_currency_query_string,
        )
        .unwrap_or_else(|_| {
            panic!("Invalid query for open currencies: {open_currency_query_string}")
        });
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut open_currency_matches =
            cursor_qry.matches(&open_currency_query, tree.root_node(), binding.as_bytes());

        while let Some(qmatch) = open_currency_matches.next() {
            for capture in qmatch.captures {
                let commodity = text_for_tree_sitter_node(content, &capture.node)
                    .trim()
                    .to_string();
                if !commodity.is_empty() {
                    *commodities_count.entry(commodity).or_insert(0) += 1;
                }
            }
        }

        // Get commodities from commodity directives using query (count once per directive)
        let commodity_currency_query_string = r#"
        (commodity (currency) @currency)
        "#;
        let commodity_currency_query = tree_sitter::Query::new(
            &tree_sitter_beancount::language(),
            commodity_currency_query_string,
        )
        .unwrap_or_else(|_| {
            panic!("Invalid query for commodity currencies: {commodity_currency_query_string}")
        });
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut commodity_currency_matches = cursor_qry.matches(
            &commodity_currency_query,
            tree.root_node(),
            binding.as_bytes(),
        );

        while let Some(qmatch) = commodity_currency_matches.next() {
            for capture in qmatch.captures {
                let commodity = text_for_tree_sitter_node(content, &capture.node)
                    .trim()
                    .to_string();
                if !commodity.is_empty() {
                    *commodities_count.entry(commodity).or_insert(0) += 1;
                }
            }
        }

        // Get commodities from transaction postings (count each usage)
        let query_string = r#"
        (currency) @currency
        "#;
        let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
            .unwrap_or_else(|_| panic!("get_position_by_query invalid query {query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let mut matches = cursor_qry.matches(&query, tree.root_node(), binding.as_bytes());
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let commodity = text_for_tree_sitter_node(content, &capture.node)
                    .trim()
                    .to_string();
                if !commodity.is_empty() {
                    *commodities_count.entry(commodity).or_insert(0) += 1;
                }
            }
        }

        // Convert to vec and sort by frequency (most used first), then alphabetically
        let mut commodities: Vec<(String, usize)> = commodities_count.into_iter().collect();
        commodities.sort_by(|a, b| {
            // First sort by count (descending), then by name (ascending)
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });
        let commodities: Vec<String> = commodities.into_iter().map(|(name, _)| name).collect();

        Self {
            accounts,
            payees,
            narration,
            flagged_entries,
            tags,
            links,
            commodities,
        }
    }

    pub fn get_accounts(&self) -> Vec<String> {
        self.accounts.clone()
    }

    pub fn get_payees(&self) -> Vec<String> {
        self.payees.clone()
    }

    pub fn get_narration(&self) -> Vec<String> {
        self.narration.clone()
    }

    pub fn get_tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    pub fn get_links(&self) -> Vec<String> {
        self.links.clone()
    }

    pub fn get_commodities(&self) -> Vec<String> {
        self.commodities.clone()
    }
}
