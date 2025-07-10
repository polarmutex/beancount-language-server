use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use chrono::Datelike;
use lsp_types::CompletionItem;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

/// Provider function for LSP completion.
pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    trigger_character: Option<char>,
    cursor: lsp_types::TextDocumentPositionParams,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion");

    let uri = &cursor.text_document.uri.to_file_path().unwrap();
    let line = &cursor.position.line;
    let char = &cursor.position.character;
    debug!("providers::completion - line {} char {}", line, char);

    let tree = snapshot.forest.get(uri).unwrap();
    let doc = snapshot.open_docs.get(uri).unwrap();
    let content = doc.clone().content;

    let start = tree_sitter::Point {
        row: *line as usize,
        column: if *char == 0 {
            *char as usize
        } else {
            *char as usize - 1
        },
    };
    let end = tree_sitter::Point {
        row: *line as usize,
        column: *char as usize,
    };
    let node = tree
        .root_node()
        .named_descendant_for_point_range(start, end);
    debug!("providers::completion - node {:?}", node);

    // Extract the current prefix for filtering completions
    let current_line_text = content.line(*line as usize).to_string();
    let prefix = extract_completion_prefix(&current_line_text, *char as usize);
    debug!("providers::completion - prefix: '{}'", prefix);

    let prev_sibling_node = match node {
        Some(node) => node.prev_sibling(),
        None => None,
    };
    debug!(
        "providers::completion - prev sibling node {:?}",
        prev_sibling_node
    );

    let prev_named_sibling_node = match node {
        Some(node) => node.prev_named_sibling(),
        None => None,
    };
    debug!(
        "providers::completion - prev named sibling node {:?}",
        prev_named_sibling_node
    );

    let parent_node = match node {
        Some(node) => node.parent(),
        None => None,
    };
    debug!("providers::completion - parent node {:?}", parent_node);

    if let Some(char) = trigger_character {
        debug!(
            "providers::completion - handle trigger_character {:?}",
            trigger_character
        );
        match char {
            '2' => complete_date(),
            '"' => {
                if prev_sibling_node.is_some() && prev_sibling_node.unwrap().kind() == "txn" {
                    complete_narration(snapshot.beancount_data)
                } else {
                    Ok(None)
                }
            }
            '#' => complete_tag(snapshot.beancount_data),
            '^' => complete_link(snapshot.beancount_data),
            ':' => {
                // Handle colon in account names - continue account completion
                complete_account_with_prefix(snapshot.beancount_data, &prefix)
            }
            _ => Ok(None),
        }
    } else {
        debug!("providers::completion - handle node {:?}", node);
        match node {
            Some(node) => {
                let text = text_for_tree_sitter_node(&content, &node);
                debug!("providers::completion - text {}", text);

                debug!("providers::completion - handle node - {}", node.kind());

                //if parent_parent_node.is_some()
                //    && parent_parent_node.unwrap().kind() == "posting_or_kv_list"
                //    && *char < 10
                //{
                //   complete_account(snapshot.beancount_data)
                //} else {
                match node.kind() {
                    "ERROR" => {
                        debug!("providers::completion - handle node - handle error");
                        debug!(
                            "providers::completion - handle node - handle error {}",
                            text
                        );
                        // For ERROR nodes, try account completion as it might be an incomplete account name
                        complete_account_with_prefix(snapshot.beancount_data, &prefix)
                    }
                    "identifier" => {
                        debug!("providers::completion - handle node - handle identifier");
                        if prev_sibling_node.is_some()
                            && prev_sibling_node.unwrap().kind() == "date"
                        {
                            complete_kind()
                        } else {
                            // if parent_parent_node.is_some() && parent_parent_node.unwrap().kind() ==
                            // "posting_or_kv_list" {
                            complete_account_with_prefix(snapshot.beancount_data, &prefix)
                            //} else {
                            //    Ok(None)
                        }
                    }
                    "narration" => {
                        debug!("providers::completion - handle node - handle narration");
                        complete_narration(snapshot.beancount_data)
                    }
                    "payee" => {
                        debug!("providers::completion - handle node - handle payee");
                        complete_narration(snapshot.beancount_data)
                    }
                    _ => Ok(None),
                }
                //}
            }
            None => Ok(None),
        }
    }
}

