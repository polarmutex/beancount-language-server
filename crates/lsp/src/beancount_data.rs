use crate::treesitter_utils::text_for_tree_sitter_node;
use std::collections::HashSet;

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
    narration: Vec<String>,
    pub flagged_entries: Vec<FlaggedEntry>,
    tags: Vec<String>,
    links: Vec<String>,
}

impl BeancountData {
    pub fn new(tree: &tree_sitter::Tree, content: &ropey::Rope) -> Self {
        let mut accounts = vec![];
        let mut narration = vec![];
        let mut flagged_entries = vec![];

        let mut cursor = tree.root_node().walk();

        // Update account opens
        // tracing::debug!("beancount_data:: get account nodes");
        // tracing::debug!("beancount_data:: get account strings");
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

        // tracing::debug!("beancount_data:: update accounts");
        accounts.clear();

        for account in account_strings {
            tracing::info!(account);
            accounts.push(account);
        }

        // Update account opens
        // tracing::debug!("beancount_data:: get narration nodes");
        // tracing::debug!("beancount_data:: get account strings");
        let transactions = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "transaction")
            .collect::<Vec<_>>();

        //TODO: consider doing something silimar with others around
        let mut txn_string_strings: HashSet<String> = HashSet::new();
        for transaction in transactions {
            if let Some(narration) = transaction.child_by_field_name("narration") {
                txn_string_strings.insert(
                    text_for_tree_sitter_node(content, &narration)
                        .trim()
                        .to_string(),
                );
            }
        }

        // tracing::debug!("beancount_data:: update narration");
        narration.clear();

        for txn_string in txn_string_strings {
            if !narration.contains(&txn_string) {
                narration.push(txn_string);
            }
        }

        // Update flagged entries
        // tracing::debug!("beancount_data:: update flagged entries");
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
                        // tracing::debug!("addind flag entry: {:?}", flag);
                        flagged_entries.push(FlaggedEntry {
                            _file: "".to_string(),
                            line: flag.start_position().row as u32,
                        });
                    }
                }
            });

        // Update tags
        // tracing::debug!("beancount_data:: get tags");
        let query_string = r#"
        (tag) @tag
        "#;
        let query = tree_sitter::Query::new(tree_sitter_beancount::language(), query_string)
            .unwrap_or_else(|_| panic!("get_position_by_query invalid query {query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let matches = cursor_qry.matches(&query, tree.root_node(), binding.as_bytes());
        let mut tags: Vec<_> = matches
            .into_iter()
            .flat_map(|m| {
                m.captures
                    .iter()
                    .map(|capture| text_for_tree_sitter_node(content, &capture.node))
            })
            .collect();
        tags.sort();
        tags.dedup();

        // Update links
        // tracing::debug!("beancount_data:: get tags");
        let query_string = r#"
        (link) @link
        "#;
        let query = tree_sitter::Query::new(tree_sitter_beancount::language(), query_string)
            .unwrap_or_else(|_| panic!("get_position_by_query invalid query {query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let binding = content.clone().to_string();
        let matches = cursor_qry.matches(&query, tree.root_node(), binding.as_bytes());
        let mut links: Vec<_> = matches
            .into_iter()
            .flat_map(|m| {
                m.captures
                    .iter()
                    .map(|capture| text_for_tree_sitter_node(content, &capture.node))
            })
            .collect();
        links.sort();
        links.dedup();

        Self {
            accounts,
            narration,
            flagged_entries,
            tags,
            links,
        }
    }

    pub fn get_accounts(&self) -> Vec<String> {
        let t = self.accounts.join(",");
        tracing::info!(t);
        self.accounts.clone()
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
}
