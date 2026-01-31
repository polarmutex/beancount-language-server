use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use anyhow::Result;
use lsp_types::{Location, SymbolInformation, SymbolKind, WorkspaceSymbolParams};
use ropey::Rope;
use std::str::FromStr;
use tree_sitter_beancount::tree_sitter::Node;
use tree_sitter_beancount::tree_sitter::StreamingIterator;
use url::Url;

/// Provider function for `workspace/symbol`.
pub(crate) fn workspace_symbols(
    snapshot: LspServerStateSnapshot,
    params: WorkspaceSymbolParams,
) -> Result<Option<Vec<SymbolInformation>>> {
    let query = params.query.to_lowercase();
    let mut symbols = Vec::new();

    // Search across all documents in workspace
    for (path, tree) in snapshot.forest.iter() {
        let content = match snapshot.open_docs.get(path) {
            Some(doc) => &doc.content,
            None => {
                tracing::warn!("Document not found in open_docs: {:?}", path);
                continue;
            }
        };

        let url = match Url::from_file_path(path) {
            Ok(url) => url,
            Err(_) => {
                tracing::warn!("Failed to convert path to URL: {:?}", path);
                continue;
            }
        };

        let uri = match lsp_types::Uri::from_str(url.as_ref()) {
            Ok(uri) => uri,
            Err(_) => {
                tracing::warn!("Failed to convert URL to URI: {:?}", url);
                continue;
            }
        };

        let root_node = tree.root_node();
        let mut cursor = root_node.walk();

        // Search all top-level nodes
        for child in root_node.children(&mut cursor) {
            match child.kind() {
                "open" => {
                    if let Some(symbol) = extract_account_symbol(&child, content, &uri, &query) {
                        symbols.push(symbol);
                    }
                }
                "transaction" => {
                    if let Some(symbol) = extract_transaction_symbol(&child, content, &uri, &query)
                    {
                        symbols.push(symbol);
                    }
                }
                "commodity" => {
                    if let Some(symbol) = extract_commodity_symbol(&child, content, &uri, &query) {
                        symbols.push(symbol);
                    }
                }
                "price" => {
                    if let Some(symbol) = extract_price_symbol(&child, content, &uri, &query) {
                        symbols.push(symbol);
                    }
                }
                _ => {}
            }
        }

        // Search for tags and links using tree-sitter query
        extract_tags_and_links_query(tree, content, &uri, &query, &mut symbols);
    }

    // Sort by relevance (exact matches first, then by file/line)
    symbols.sort_by(|a, b| {
        let a_exact = a.name.to_lowercase() == query;
        let b_exact = b.name.to_lowercase() == query;
        if a_exact != b_exact {
            return b_exact.cmp(&a_exact);
        }

        // Then by file and line number
        a.location.uri.cmp(&b.location.uri).then(
            a.location
                .range
                .start
                .line
                .cmp(&b.location.range.start.line),
        )
    });

    tracing::trace!(
        "Workspace symbols: found {} symbols for query '{}'",
        symbols.len(),
        params.query
    );

    if symbols.is_empty() {
        Ok(None)
    } else {
        Ok(Some(symbols))
    }
}

/// Extract account open symbol if it matches the query.
fn extract_account_symbol(
    node: &Node,
    content: &Rope,
    uri: &lsp_types::Uri,
    query: &str,
) -> Option<SymbolInformation> {
    let mut cursor = node.walk();
    let mut account = String::new();

    for child in node.children(&mut cursor) {
        if child.kind() == "account" {
            account = text_for_tree_sitter_node(content, &child);
            break;
        }
    }

    if account.to_lowercase().contains(query) {
        #[allow(deprecated)]
        Some(SymbolInformation {
            name: account,
            kind: SymbolKind::NAMESPACE,
            location: Location {
                uri: uri.clone(),
                range: node_to_range(node),
            },
            container_name: Some(uri.path().to_string()),
            deprecated: None,
            tags: None,
        })
    } else {
        None
    }
}

