/// Utility module for tree-sitter query operations
///
/// This module provides helper functions to simplify common tree-sitter query patterns
/// used throughout the beancount language server.
///
/// # Query Patterns
///
/// ## Field Queries
/// Field queries allow extracting specific named fields from tree-sitter nodes:
/// ```scheme
/// (open account: (account) @account)
/// (transaction payee: (string) @payee)
/// (transaction narration: (string) @narration)
/// ```
///
/// ## Node Queries
/// Simple node queries match any occurrence of a node type:
/// ```scheme
/// (tag) @tag
/// (link) @link
/// (currency) @currency
/// ```
///
/// ## Nested Queries
/// Queries can specify parent-child relationships:
/// ```scheme
/// (txn (flag) @flag)
/// (open (currency) @currency)
/// (commodity (currency) @currency)
/// ```
use crate::treesitter_utils::text_for_tree_sitter_node;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

/// Execute a tree-sitter query and collect all matching text nodes
///
/// # Arguments
/// * `query_string` - The tree-sitter query pattern (S-expression format)
/// * `tree` - The parsed syntax tree
/// * `content` - The source text (as ropey::Rope)
///
/// # Returns
/// A vector of strings containing the text of all captured nodes
///
/// # Example
/// ```ignore
/// let accounts = query_and_collect(
///     r#"(open account: (account) @account)"#,
///     tree,
///     content
/// );
/// ```
#[allow(dead_code)]
pub fn query_and_collect(
    query_string: &str,
    tree: &tree_sitter::Tree,
    content: &ropey::Rope,
) -> Vec<String> {
    let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
        .unwrap_or_else(|_| panic!("Invalid query: {query_string}"));

    let mut cursor = tree_sitter::QueryCursor::new();
    let binding = content.clone().to_string();
    let mut matches = cursor.matches(&query, tree.root_node(), binding.as_bytes());

    let mut results = Vec::new();
    while let Some(qmatch) = matches.next() {
        for capture in qmatch.captures {
            let text = text_for_tree_sitter_node(content, &capture.node);
            results.push(text);
        }
    }
    results
}

/// Execute a tree-sitter query and collect all matching nodes (not just text)
///
/// # Arguments
/// * `query_string` - The tree-sitter query pattern (S-expression format)
/// * `tree` - The parsed syntax tree
/// * `content` - The source text (as ropey::Rope)
///
/// # Returns
/// A vector of tree_sitter::Node references for all captured nodes
///
/// # Example
/// ```ignore
/// let flag_nodes = query_and_collect_nodes(
///     r#"(txn (flag) @flag)"#,
///     tree,
///     content
/// );
/// for node in flag_nodes {
///     println!("Flag at line: {}", node.start_position().row);
/// }
/// ```
#[allow(dead_code)]
pub fn query_and_collect_nodes<'tree>(
    query_string: &str,
    tree: &'tree tree_sitter::Tree,
    content: &ropey::Rope,
) -> Vec<tree_sitter::Node<'tree>> {
    let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
        .unwrap_or_else(|_| panic!("Invalid query: {query_string}"));

    let mut cursor = tree_sitter::QueryCursor::new();
    let binding = content.clone().to_string();
    let mut matches = cursor.matches(&query, tree.root_node(), binding.as_bytes());

    let mut results = Vec::new();
    while let Some(qmatch) = matches.next() {
        for capture in qmatch.captures {
            results.push(capture.node);
        }
    }
    results
}

