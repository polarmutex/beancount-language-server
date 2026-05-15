use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils;
use anyhow::Result;

pub(crate) fn code_action(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::CodeActionParams,
) -> Result<Option<Vec<lsp_types::CodeActionResponse>>> {
    let uri = &params.text_document.uri;

    let (tree, doc) = match snapshot.tree_and_document_for_uri(uri) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    Ok(Some(sort_transactions(tree, doc, uri)?))
}

fn sort_transactions(
    tree: &tree_sitter_beancount::tree_sitter::Tree,
    doc: &Document,
    uri: &lsp_types::Uri,
) -> Result<Vec<lsp_types::CodeActionResponse>> {
    let content_str = doc.content.to_string();
    let root = tree.root_node();

    let mut txn_info: Vec<(String, usize, usize)> = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "transaction" => {
                if let Some(date_node) = child.child_by_field_name("date") {
                    let date_str = content_str[date_node.byte_range()].to_string();
                    txn_info.push((date_str, child.start_byte(), child.end_byte()));
                }
            }
            "comment" => {}
            _ => return Ok(vec![]),
        }
    }

    if txn_info.len() < 2 {
        return Ok(vec![]);
    }

    let is_sorted = txn_info.windows(2).all(|w| w[0].0 <= w[1].0);
    if is_sorted {
        return Ok(vec![]);
    }

    let mut txn_chunks: Vec<(String, String)> = txn_info
        .iter()
        .map(|(date, start, end)| (date.clone(), content_str[*start..*end].to_string()))
        .collect();
    txn_chunks.sort_by(|a, b| a.0.cmp(&b.0));

    let sorted_texts: Vec<&str> = txn_chunks.iter().map(|(_, t)| t.as_str()).collect();
    let sorted_text = sorted_texts.join("\n");

    let block_start = txn_info[0].1;
    let block_end = txn_info.last().unwrap().2;

    let start_pos = treesitter_utils::byte_to_lsp_position(&doc.content, block_start);
    let end_pos = treesitter_utils::byte_to_lsp_position(&doc.content, block_end);

    let text_edit = lsp_types::TextEdit {
        range: lsp_types::Range {
            start: start_pos,
            end: end_pos,
        },
        new_text: sorted_text,
    };

    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), vec![text_edit]);

    let workspace_edit = lsp_types::WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    };

    let action = lsp_types::CodeAction {
        title: "Sort transactions by date".to_string(),
        kind: Some(lsp_types::CodeActionKind::Source),
        edit: Some(workspace_edit),
        ..Default::default()
    };

    Ok(vec![lsp_types::CodeActionResponse::CodeAction(action)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use crate::server::LspServerStateSnapshot;
    use std::collections::HashMap;
    use std::str::FromStr;
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

            let snapshot = LspServerStateSnapshot {
                beancount_data: Arc::new(beancount_data),
                config: Config::new(std::env::current_dir()?),
                forest: Arc::new(forest),
                forest_content: Arc::new(HashMap::new()),
                open_docs: Arc::new(open_docs),
                checker: None,
            };

            Ok(TestState { snapshot })
        }

        fn invoke(&self) -> anyhow::Result<Vec<lsp_types::CodeActionResponse>> {
            let path = std::env::current_dir()?.join("test.beancount");
            let url = url::Url::from_file_path(&path)
                .map_err(|_| anyhow::anyhow!("Failed to convert path to URL"))?;
            let uri = lsp_types::Uri::from_str(url.as_str())
                .map_err(|e| anyhow::anyhow!("Failed to create URI: {:?}", e))?;

            let params = lsp_types::CodeActionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                range: lsp_types::Range::default(),
                context: lsp_types::CodeActionContext {
                    diagnostics: vec![],
                    only: None,
                    trigger_kind: None,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };

            let snapshot = LspServerStateSnapshot {
                beancount_data: self.snapshot.beancount_data.clone(),
                config: self.snapshot.config.clone(),
                forest: self.snapshot.forest.clone(),
                forest_content: self.snapshot.forest_content.clone(),
                open_docs: self.snapshot.open_docs.clone(),
                checker: self.snapshot.checker.clone(),
            };

            Ok(code_action(snapshot, params)?.unwrap_or_default())
        }
    }

    fn apply_code_action(content: &str, actions: &[lsp_types::CodeActionResponse]) -> String {
        for action in actions {
            if let lsp_types::CodeActionResponse::CodeAction(ca) = action
                && let Some(edit) = &ca.edit
                && let Some(changes) = &edit.changes
                && let Some(text_edits) = changes.values().next()
            {
                return apply_text_edits(content, text_edits);
            }
        }
        content.to_string()
    }

    fn apply_text_edits(content: &str, edits: &[lsp_types::TextEdit]) -> String {
        let rope = ropey::Rope::from_str(content);
        let mut sorted_edits = edits.to_vec();
        sorted_edits.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });

        let mut result = rope;
        for edit in sorted_edits {
            let start_line = edit.range.start.line as usize;
            let start_char = edit.range.start.character as usize;
            let end_line = edit.range.end.line as usize;
            let end_char = edit.range.end.character as usize;

            let start_char_idx = result.line_to_char(start_line) + start_char;
            let end_char_idx = result.line_to_char(end_line) + end_char;

            if start_char_idx < end_char_idx {
                result.remove(start_char_idx..end_char_idx);
            }
            if !edit.new_text.is_empty() {
                result.insert(start_char_idx, &edit.new_text);
            }
        }
        result.to_string()
    }

    #[test]
    fn test_already_sorted_returns_no_action() {
        let content = "2023-01-01 * \"First\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n\n2023-01-02 * \"Second\"\n  Assets:Cash  75.00 USD\n  Expenses:Food\n\n2023-01-03 * \"Third\"\n  Assets:Cash  100.00 USD\n  Expenses:Food\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert!(
            actions.is_empty(),
            "Already-sorted transactions should produce no code actions"
        );
    }

    #[test]
    fn test_unsorted_returns_sort_action() {
        let content = "2023-01-03 * \"Third\"\n  Assets:Cash  100.00 USD\n  Expenses:Food\n\n2023-01-01 * \"First\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert_eq!(
            actions.len(),
            1,
            "Unsorted transactions should produce one sort action"
        );
        if let lsp_types::CodeActionResponse::CodeAction(ca) = &actions[0] {
            assert_eq!(ca.title, "Sort transactions by date");
        } else {
            panic!("Expected CodeAction");
        }
    }

    #[test]
    fn test_sort_action_produces_sorted_output() {
        let content = "2023-01-03 * \"Third\"\n  Assets:Cash  100.00 USD\n  Expenses:Food\n\n2023-01-01 * \"First\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n\n2023-01-02 * \"Second\"\n  Assets:Cash  75.00 USD\n  Expenses:Food\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert!(!actions.is_empty(), "Should produce a sort action");

        let sorted = apply_code_action(content, &actions);

        let date_lines: Vec<&str> = sorted
            .lines()
            .filter(|l| l.starts_with("2023-01-"))
            .map(|l| &l[..10])
            .collect();

        assert!(!date_lines.is_empty());
        let mut expected = date_lines.clone();
        expected.sort();
        assert_eq!(
            date_lines, expected,
            "Dates should appear in sorted order in the output"
        );
    }

    #[test]
    fn test_mixed_directives_produces_no_action() {
        let content = "2023-01-01 open Assets:Cash USD\n\n2023-01-03 * \"Third\"\n  Assets:Cash  100.00 USD\n  Expenses:Food\n\n2023-01-01 * \"First\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert!(
            actions.is_empty(),
            "Files with non-transaction directives should produce no actions"
        );
    }

    #[test]
    fn test_single_transaction_produces_no_action() {
        let content = "2023-01-01 * \"Only one\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert!(
            actions.is_empty(),
            "A single transaction has nothing to sort"
        );
    }

    #[test]
    fn test_sort_preserves_transaction_content() {
        let content = "2023-01-02 * \"Payee\" \"Narration\" #tag\n  Assets:Cash:Checking  -100.00 USD\n  Expenses:Groceries   100.00 USD\n\n2023-01-01 * \"Earlier\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert!(!actions.is_empty());

        let sorted = apply_code_action(content, &actions);
        assert!(
            sorted.contains("\"Payee\" \"Narration\" #tag"),
            "Tags and payee should be preserved"
        );
        assert!(
            sorted.contains("Assets:Cash:Checking"),
            "Account names should be preserved"
        );
        assert!(
            sorted.contains("-100.00 USD"),
            "Amounts should be preserved"
        );
    }

    #[test]
    fn test_empty_file_produces_no_action() {
        let actions = TestState::new("").unwrap().invoke().unwrap();
        assert!(actions.is_empty(), "Empty file should produce no actions");
    }

    #[test]
    fn test_same_date_transactions_no_action() {
        let content = "2023-01-01 * \"First\"\n  Assets:Cash  50.00 USD\n  Expenses:Food\n\n2023-01-01 * \"Second\"\n  Assets:Cash  75.00 USD\n  Expenses:Other\n";
        let actions = TestState::new(content).unwrap().invoke().unwrap();
        assert!(
            actions.is_empty(),
            "Same-date transactions are already in valid order"
        );
    }
}