fn complete_date() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::date");
    let today = chrono::offset::Local::now().naive_local().date();
    let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
    debug!("providers::completion::date {}", prev_month);
    let cur_month = today.format("%Y-%m-").to_string();
    debug!("providers::completion::date {}", cur_month);
    let next_month = add_one_month(today).format("%Y-%m-").to_string();
    debug!("providers::completion::date {}", next_month);
    let today = today.format("%Y-%m-%d").to_string();
    debug!("providers::completion::date {}", today);
    let items = vec![
        lsp_types::CompletionItem {
            label: today,
            detail: Some("today".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: cur_month,
            detail: Some("this month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: prev_month,
            detail: Some("prev month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: next_month,
            detail: Some("next month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

fn complete_kind() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::kind");
    let items = vec![
        lsp_types::CompletionItem {
            label: String::from("txn"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("balance"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("open"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("close"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

pub fn add_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 12 {
        year += 1;
        month = 1;
    } else {
        month += 1;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid date")
}

pub fn sub_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 1 {
        year -= 1;
        month = 12;
    } else {
        month -= 1;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid date")
}

fn complete_narration(
    data: HashMap<PathBuf, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::narration");

    let completions: Vec<CompletionItem> = data
        .values()
        .flat_map(|d| {
            d.get_narration().iter().map(|n| lsp_types::CompletionItem {
                label: n.clone(),
                detail: Some("Beancount Narration".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            })
        })
        .collect();

    Ok(Some(completions))
}

fn complete_account_with_prefix(
    data: HashMap<PathBuf, BeancountData>,
    prefix: &str,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::account");
    let prefix_lower = prefix.to_lowercase();

    let mut completions: Vec<CompletionItem> = data
        .values()
        .flat_map(|d| {
            d.accounts_definitions
                .keys()
                // Case-insensitive prefix matching
                .filter(|&s| prefix.is_empty() || s.to_lowercase().starts_with(&prefix_lower))
                .map(|n| lsp_types::CompletionItem {
                    label: n.clone(),
                    detail: Some("Beancount Account".to_string()),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    // Set filter_text to enable proper LSP client filtering
                    filter_text: Some(n.clone()),
                    ..Default::default()
                })
        })
        .collect();
    completions.dedup();
    completions.sort_by(|x, y| x.label.cmp(&y.label));
    Ok(Some(completions))
}

/// Extract the current word/prefix being typed for completion
fn extract_completion_prefix(line_text: &str, cursor_char: usize) -> String {
    let chars: Vec<char> = line_text.chars().collect();
    if cursor_char == 0 || cursor_char > chars.len() {
        return String::new();
    }

    let mut start = cursor_char.saturating_sub(1);

    // Find the start of the current word (account name)
    // Account names can contain letters, numbers, colons, and hyphens
    while start > 0 {
        let c = chars[start.saturating_sub(1)];
        if !c.is_alphanumeric() && c != ':' && c != '-' && c != '_' {
            break;
        }
        start = start.saturating_sub(1);
    }

    // Extract the prefix from start to cursor
    let end = cursor_char.min(chars.len());
    chars[start..end].iter().collect()
}

fn complete_tag(
    data: HashMap<PathBuf, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for tag in data.get_tags() {
            completions.push(lsp_types::CompletionItem {
                label: tag,
                detail: Some("Beancount Tag".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

fn complete_link(
    data: HashMap<PathBuf, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for link in data.get_links() {
            completions.push(lsp_types::CompletionItem {
                label: link,
                detail: Some("Beancount Link".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

#[cfg(test)]
mod tests {
    use crate::providers::completion::completion;
    use crate::{
        providers::completion::{add_one_month, sub_one_month},
        test_utils::TestState,
    };

    #[test]
    fn handle_sub_one_month() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 5, 1).expect("valid date");
        assert_eq!(sub_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_sub_one_month_in_jan() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
        assert_eq!(sub_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_add_one_month() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 7, 1).expect("valid date");
        assert_eq!(add_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_add_one_month_in_dec() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        assert_eq!(add_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_date_completion() {
        let fixure = r#"
%! /main.beancount
2
|
^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let text_document_position = test_state.cursor().unwrap();
        println!(
            "{} {}",
            text_document_position.position.line, text_document_position.position.character
        );
        let items = completion(test_state.snapshot, Some('2'), text_document_position)
            .unwrap()
            .unwrap_or_default();
        let today = chrono::offset::Local::now().naive_local().date();
        let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
        let cur_month = today.format("%Y-%m-").to_string();
        let next_month = add_one_month(today).format("%Y-%m-").to_string();
        let today = today.format("%Y-%m-%d").to_string();
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: today,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("today")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: cur_month,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("this month")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: prev_month,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("prev month")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: next_month,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("next month")),
                    ..Default::default()
                }
            ]
        )
    }

    #[test]
    fn handle_txn_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 t
            |
            ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: String::from("txn"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("balance"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("open"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("close"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
            ]
        )
    }

    #[test]
    fn handle_narration_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("\"Test Co\""),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Narration")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_payee_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "Test" "
                       |
                       ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(items, [])
    }

    #[test]
    fn handle_account_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    a
     |
     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("Assets:Test"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Account")),
                filter_text: Some(String::from("Assets:Test")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_account_completion_with_colon() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Assets:Checking USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:
           |
           ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some(':'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: String::from("Assets:Checking"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("Beancount Account")),
                    filter_text: Some(String::from("Assets:Checking")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("Assets:Test"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("Beancount Account")),
                    filter_text: Some(String::from("Assets:Test")),
                    ..Default::default()
                },
            ]
        )
    }

    #[test]
    fn handle_case_insensitive_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Asse
        |
        ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("Assets:Test"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Account")),
                filter_text: Some(String::from("Assets:Test")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_account_completion_case_sensitive() {
        let _ = env_logger::builder().is_test(true).try_init();

        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    A
     |
     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("Assets:Test"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Account")),
                filter_text: Some(String::from("Assets:Test")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_tag_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar" #tag ^link
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn  "Test Co" "Foo Bar" #
                                     |
                                     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('#'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("#tag"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Tag")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_link_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar" #tag ^link
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn  "Test Co" "Foo Bar" #
                                     |
                                     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('^'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("^link"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Link")),
                ..Default::default()
            },]
        )
    }
}