/// Execute a tree-sitter query and apply a transformation function to each match
///
/// # Arguments
/// * `query_string` - The tree-sitter query pattern (S-expression format)
/// * `tree` - The parsed syntax tree
/// * `content` - The source text (as ropey::Rope)
/// * `f` - Transformation function to apply to each captured node
///
/// # Returns
/// A vector of results from applying the transformation function
///
/// # Example
/// ```ignore
/// let trimmed_tags = query_and_map(
///     r#"(tag) @tag"#,
///     tree,
///     content,
///     |text| text.trim().to_string()
/// );
/// ```
#[allow(dead_code)]
pub fn query_and_map<F, T>(
    query_string: &str,
    tree: &tree_sitter::Tree,
    content: &ropey::Rope,
    mut f: F,
) -> Vec<T>
where
    F: FnMut(String) -> T,
{
    let query = tree_sitter::Query::new(&tree_sitter_beancount::language(), query_string)
        .unwrap_or_else(|_| panic!("Invalid query: {query_string}"));

    let mut cursor = tree_sitter::QueryCursor::new();
    let binding = content.clone().to_string();
    let mut matches = cursor.matches(&query, tree.root_node(), binding.as_bytes());

    let mut results = Vec::new();
    while let Some(qmatch) = matches.next() {
        for capture in qmatch.captures {
            let text = text_for_tree_sitter_node(content, &capture.node);
            results.push(f(text));
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_beancount(source: &str) -> (tree_sitter::Tree, ropey::Rope) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .expect("Failed to load beancount grammar");

        let rope = ropey::Rope::from_str(source);
        let tree = parser
            .parse(source, None)
            .expect("Failed to parse beancount");

        (tree, rope)
    }

    #[test]
    fn test_query_and_collect_accounts() {
        let source = r#"
2024-01-01 open Assets:Bank:Checking
2024-01-01 open Expenses:Groceries
        "#;

        let (tree, content) = parse_beancount(source);
        let accounts = query_and_collect(r#"(open account: (account) @account)"#, &tree, &content);

        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0], "Assets:Bank:Checking");
        assert_eq!(accounts[1], "Expenses:Groceries");
    }

    #[test]
    #[ignore]
    fn test_debug_tree_structure() {
        let source = r#"2024-01-01 * "Grocery Store" "Weekly shopping""#;
        let (tree, _content) = parse_beancount(source);

        // Print the tree structure to understand the node types
        fn print_node(node: tree_sitter::Node, depth: usize) {
            let indent = "  ".repeat(depth);
            println!(
                "{}kind: {}, field_name: {:?}",
                indent,
                node.kind(),
                node.grammar_name()
            );

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                print_node(child, depth + 1);
            }
        }

        print_node(tree.root_node(), 0);
        panic!("Debug tree structure - check output above");
    }

    #[test]
    #[ignore]
    fn test_debug_flag_structure() {
        let source = r#"
2024-01-01 ! "Flagged"
2024-01-02 * "Normal"
        "#;
        let (tree, _content) = parse_beancount(source);

        fn print_node(node: tree_sitter::Node, depth: usize) {
            let indent = "  ".repeat(depth);
            println!("{}kind: {}", indent, node.kind());

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                print_node(child, depth + 1);
            }
        }

        print_node(tree.root_node(), 0);
        panic!("Debug flag structure - check output above");
    }

    #[test]
    fn test_query_and_collect_payees() {
        let source = r#"
2024-01-01 * "Grocery Store" "Weekly shopping"
2024-01-02 * "Coffee Shop" "Morning coffee"
        "#;

        let (tree, content) = parse_beancount(source);
        // Query by node kind, not field name
        let payees = query_and_collect(r#"(payee) @payee"#, &tree, &content);

        assert_eq!(payees.len(), 2);
        assert!(payees[0].contains("Grocery Store"));
        assert!(payees[1].contains("Coffee Shop"));
    }

    #[test]
    fn test_query_and_collect_narrations() {
        let source = r#"
2024-01-01 * "Grocery Store" "Weekly shopping"
2024-01-02 * "Morning coffee"
        "#;

        let (tree, content) = parse_beancount(source);
        // Query by node kind, not field name
        let narrations = query_and_collect(r#"(narration) @narration"#, &tree, &content);

        assert_eq!(narrations.len(), 2);
        assert!(narrations[0].contains("Weekly shopping"));
        assert!(narrations[1].contains("Morning coffee"));
    }

    #[test]
    fn test_query_and_collect_tags() {
        let source = r#"
2024-01-01 * "Test" #vacation #travel
2024-01-02 * "Test" #work
        "#;

        let (tree, content) = parse_beancount(source);
        let tags = query_and_collect(r#"(tag) @tag"#, &tree, &content);

        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], "#vacation");
        assert_eq!(tags[1], "#travel");
        assert_eq!(tags[2], "#work");
    }

    #[test]
    fn test_query_and_collect_links() {
        let source = r#"
2024-01-01 * "Test" ^link1 ^link2
2024-01-02 * "Test" ^link3
        "#;

        let (tree, content) = parse_beancount(source);
        let links = query_and_collect(r#"(link) @link"#, &tree, &content);

        assert_eq!(links.len(), 3);
        assert_eq!(links[0], "^link1");
        assert_eq!(links[1], "^link2");
        assert_eq!(links[2], "^link3");
    }

    #[test]
    fn test_query_and_collect_nodes_flags() {
        let source = r#"
2024-01-01 ! "Flagged transaction"
2024-01-02 ! "Another flagged"
2024-01-03 * "Normal transaction"
        "#;

        let (tree, content) = parse_beancount(source);
        // Query for "flag" nodes - this matches ! but not *
        // (In tree-sitter-beancount, ! has kind="flag", * has kind="*")
        let flag_nodes = query_and_collect_nodes(r#"(flag) @flag"#, &tree, &content);

        // Only ! flags are matched, not *
        assert_eq!(flag_nodes.len(), 2);
        assert_eq!(flag_nodes[0].start_position().row, 1);
        assert_eq!(flag_nodes[1].start_position().row, 2);
    }

    #[test]
    fn test_query_and_map_trimmed() {
        let source = r#"
2024-01-01 open Assets:Bank:Checking
2024-01-01 open Expenses:Groceries
        "#;

        let (tree, content) = parse_beancount(source);
        let accounts = query_and_map(
            r#"(open account: (account) @account)"#,
            &tree,
            &content,
            |text| text.trim().to_lowercase(),
        );

        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0], "assets:bank:checking");
        assert_eq!(accounts[1], "expenses:groceries");
    }

    #[test]
    fn test_query_currencies_in_open() {
        let source = r#"
2024-01-01 open Assets:Bank:Checking USD
2024-01-01 open Assets:Crypto:Wallet BTC
        "#;

        let (tree, content) = parse_beancount(source);
        let currencies = query_and_collect(r#"(open (currency) @currency)"#, &tree, &content);

        assert_eq!(currencies.len(), 2);
        assert_eq!(currencies[0], "USD");
        assert_eq!(currencies[1], "BTC");
    }

    #[test]
    fn test_query_currencies_in_commodity() {
        let source = r#"
2024-01-01 commodity USD
2024-01-01 commodity EUR
        "#;

        let (tree, content) = parse_beancount(source);
        let currencies = query_and_collect(r#"(commodity (currency) @currency)"#, &tree, &content);

        assert_eq!(currencies.len(), 2);
        assert_eq!(currencies[0], "USD");
        assert_eq!(currencies[1], "EUR");
    }
}
