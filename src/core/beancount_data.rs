use crate::core::RopeExt;
use dashmap::DashMap;
use log::debug;
use lspower::lsp;

pub struct BeancountData {
    accounts: DashMap<lsp::Url, Vec<String>>,
    txn_strings: DashMap<lsp::Url, Vec<String>>,
}

impl BeancountData {
    pub fn new() -> Self {
        let accounts = DashMap::new();
        let txn_strings = DashMap::new();
        Self { accounts, txn_strings }
    }

    pub fn update_data(&self, uri: lsp::Url, tree: &tree_sitter::Tree, content: &ropey::Rope) {
        let mut cursor = tree.root_node().walk();

        // Update account opens
        debug!("beancount_data:: get account nodes");
        let accounts_nodes = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "open")
            .collect::<Vec<_>>();

        debug!("beancount_data:: get account strings");
        let account_strings = accounts_nodes.into_iter().filter_map(|node| {
            let account_node = node.children(&mut cursor).find(|c| c.kind() == "account")?;
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
        let transaction_nodes = tree
            .root_node()
            .children(&mut cursor)
            .filter(|c| c.kind() == "transaction")
            .collect::<Vec<_>>();

        debug!("beancount_data:: get account strings");
        let txn_string_strings = transaction_nodes.into_iter().filter_map(|node| {
            let txn_strings_node = node.children(&mut cursor).find(|c| c.kind() == "txn_strings")?;
            if let Some(txn_string_node) = txn_strings_node.children(&mut cursor).next() {
                Some(
                    content
                        .utf8_text_for_tree_sitter_node(&txn_string_node)
                        .trim()
                        .to_string(),
                )
            } else {
                None
            }
        });

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