/// Extract transaction symbol if payee or narration matches the query.
fn extract_transaction_symbol(
    node: &Node,
    content: &Rope,
    uri: &lsp_types::Uri,
    query: &str,
) -> Option<SymbolInformation> {
    let mut cursor = node.walk();
    let mut date = String::new();
    let mut flag = String::new();
    let mut payee = String::new();
    let mut narration = String::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "date" => {
                date = text_for_tree_sitter_node(content, &child);
            }
            "txn" => {
                flag = text_for_tree_sitter_node(content, &child);
            }
            "payee" => {
                payee = text_for_tree_sitter_node(content, &child);
            }
            "narration" => {
                narration = text_for_tree_sitter_node(content, &child);
            }
            _ => {}
        }
    }

    // Check if payee or narration matches
    let matches = payee.to_lowercase().contains(query) || narration.to_lowercase().contains(query);

    if matches {
        let name = if !payee.is_empty() && !narration.is_empty() {
            format!("{} {} {} {}", date, flag, payee, narration)
        } else if !payee.is_empty() {
            format!("{} {} {}", date, flag, payee)
        } else if !narration.is_empty() {
            format!("{} {} {}", date, flag, narration)
        } else {
            format!("{} {}", date, flag)
        };

        #[allow(deprecated)]
        Some(SymbolInformation {
            name,
            kind: SymbolKind::EVENT,
            location: Location {
                uri: uri.clone(),
                range: node_to_range(node),
            },
            container_name: Some(uri.path().to_string()),
            deprecated: None,
            tags: None,
        })
    } else {
        None
    }
}

/// Extract tags and links using tree-sitter query.
fn extract_tags_and_links_query(
    tree: &tree_sitter_beancount::tree_sitter::Tree,
    content: &Rope,
    uri: &lsp_types::Uri,
    query_str: &str,
    symbols: &mut Vec<SymbolInformation>,
) {
    use tree_sitter_beancount::tree_sitter;

    // Create a simple query to find all tags and links
    let query = tree_sitter::Query::new(
        &tree_sitter_beancount::language(),
        r#"
            (tag) @tag
            (link) @link
        "#,
    )
    .expect("Failed to compile tag/link query");

    let content_bytes = content.to_string().into_bytes();
    let mut cursor_qry = tree_sitter::QueryCursor::new();
    let mut matches = cursor_qry.matches(&query, tree.root_node(), content_bytes.as_slice());

    let tag_idx = query
        .capture_index_for_name("tag")
        .expect("query should have 'tag' capture");
    let link_idx = query
        .capture_index_for_name("link")
        .expect("query should have 'link' capture");

    while let Some(qmatch) = matches.next() {
        for capture in qmatch.captures {
            let text = text_for_tree_sitter_node(content, &capture.node);

            if capture.index == tag_idx {
                // Tag node - text already includes the #
                if text.to_lowercase().contains(query_str) {
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name: text.clone(),
                        kind: SymbolKind::STRING,
                        location: Location {
                            uri: uri.clone(),
                            range: node_to_range(&capture.node),
                        },
                        container_name: Some(uri.path().to_string()),
                        deprecated: None,
                        tags: None,
                    });
                }
            } else if capture.index == link_idx {
                // Link node - text already includes the ^
                if text.to_lowercase().contains(query_str) {
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name: text.clone(),
                        kind: SymbolKind::KEY,
                        location: Location {
                            uri: uri.clone(),
                            range: node_to_range(&capture.node),
                        },
                        container_name: Some(uri.path().to_string()),
                        deprecated: None,
                        tags: None,
                    });
                }
            }
        }
    }
}

/// Extract commodity symbol if it matches the query.
fn extract_commodity_symbol(
    node: &Node,
    content: &Rope,
    uri: &lsp_types::Uri,
    query: &str,
) -> Option<SymbolInformation> {
    let mut cursor = node.walk();
    let mut currency = String::new();

    for child in node.children(&mut cursor) {
        if child.kind() == "currency" {
            currency = text_for_tree_sitter_node(content, &child);
            break;
        }
    }

    if currency.to_lowercase().contains(query) {
        #[allow(deprecated)]
        Some(SymbolInformation {
            name: currency,
            kind: SymbolKind::CLASS,
            location: Location {
                uri: uri.clone(),
                range: node_to_range(node),
            },
            container_name: Some(uri.path().to_string()),
            deprecated: None,
            tags: None,
        })
    } else {
        None
    }
}

/// Extract price symbol if currency matches the query.
fn extract_price_symbol(
    node: &Node,
    content: &Rope,
    uri: &lsp_types::Uri,
    query: &str,
) -> Option<SymbolInformation> {
    let mut cursor = node.walk();
    let mut date = String::new();
    let mut currency = String::new();
    let mut amount = String::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "date" => {
                date = text_for_tree_sitter_node(content, &child);
            }
            "currency" => {
                if currency.is_empty() {
                    currency = text_for_tree_sitter_node(content, &child);
                }
            }
            "amount" | "incomplete_amount" => {
                amount = text_for_tree_sitter_node(content, &child);
            }
            _ => {}
        }
    }

    if currency.to_lowercase().contains(query) {
        let name = format!("{} price {} {}", date, currency, amount.trim());

        #[allow(deprecated)]
        Some(SymbolInformation {
            name,
            kind: SymbolKind::NUMBER,
            location: Location {
                uri: uri.clone(),
                range: node_to_range(node),
            },
            container_name: Some(uri.path().to_string()),
            deprecated: None,
            tags: None,
        })
    } else {
        None
    }
}

