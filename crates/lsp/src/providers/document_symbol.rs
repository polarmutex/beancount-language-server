use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use lsp_types::{DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, SymbolKind};
use ropey::Rope;
use tree_sitter::Node;
use tree_sitter_beancount::tree_sitter;

/// Provider function for `textDocument/documentSymbol`.
pub(crate) fn document_symbols(
    snapshot: LspServerStateSnapshot,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let uri = match params.text_document.uri.to_file_path() {
        Ok(path) => path,
        Err(_) => {
            tracing::debug!("Failed to convert URI to file path");
            return Ok(None);
        }
    };

    let forest = snapshot.forest;
    let tree = match forest.get(&uri) {
        Some(tree) => tree,
        None => {
            tracing::warn!("Tree not found in forest: {:?}", uri);
            return Ok(None);
        }
    };

    let content = match snapshot.open_docs.get(&uri) {
        Some(doc) => doc.content.clone(),
        None => {
            tracing::warn!("Document not found in open_docs: {:?}", uri);
            return Ok(None);
        }
    };

    let mut symbols = Vec::new();
    let root_node = tree.root_node();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        if let Some(symbol) = extract_symbol(&child, &content) {
            symbols.push(symbol);
        }
    }

    tracing::trace!("Document symbols: found {} symbols", symbols.len());
    Ok(Some(DocumentSymbolResponse::Nested(symbols)))
}

/// Extract a DocumentSymbol from a tree-sitter node.
fn extract_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    match node.kind() {
        "transaction" => extract_transaction_symbol(node, content),
        "open" => extract_open_symbol(node, content),
        "close" => extract_close_symbol(node, content),
        "balance" => extract_balance_symbol(node, content),
        "price" => extract_price_symbol(node, content),
        "commodity" => extract_commodity_symbol(node, content),
        "event" => extract_event_symbol(node, content),
        "option" => extract_option_symbol(node, content),
        "comment" => extract_heading_symbol(node, content),
        "section" => extract_section_symbol(node, content),
        _ => None,
    }
}

