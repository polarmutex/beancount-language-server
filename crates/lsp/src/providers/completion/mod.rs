mod context;
mod items;

use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::lsp_position_to_tree_sitter_point;
use anyhow::Result;
use tracing::debug;

pub use items::{add_one_month, sub_one_month};

/// Main entry point for completion with LSP 3.17 compliant implementation.
///
/// Uses left-context-aware traversal to determine completion context even when
/// the syntax tree is in an ERROR state due to incomplete input.
pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::CompletionParams,
) -> Result<Option<lsp_types::CompletionResponse>> {
    let trigger_character = extract_trigger_char(&params);
    let cursor = params.text_document_position_params;

    debug!("=== Completion Request ===");
    debug!("Trigger character: {:?}", trigger_character);
    debug!(
        "Position: {}:{}",
        cursor.position.line, cursor.position.character
    );

    // Get parsed tree and document (snapshot helper supports Uri directly)
    let (tree, doc) = snapshot.tree_and_document_for_uri(&cursor.text_document.uri)?;

    let content = &doc.content;
    let cursor_point = lsp_position_to_tree_sitter_point(content, cursor.position)?;

    // Determine completion context using left-context-aware analysis
    let ctx = context::determine_completion_context(tree, content, cursor_point, trigger_character);

    debug!("Determined context: {:?}", ctx);

    // Generate completions based on context
    let items = items::generate_completions(
        &snapshot.beancount_data,
        &ctx,
        content,
        cursor.position,
        &snapshot.config,
    )?;

    Ok(items.map(|items| {
        // Return CompletionList instead of Array to signal that server-side
        // filtering is preferred. Setting `is_incomplete: true` tells clients
        // like Zed to re-query on each keystroke rather than filtering internally.
        lsp_types::CompletionResponse::CompletionList(lsp_types::CompletionList {
            is_incomplete: true,
            item_defaults: None,
            apply_kind: None,
            items,
        })
    }))
}

