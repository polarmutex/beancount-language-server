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

        let mut cursor = tree.root_node().walk();

        // Update account opens
        tracing::debug!("beancount_data:: get account nodes");
        tracing::debug!("beancount_data:: get account strings");
        let account_strings = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "open")
            .filter_map(|node| {
                let mut node_cursor = node.walk();
                let account_node = node
                    .children(&mut node_cursor)
                    .find(|c| c.kind() == "account")?;
                let account = text_for_tree_sitter_node(content, &account_node);
                Some(account)
            });

        tracing::debug!("beancount_data:: update accounts");
        accounts.clear();

        for account in account_strings {
            accounts.push(account);
        }

        // Update payees and narration with frequency tracking
        tracing::debug!("beancount_data:: get payee and narration nodes");
        let transactions = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "transaction")
            .collect::<Vec<_>>();

        let mut payee_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut narration_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for transaction in transactions {
            // When there's a payee field (two strings), use it
            if let Some(payee_node) = transaction.child_by_field_name("payee") {
                let payee_text = text_for_tree_sitter_node(content, &payee_node)
                    .trim()
                    .to_string();
                if !payee_text.is_empty() {
                    *payee_count.entry(payee_text).or_insert(0) += 1;
                }
            }
            // When there's only narration (one string), also add it to payees
            // since semantically it often represents the payee
            else if let Some(narration_node) = transaction.child_by_field_name("narration") {
                let narration_text = text_for_tree_sitter_node(content, &narration_node)
                    .trim()
                    .to_string();
                if !narration_text.is_empty() {
                    // Add single-string transactions to payees for completion
                    *payee_count.entry(narration_text.clone()).or_insert(0) += 1;
                }
            }

            // Always collect narration for narration completions
            if let Some(narration_node) = transaction.child_by_field_name("narration") {
                let narration_text = text_for_tree_sitter_node(content, &narration_node)
                    .trim()
                    .to_string();
                if !narration_text.is_empty() {
                    *narration_count.entry(narration_text).or_insert(0) += 1;
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

        // Update flagged entries
        tracing::debug!("beancount_data:: update flagged entries");
        flagged_entries.clear();

        tree.root_node()
            .children(&mut cursor)
            .filter(|c| {
                let txn_node = c.child_by_field_name("txn");
                if let Some(txn_node) = txn_node {
                    let txn_child_node = txn_node.child(0);
                    txn_child_node.is_some() && txn_child_node.unwrap().kind() == "flag"
                } else {
                    false
                }
            })
            .for_each(|node| {
                let mut node_cursor = node.walk();
                let txn_node = node.children(&mut node_cursor).find(|c| c.kind() == "txn");
                if let Some(txn_node) = txn_node {
                    let mut flag_cursor = txn_node.walk();
                    let flag_node = txn_node
                        .children(&mut flag_cursor)
                        .find(|c| c.kind() == "flag");
                    if let Some(flag) = flag_node {
                        tracing::debug!("addind flag entry: {:?}", flag);
                        flagged_entries.push(FlaggedEntry {
                            _file: "".to_string(),
                            line: flag.start_position().row as u32,
                        });
                    }
                }
            });

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

        // Get commodities from open directives (count once per directive)
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "open")
            .for_each(|node| {
                let mut node_cursor = node.walk();
                // Look for currency nodes in open directives
                for child in node.children(&mut node_cursor) {
                    if child.kind() == "currency" {
                        let commodity = text_for_tree_sitter_node(content, &child)
                            .trim()
                            .to_string();
                        if !commodity.is_empty() {
                            *commodities_count.entry(commodity).or_insert(0) += 1;
                        }
                    }
                }
            });

        // Get commodities from commodity directives (count once per directive)
        let mut cursor = tree.root_node().walk();
        tree.root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "commodity")
            .for_each(|node| {
                let mut node_cursor = node.walk();
                for child in node.children(&mut node_cursor) {
                    if child.kind() == "currency" {
                        let commodity = text_for_tree_sitter_node(content, &child)
                            .trim()
                            .to_string();
                        if !commodity.is_empty() {
                            *commodities_count.entry(commodity).or_insert(0) += 1;
                        }
                    }
                }
            });

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