/// Extract transaction symbol with postings as children.
fn extract_transaction_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut date = String::new();
    let mut flag = String::new();
    let mut payee = String::new();
    let mut narration = String::new();
    let mut postings = Vec::new();

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
            "posting" => {
                if let Some(posting_symbol) = extract_posting_symbol(&child, content) {
                    postings.push(posting_symbol);
                }
            }
            _ => {}
        }
    }

    // Build the transaction name
    let name = if !payee.is_empty() && !narration.is_empty() {
        format!("{} {} {} {}", date, flag, payee, narration)
    } else if !payee.is_empty() {
        format!("{} {} {}", date, flag, payee)
    } else if !narration.is_empty() {
        format!("{} {} {}", date, flag, narration)
    } else {
        format!("{} {}", date, flag)
    };

    Some(DocumentSymbol {
        name,
        detail: Some("Transaction".to_string()),
        kind: SymbolKind::STRUCT,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: if postings.is_empty() {
            None
        } else {
            Some(postings)
        },
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract posting symbol as a child of transaction.
fn extract_posting_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut account = String::new();
    let mut amount = String::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "account" => {
                account = text_for_tree_sitter_node(content, &child);
            }
            "incomplete_amount" | "amount" => {
                // Extract the full amount text including number and currency
                amount = text_for_tree_sitter_node(content, &child);
            }
            _ => {}
        }
    }

    let name = if !amount.is_empty() {
        format!("{} {}", account, amount.trim())
    } else {
        account
    };

    Some(DocumentSymbol {
        name,
        detail: Some("Posting".to_string()),
        kind: SymbolKind::PROPERTY,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract open account symbol.
fn extract_open_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut account = String::new();
    let mut currencies = Vec::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "account" => {
                account = text_for_tree_sitter_node(content, &child);
            }
            "currency" => {
                currencies.push(text_for_tree_sitter_node(content, &child));
            }
            _ => {}
        }
    }

    let detail = if !currencies.is_empty() {
        format!("Open: {}", currencies.join(", "))
    } else {
        "Open".to_string()
    };

    Some(DocumentSymbol {
        name: account,
        detail: Some(detail),
        kind: SymbolKind::FILE,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract close account symbol.
fn extract_close_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut account = String::new();

    for child in node.children(&mut cursor) {
        if child.kind() == "account" {
            account = text_for_tree_sitter_node(content, &child);
            break;
        }
    }

    Some(DocumentSymbol {
        name: account,
        detail: Some("Close".to_string()),
        kind: SymbolKind::FILE,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract balance assertion symbol.
fn extract_balance_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut account = String::new();
    let mut amount = String::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "account" => {
                account = text_for_tree_sitter_node(content, &child);
            }
            "amount" | "incomplete_amount" | "amount_tolerance" => {
                amount = text_for_tree_sitter_node(content, &child);
            }
            _ => {}
        }
    }

    let name = if !amount.is_empty() {
        format!("{} = {}", account, amount.trim())
    } else {
        account
    };

    Some(DocumentSymbol {
        name,
        detail: Some("Balance".to_string()),
        kind: SymbolKind::CONSTANT,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract price directive symbol.
fn extract_price_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
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

    let name = format!("{} 1 {} = {}", date, currency, amount.trim());

    Some(DocumentSymbol {
        name,
        detail: Some("Price".to_string()),
        kind: SymbolKind::NUMBER,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract commodity declaration symbol.
fn extract_commodity_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut currency = String::new();

    for child in node.children(&mut cursor) {
        if child.kind() == "currency" {
            currency = text_for_tree_sitter_node(content, &child);
            break;
        }
    }

    Some(DocumentSymbol {
        name: currency,
        detail: Some("Commodity".to_string()),
        kind: SymbolKind::CLASS,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract event directive symbol.
fn extract_event_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut date = String::new();
    let mut event_type = String::new();
    let mut description = String::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "date" => {
                date = text_for_tree_sitter_node(content, &child);
            }
            "string" => {
                if event_type.is_empty() {
                    event_type = text_for_tree_sitter_node(content, &child);
                } else if description.is_empty() {
                    description = text_for_tree_sitter_node(content, &child);
                }
            }
            _ => {}
        }
    }

    let name = format!("{} {} {}", date, event_type, description);

    Some(DocumentSymbol {
        name,
        detail: Some("Event".to_string()),
        kind: SymbolKind::EVENT,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract option directive symbol.
fn extract_option_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut option_name = String::new();
    let mut option_value = String::new();

    for child in node.children(&mut cursor) {
        if child.kind() == "string" {
            if option_name.is_empty() {
                option_name = text_for_tree_sitter_node(content, &child);
            } else if option_value.is_empty() {
                option_value = text_for_tree_sitter_node(content, &child);
            }
        }
    }

    let name = format!("{} = {}", option_name, option_value);

    Some(DocumentSymbol {
        name,
        detail: Some("Option".to_string()),
        kind: SymbolKind::PROPERTY,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: None,
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract section symbol (org-mode and markdown sections parsed by tree-sitter-beancount).
/// Sections are hierarchical with "headline" and nested "section" children.
/// Supports both org-mode (* headers) and markdown (# headers).
fn extract_section_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let mut cursor = node.walk();
    let mut headline_text = String::new();
    let mut level = 0;
    let mut section_type = "Section";
    let mut children = Vec::new();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "headline" => {
                let text = text_for_tree_sitter_node(content, &child);
                let trimmed = text.trim();

                // Check for org-mode style (* headers)
                if trimmed.starts_with('*') {
                    level = trimmed.chars().take_while(|&c| c == '*').count();
                    if level > 0 {
                        headline_text = trimmed[level..].trim().to_string();
                        section_type = "Section";
                    }
                }
                // Check for markdown style (# headers)
                else if trimmed.starts_with('#') {
                    level = trimmed.chars().take_while(|&c| c == '#').count();
                    if level > 0 {
                        headline_text = trimmed[level..].trim().to_string();
                        section_type = "Heading";
                    }
                }
            }
            "section" => {
                // Recursively extract nested sections as children
                if let Some(child_symbol) = extract_section_symbol(&child, content) {
                    children.push(child_symbol);
                }
            }
            _ => {
                // Extract other directives (open, transaction, etc.) as children
                if let Some(child_symbol) = extract_symbol(&child, content) {
                    children.push(child_symbol);
                }
            }
        }
    }

    if headline_text.is_empty() {
        return None;
    }

    let detail = format!("{} (Level {})", section_type, level);
    Some(DocumentSymbol {
        name: headline_text,
        detail: Some(detail),
        kind: SymbolKind::NAMESPACE,
        range: node_to_range(node),
        selection_range: node_to_range(node),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
        #[allow(deprecated)]
        deprecated: None,
        tags: None,
    })
}

/// Extract heading symbol from comment lines (markdown style).
/// Markdown: `# Heading`, `## Subheading`
fn extract_heading_symbol(node: &Node, content: &Rope) -> Option<DocumentSymbol> {
    let text = text_for_tree_sitter_node(content, node);
    let trimmed = text.trim();

    // Check for markdown style (# headers)
    if let Some(stripped) = trimmed.strip_prefix('#') {
        let mut level = 1;
        let mut remaining = stripped;

        // Count additional hashes
        while let Some(rest) = remaining.strip_prefix('#') {
            level += 1;
            remaining = rest;
        }

        // Extract the heading text after hashes and any whitespace
        let heading_text = remaining.trim();
        if heading_text.is_empty() {
            return None;
        }

        let detail = format!("Heading (Level {})", level);
        return Some(DocumentSymbol {
            name: heading_text.to_string(),
            detail: Some(detail),
            kind: SymbolKind::NAMESPACE,
            range: node_to_range(node),
            selection_range: node_to_range(node),
            children: None,
            #[allow(deprecated)]
            deprecated: None,
            tags: None,
        });
    }

    // Not a heading comment, ignore regular comments
    None
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
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;
    use tree_sitter_beancount::tree_sitter;
    use url::Url;

    struct TestState {
        snapshot: LspServerStateSnapshot,
        path: PathBuf,
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
                path,
            })
        }
    }

    #[test]
    fn test_transaction_symbol() {
        let content = r#"2024-01-15 * "Grocery Store" "Weekly shopping"
  Expenses:Food:Groceries    45.23 USD
  Assets:Bank:Checking      -45.23 USD
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 1);
            let symbol = &symbols[0];
            assert_eq!(symbol.kind, SymbolKind::STRUCT);
            assert!(symbol.name.contains("2024-01-15"));
            assert!(symbol.name.contains("Grocery Store"));
            assert!(symbol.name.contains("Weekly shopping"));

            // Check postings
            let children = symbol.children.as_ref().unwrap();
            assert_eq!(children.len(), 2);
            assert_eq!(children[0].kind, SymbolKind::PROPERTY);
            assert!(children[0].name.contains("Expenses:Food:Groceries"));
            assert!(children[0].name.contains("45.23 USD"));
        } else {
            panic!("Expected nested document symbols");
        }
    }

    #[test]
    fn test_open_symbol() {
        let content = "2024-01-01 open Assets:Checking USD\n";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 1);
            let symbol = &symbols[0];
            assert_eq!(symbol.kind, SymbolKind::FILE);
            assert_eq!(symbol.name, "Assets:Checking");
            assert!(symbol.detail.as_ref().unwrap().contains("USD"));
        }
    }

    #[test]
    fn test_balance_symbol() {
        let content = "2024-01-01 balance Assets:Checking 1000.00 USD\n";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 1);
            let symbol = &symbols[0];
            eprintln!("Balance symbol name: {}", symbol.name);
            assert_eq!(symbol.kind, SymbolKind::CONSTANT);
            assert!(symbol.name.contains("Assets:Checking"));
            assert!(symbol.name.contains("1000.00") || symbol.name.contains("="));
        }
    }

    #[test]
    fn test_price_symbol() {
        let content = "2024-01-15 price AAPL 150.00 USD\n";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 1);
            let symbol = &symbols[0];
            assert_eq!(symbol.kind, SymbolKind::NUMBER);
            assert!(symbol.name.contains("AAPL"));
            assert!(symbol.name.contains("150.00 USD"));
        }
    }

    #[test]
    fn test_option_symbol() {
        let content = r#"option "title" "My Ledger"
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 1);
            let symbol = &symbols[0];
            assert_eq!(symbol.kind, SymbolKind::PROPERTY);
            assert!(symbol.name.contains("title"));
            assert!(symbol.name.contains("My Ledger"));
        }
    }

    #[test]
    fn test_mixed_content() {
        let content = r#"option "title" "My Ledger"

2024-01-01 open Assets:Checking USD

2024-01-15 * "Grocery Store" "Weekly shopping"
  Expenses:Food:Groceries    45.23 USD
  Assets:Checking           -45.23 USD

2024-01-20 balance Assets:Checking 1000.00 USD

2024-01-21 price AAPL 150.00 USD
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 5);

            // Option
            assert_eq!(symbols[0].kind, SymbolKind::PROPERTY);

            // Open
            assert_eq!(symbols[1].kind, SymbolKind::FILE);
            assert_eq!(symbols[1].name, "Assets:Checking");

            // Transaction
            assert_eq!(symbols[2].kind, SymbolKind::STRUCT);
            assert!(symbols[2].children.is_some());

            // Balance
            assert_eq!(symbols[3].kind, SymbolKind::CONSTANT);

            // Price
            assert_eq!(symbols[4].kind, SymbolKind::NUMBER);
        }
    }

    #[test]
    fn test_empty_file() {
        let content = "";
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            assert_eq!(symbols.len(), 0);
        }
    }

    #[test]
    fn test_org_mode_headers() {
        let content = r#"* Top Level Section
** Subsection Level 2
*** Subsection Level 3

2024-01-01 open Assets:Checking USD
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            // Should have 1 top-level section with nested children
            assert_eq!(symbols.len(), 1);

            // Check top level section
            let section1 = &symbols[0];
            assert_eq!(section1.kind, SymbolKind::NAMESPACE);
            assert_eq!(section1.name, "Top Level Section");
            assert_eq!(section1.detail, Some("Section (Level 1)".to_string()));

            // Check nested children - only level 2 section (open is nested deeper)
            let children = section1.children.as_ref().unwrap();
            assert_eq!(children.len(), 1);

            // Check second level section
            let section2 = &children[0];
            assert_eq!(section2.kind, SymbolKind::NAMESPACE);
            assert_eq!(section2.name, "Subsection Level 2");
            assert_eq!(section2.detail, Some("Section (Level 2)".to_string()));

            // Check third level section nested in second
            let section2_children = section2.children.as_ref().unwrap();
            assert_eq!(section2_children.len(), 1); // Level 3 section

            let section3 = &section2_children[0];
            assert_eq!(section3.kind, SymbolKind::NAMESPACE);
            assert_eq!(section3.name, "Subsection Level 3");
            assert_eq!(section3.detail, Some("Section (Level 3)".to_string()));

            // Check open directive at level 3
            let section3_children = section3.children.as_ref().unwrap();
            assert_eq!(section3_children.len(), 1);
            let open = &section3_children[0];
            assert_eq!(open.kind, SymbolKind::FILE);
            assert_eq!(open.name, "Assets:Checking");
        } else {
            panic!("Expected nested document symbols");
        }
    }

    #[test]
    fn test_markdown_headers() {
        let content = r#"# Markdown Header 1
## Markdown Header 2
### Markdown Header 3

2024-01-01 open Assets:Checking USD
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            // Should have 1 top-level section with nested children
            assert_eq!(symbols.len(), 1);

            // Check top level header
            let header1 = &symbols[0];
            assert_eq!(header1.kind, SymbolKind::NAMESPACE);
            assert_eq!(header1.name, "Markdown Header 1");
            assert_eq!(header1.detail, Some("Heading (Level 1)".to_string()));

            // Check nested children
            let children = header1.children.as_ref().unwrap();
            assert_eq!(children.len(), 1); // Level 2 header

            // Check second level header
            let header2 = &children[0];
            assert_eq!(header2.kind, SymbolKind::NAMESPACE);
            assert_eq!(header2.name, "Markdown Header 2");
            assert_eq!(header2.detail, Some("Heading (Level 2)".to_string()));

            // Check third level header nested in second
            let header2_children = header2.children.as_ref().unwrap();
            assert_eq!(header2_children.len(), 1);

            let header3 = &header2_children[0];
            assert_eq!(header3.kind, SymbolKind::NAMESPACE);
            assert_eq!(header3.name, "Markdown Header 3");
            assert_eq!(header3.detail, Some("Heading (Level 3)".to_string()));

            // Check open directive at level 3
            let header3_children = header3.children.as_ref().unwrap();
            assert_eq!(header3_children.len(), 1);
            let open = &header3_children[0];
            assert_eq!(open.kind, SymbolKind::FILE);
            assert_eq!(open.name, "Assets:Checking");
        } else {
            panic!("Expected nested document symbols");
        }
    }

    #[test]
    fn test_regular_comments_ignored() {
        let content = r#"; Regular comment
;; Another comment
; Not a header

2024-01-01 open Assets:Checking USD
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            // Only the open directive should be returned, regular comments are ignored
            assert_eq!(symbols.len(), 1);
            assert_eq!(symbols[0].kind, SymbolKind::FILE);
            assert_eq!(symbols[0].name, "Assets:Checking");
        } else {
            panic!("Expected nested document symbols");
        }
    }

    #[test]
    fn test_mixed_headers_and_directives() {
        let content = r#"* Income

2024-01-01 open Income:Salary USD

** Salary Details

2024-01-15 * "Employer" "Monthly salary"
  Income:Salary   -5000.00 USD
  Assets:Checking  5000.00 USD

# Expenses

2024-01-01 open Expenses:Groceries USD
"#;
        let state = TestState::new(content).unwrap();

        let uri =
            lsp_types::Uri::from_str(Url::from_file_path(&state.path).unwrap().as_ref()).unwrap();
        let params = DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = document_symbols(state.snapshot, params).unwrap();
        assert!(result.is_some());

        if let Some(DocumentSymbolResponse::Nested(symbols)) = result {
            // Should have 2 top-level sections: Income (org-mode) and Expenses (markdown)
            assert_eq!(symbols.len(), 2);

            // Check org-mode Income section
            assert_eq!(symbols[0].name, "Income");
            assert_eq!(symbols[0].detail, Some("Section (Level 1)".to_string()));

            // Income section should have: open directive, Salary Details subsection
            let income_children = symbols[0].children.as_ref().unwrap();
            assert!(income_children.len() >= 2);
            assert_eq!(income_children[0].name, "Income:Salary");
            assert_eq!(income_children[1].name, "Salary Details");
            assert_eq!(
                income_children[1].detail,
                Some("Section (Level 2)".to_string())
            );

            // Check markdown Expenses section
            assert_eq!(symbols[1].name, "Expenses");
            assert_eq!(symbols[1].detail, Some("Heading (Level 1)".to_string()));

            // Expenses section should have open directive
            let expenses_children = symbols[1].children.as_ref().unwrap();
            assert_eq!(expenses_children[0].name, "Expenses:Groceries");
        } else {
            panic!("Expected nested document symbols");
        }
    }
}
