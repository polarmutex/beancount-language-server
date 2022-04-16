use crate::core::RopeExt;
use dashmap::DashMap;
use log::debug;
use lspower::lsp;
use std::collections::HashSet;

pub struct FlaggedEntry {
    _file: String,
    pub line: u32,
}

//impl FlaggedEntry {
//    pub fn new(file: String, line: u32) -> Self {
//        Self { file, line }
//    }
//}

pub struct BeancountData {
    accounts: DashMap<lsp::Url, Vec<String>>,
    txn_strings: DashMap<lsp::Url, Vec<String>>,
    pub flagged_entries: DashMap<lsp::Url, Vec<FlaggedEntry>>,
}

impl BeancountData {
    pub fn new() -> Self {
        let accounts = DashMap::new();
        let txn_strings = DashMap::new();
        let flagged_entries = DashMap::new();
        Self {
            accounts,
            txn_strings,
            flagged_entries,
        }
    }

    pub fn update_data(&self, uri: lsp::Url, tree: &tree_sitter::Tree, content: &ropey::Rope) {
        let mut cursor = tree.root_node().walk();

        // Update account opens
        debug!("beancount_data:: get account nodes");
        debug!("beancount_data:: get account strings");
        let account_strings = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "open")
            .filter_map(|node| {
                let mut node_cursor = node.walk();
                let account_node = node.children(&mut node_cursor).find(|c| c.kind() == "account")?;
                let account = content.utf8_text_for_tree_sitter_node(&account_node).to_string();
                Some(account)
            });

        debug!("beancount_data:: update accounts");
        if self.accounts.contains_key(&uri) {
            self.accounts.get_mut(&uri).unwrap().clear();
        } else {
            self.accounts.insert(uri.clone(), Vec::new());
        }

        for account in account_strings {
            self.accounts.get_mut(&uri).unwrap().push(account);
        }

        // Update account opens
        debug!("beancount_data:: get txn_strings nodes");
        debug!("beancount_data:: get account strings");
        let transactions = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "transaction")
            .collect::<Vec<_>>();

        //TODO: consider doing something silimar with others around
        let mut txn_string_strings: HashSet<String> = HashSet::new();
        for transaction in transactions {
            if let Some(txn_strings) = transaction.child_by_field_name("txn_strings") {
                if let Some(payee) = txn_strings.children(&mut cursor).next() {
                    txn_string_strings.insert(content.utf8_text_for_tree_sitter_node(&payee).trim().to_string());
                }
            }
        }

        debug!("beancount_data:: update txn_strings");
        if self.txn_strings.contains_key(&uri) {
            self.txn_strings.get_mut(&uri).unwrap().clear();
        } else {
            self.txn_strings.insert(uri.clone(), Vec::new());
        }

        for txn_string in txn_string_strings {
            if !self.txn_strings.get_mut(&uri).unwrap().contains(&txn_string) {
                self.txn_strings.get_mut(&uri).unwrap().push(txn_string);
            }
        }

        // Update flagged entries
        debug!("beancount_data:: update flagged entries");
        if self.flagged_entries.contains_key(&uri) {
            self.flagged_entries.get_mut(&uri).unwrap().clear();
        } else {
            self.flagged_entries.insert(uri.clone(), Vec::new());
        }

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
                    let flag_node = txn_node.children(&mut flag_cursor).find(|c| c.kind() == "flag");
                    if let Some(flag) = flag_node {
                        debug!("addind flag entry: {:?}", flag);
                        self.flagged_entries.get_mut(&uri).unwrap().push(FlaggedEntry {
                            _file: "".to_string(),
                            line: flag.start_position().row as u32,
                        });
                    }
                }
            });
    }

    pub fn get_accounts(&self) -> Vec<String> {
        let mut accounts = Vec::new();
        for it in self.accounts.iter() {
            for account in it.value() {
                accounts.push(account.clone());
            }
        }
        accounts
    }

    pub fn get_txn_strings(&self) -> Vec<String> {
        let mut txn_strings = Vec::new();
        for it in self.txn_strings.iter() {
            for txn_string in it.value() {
                if !txn_strings.contains(txn_string) {
                    txn_strings.push(txn_string.clone());
                }
            }
        }
        txn_strings
    }
}
