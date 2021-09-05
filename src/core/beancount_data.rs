use crate::core::RopeExt;
use dashmap::DashMap;
use log::debug;
use lspower::lsp;

pub struct BeancountData {
    accounts: DashMap<lsp::Url, Vec<String>>,
}

impl BeancountData {
    pub fn new() -> Self {
        let accounts = DashMap::new();
        Self { accounts }
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
}