/// Convert a tree-sitter node to an LSP Range.
fn node_to_range(node: &Node) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position {
            line: node.start_position().row as u32,
            character: node.start_position().column as u32,
        },
        end: lsp_types::Position {
            line: node.end_position().row as u32,
            character: node.end_position().column as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tree_sitter_beancount::tree_sitter;

    struct TestState {
        snapshot: LspServerStateSnapshot,
    }

    impl TestState {
        fn new(content: &str) -> anyhow::Result<Self> {
            let path = std::env::current_dir()?.join("test.beancount");
            let rope_content = ropey::Rope::from_str(content);

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(content, None).unwrap();

            let mut forest = HashMap::new();
            forest.insert(path.clone(), Arc::new(tree.clone()));

            let mut open_docs = HashMap::new();
            open_docs.insert(
                path.clone(),
                Document {
                    content: rope_content.clone(),
                    version: 0,
                },
            );

            let mut beancount_data = HashMap::new();
            beancount_data.insert(
                path.clone(),
                Arc::new(BeancountData::new(&tree, &rope_content)),
            );

            let config = Config::new(path.clone());

            Ok(Self {
                snapshot: LspServerStateSnapshot {
                    forest,
                    open_docs,
                    beancount_data,
                    config,
                    checker: None,
                },
            })
        }
    }

    #[test]
    fn test_search_accounts() {
        let content = r#"2024-01-01 open Assets:Bank:Checking USD
2024-01-01 open Assets:Bank:Savings USD
2024-01-01 open Expenses:Food:Groceries
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "checking".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        let symbols = result.unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::NAMESPACE);
        assert_eq!(symbols[0].name, "Assets:Bank:Checking");
    }

    #[test]
    fn test_search_payee() {
        let content = r#"2024-01-15 * "Amazon.com" "Books"
  Expenses:Shopping    45.23 USD
  Assets:Bank:Checking -45.23 USD

2024-01-22 * "Amazon AWS" "Cloud hosting"
  Expenses:Tech    100.00 USD
  Assets:Bank:Checking -100.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "amazon".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        let symbols = result.unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].kind, SymbolKind::EVENT);
        assert!(symbols[0].name.contains("Amazon"));
    }

    #[test]
    fn test_search_tags() {
        let content = r#"2024-01-15 * "Donation" "Charity" #tax-deductible
  Expenses:Charity    100.00 USD
  Assets:Bank:Checking -100.00 USD

2024-02-10 * "Medical" "Doctor visit" #tax-deductible
  Expenses:Medical    50.00 USD
  Assets:Bank:Checking -50.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "tax".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        let symbols = result.unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].kind, SymbolKind::STRING);
        assert!(symbols[0].name.contains("#tax"));
    }

    #[test]
    fn test_search_links() {
        let content = r#"2024-01-15 * "Flight" "Paris trip" ^trip-paris-2024
  Expenses:Travel    500.00 USD
  Assets:Bank:Checking -500.00 USD

2024-01-20 * "Hotel" "Paris stay" ^trip-paris-2024
  Expenses:Travel    300.00 USD
  Assets:Bank:Checking -300.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "trip".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        let symbols = result.unwrap();
        // Should find 1 transaction (narration "Paris trip") + 2 links
        assert_eq!(symbols.len(), 3);

        // Verify we have both transactions and links
        let links: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::KEY)
            .collect();
        assert_eq!(links.len(), 2);
        assert!(links[0].name.contains("^trip-paris-2024"));

        let transactions: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::EVENT)
            .collect();
        assert_eq!(transactions.len(), 1);
        assert!(transactions[0].name.contains("Paris trip"));
    }

    #[test]
    fn test_search_commodity() {
        let content = r#"2024-01-01 commodity AAPL

2024-01-15 price AAPL 150.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "aapl".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        let symbols = result.unwrap();
        assert_eq!(symbols.len(), 2);
        // First should be commodity, second should be price
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::CLASS));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::NUMBER));
    }

    #[test]
    fn test_empty_query() {
        let content = r#"2024-01-01 open Assets:Checking USD
2024-01-02 * "Test" "Test transaction"
  Assets:Checking  100.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        // Empty query should match everything
        assert!(result.is_some());
        let symbols = result.unwrap();
        assert!(symbols.len() >= 2);
    }

    #[test]
    fn test_no_match() {
        let content = r#"2024-01-01 open Assets:Checking USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "nomatch".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let content = r#"2024-01-01 open Assets:Bank:Checking USD
"#;
        let state = TestState::new(content).unwrap();

        let params = WorkspaceSymbolParams {
            query: "CHECKING".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = workspace_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        let symbols = result.unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Assets:Bank:Checking");
    }
}