/// Extract the effective trigger character from completion params.
///
/// The "2" character is treated as a trigger only at the start of a line
/// (character position <= 1); otherwise it is ignored so that typing a year
/// like "2024" does not falsely trigger date completion mid-number.
fn extract_trigger_char(params: &lsp_types::CompletionParams) -> Option<char> {
    let trigger = params
        .context
        .as_ref()
        .and_then(|ctx| ctx.trigger_character.as_deref())?;

    if trigger == "2" {
        if params.text_document_position_params.position.character > 1 {
            None
        } else {
            trigger.chars().last()
        }
    } else {
        trigger.chars().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::server::LspServerStateSnapshot;
    use lsp_types::{TextDocumentIdentifier, TextDocumentPositionParams};
    use ropey::Rope;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;
    use tree_sitter_beancount::tree_sitter::Parser;

    fn make_snapshot(
        test_data: &str,
        edit_text: &str,
    ) -> (LspServerStateSnapshot, lsp_types::Uri, PathBuf) {
        let rope = Rope::from_str(test_data);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(test_data, None).unwrap();
        let bean_data = BeancountData::new(&Arc::new(tree), &rope);

        let mut beancount_data: HashMap<PathBuf, Arc<BeancountData>> = HashMap::new();
        let (path, uri) = if cfg!(windows) {
            let path = PathBuf::from("C:\\test.bean");
            let url = url::Url::from_file_path(&path).unwrap();
            let uri = lsp_types::Uri::from_str(url.as_str()).unwrap();
            (path, uri)
        } else {
            let path = PathBuf::from("/test.bean");
            let url = url::Url::from_file_path(&path).unwrap();
            let uri = lsp_types::Uri::from_str(url.as_str()).unwrap();
            (path, uri)
        };
        beancount_data.insert(path.clone(), Arc::new(bean_data));

        let edit_rope = Rope::from_str(edit_text);
        let mut edit_parser = Parser::new();
        edit_parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let edit_tree = edit_parser.parse(edit_text, None).unwrap();

        let mut forest = HashMap::new();
        forest.insert(path.clone(), Arc::new(edit_tree));

        let mut open_docs = HashMap::new();
        open_docs.insert(
            path.clone(),
            crate::document::Document {
                content: edit_rope,
                version: 0,
            },
        );

        let snapshot = LspServerStateSnapshot {
            beancount_data: Arc::new(beancount_data),
            config: crate::config::Config::new(PathBuf::from("/test")),
            forest: Arc::new(forest),
            forest_content: Arc::new(HashMap::new()),
            open_docs: Arc::new(open_docs),
            checker: None,
        };

        (snapshot, uri, path)
    }

    #[test]
    fn test_integration_narration_completion_not_payee() {
        let test_data = r#"
2026-01-01 * "PayeeOne" "NarrationOne"
2026-01-02 * "PayeeTwo" "NarrationTwo"
2026-01-03 * "PayeeThree" "NarrationThree"
"#;
        let edit_text = r#"2026-01-06 * "NewPayee" "Nar"#;
        let (snapshot, uri, _) = make_snapshot(test_data, edit_text);

        // Cursor position inside second string after "Nar"
        // Text: '2026-01-06 * "NewPayee" "Nar"'
        //        012345678901234567890123456 7
        //                                  ^27 = after "Nar"
        let position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: lsp_types::Position {
                line: 0,
                character: 27,
            },
        };

        let result = completion(
            snapshot,
            lsp_types::CompletionParams {
                text_document_position_params: position,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            },
        )
        .unwrap();
        assert!(result.is_some(), "Should return completion items");

        let lsp_types::CompletionResponse::CompletionList(list) = result.unwrap() else {
            panic!("Expected CompletionResponse::CompletionList");
        };
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();

        // Should contain NARRATIONS
        assert!(
            labels.contains(&"NarrationOne"),
            "Should contain narration: {:?}",
            labels
        );
        assert!(
            labels.contains(&"NarrationTwo"),
            "Should contain narration: {:?}",
            labels
        );
        assert!(
            labels.contains(&"NarrationThree"),
            "Should contain narration: {:?}",
            labels
        );

        // Should NOT contain PAYEES
        assert!(
            !labels.contains(&"PayeeOne"),
            "Should NOT contain payee in narration context: {:?}",
            labels
        );
        assert!(
            !labels.contains(&"PayeeTwo"),
            "Should NOT contain payee in narration context: {:?}",
            labels
        );
        assert!(
            !labels.contains(&"PayeeThree"),
            "Should NOT contain payee in narration context: {:?}",
            labels
        );
    }

    #[test]
    fn test_balance_completion_lowercase_prefix() {
        let test_data = r#"
2026-01-01 open Assets:Checking
2026-01-01 open Assets:Savings
2026-01-01 open Liabilities:CreditCard
2026-01-01 open Liabilities:Loan
"#;
        let edit_text = r#"2026-01-06 balance lia"#;
        let (snapshot, uri, _) = make_snapshot(test_data, edit_text);

        // Cursor position after "lia"
        let position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: lsp_types::Position {
                line: 0,
                character: 22,
            },
        };

        let result = completion(
            snapshot,
            lsp_types::CompletionParams {
                text_document_position_params: position,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            },
        )
        .unwrap();
        assert!(
            result.is_some(),
            "Should return completion items for lowercase prefix"
        );

        let lsp_types::CompletionResponse::CompletionList(list) = result.unwrap() else {
            panic!("Expected CompletionResponse::CompletionList");
        };
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();

        // Should contain Liabilities accounts (case-insensitive match)
        assert!(
            labels.contains(&"Liabilities:CreditCard"),
            "Should contain Liabilities:CreditCard for lowercase 'lia' prefix. Found: {:?}",
            labels
        );
        assert!(
            labels.contains(&"Liabilities:Loan"),
            "Should contain Liabilities:Loan for lowercase 'lia' prefix. Found: {:?}",
            labels
        );

        // Liabilities accounts should be ranked higher (appear first) due to prefix match
        let liabilities_cc_pos = labels.iter().position(|&l| l == "Liabilities:CreditCard");
        let assets_checking_pos = labels.iter().position(|&l| l == "Assets:Checking");

        if let (Some(lia_pos), Some(assets_pos)) = (liabilities_cc_pos, assets_checking_pos) {
            assert!(
                lia_pos < assets_pos,
                "Liabilities:CreditCard should be ranked higher than Assets:Checking for 'lia' prefix. Order: {:?}",
                labels
            );
        }
    }

    #[test]
    fn test_completion_with_renamed_accounts() {
        // Test for issue #672: Support option "name_..." for renamed account types
        let test_data = r#"
option "name_assets" "Aktiva"
option "name_expenses" "Aufwendungen"

2025-01-01 open Aktiva:Bank USD
2025-01-01 open Aufwendungen:Food USD
"#;
        // Verify that accounts are extracted with custom names
        let rope = Rope::from_str(test_data);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(test_data, None).unwrap();
        let bean_data = BeancountData::new(&Arc::new(tree), &rope);
        let accounts = bean_data.get_accounts();
        assert!(
            accounts.contains(&"Aktiva:Bank".to_string()),
            "Should extract account with custom name 'Aktiva:Bank'"
        );
        assert!(
            accounts.contains(&"Aufwendungen:Food".to_string()),
            "Should extract account with custom name 'Aufwendungen:Food'"
        );

        let edit_text = r#"2025-01-02 * "Shopping"
  Akti"#;
        let (snapshot, uri, _) = make_snapshot(test_data, edit_text);

        // Cursor position after "Akti" on line 1
        let position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: lsp_types::Position {
                line: 1,
                character: 6,
            },
        };

        let result = completion(
            snapshot,
            lsp_types::CompletionParams {
                text_document_position_params: position,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            },
        )
        .unwrap();
        assert!(
            result.is_some(),
            "Should return completion items for custom account name prefix 'Akti'"
        );

        let lsp_types::CompletionResponse::CompletionList(list) = result.unwrap() else {
            panic!("Expected CompletionResponse::CompletionList");
        };
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();

        println!("Completion items for 'Akti' prefix: {:?}", labels);

        // Should contain the custom account name
        assert!(
            labels.contains(&"Aktiva:Bank"),
            "Should contain 'Aktiva:Bank' for prefix 'Akti'. Found: {:?}",
            labels
        );

        // Verify Aktiva:Bank is ranked highly (should be first or near first)
        let aktiva_pos = labels.iter().position(|&l| l == "Aktiva:Bank");
        assert!(
            aktiva_pos.is_some(),
            "Aktiva:Bank should be in completion results"
        );
        assert!(
            aktiva_pos.unwrap() < 3,
            "Aktiva:Bank should be ranked in top 3 for prefix 'Akti', found at position {:?}",
            aktiva_pos
        );
    }
}
