use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use anyhow::Result;
use chrono::Datelike;
use lsp_types::Url;
use std::collections::HashMap;
use tracing::debug;

/// Provider function for LSP ``.
pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    trigger_character: Option<char>,
    uri: &Url,
    line: &u32,
    char: &u32,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion");

    let tree = snapshot.forest.get(&uri).unwrap();
    let doc = snapshot.open_docs.get(&uri).unwrap();
    let content = doc.clone().content;
    debug!("providers::completion - line {}", line);
    debug!("providers::completion - char {}", char);
    let start = tree_sitter::Point {
        row: *line as usize,
        column: if *char == 0 {
            *char as usize
        } else {
            *char as usize - 1
        },
    };
    debug!("providers::completion - start {}", start);
    let end = tree_sitter::Point {
        row: *line as usize,
        column: *char as usize,
    };
    debug!("providers::completion - end {}", end);
    debug!(
        "providers::completion - is_char_triggered {:?}",
        trigger_character
    );
    let node = tree
        .root_node()
        .named_descendant_for_point_range(start, end);
    debug!("providers::completion - node {:?}", node);

    match node {
        Some(node) => {
            let text = text_for_tree_sitter_node(&content, &node);
            debug!("providers::completion - text {}", text);
            let parent_node = node.parent();
            debug!("providers::completion - parent node {:?}", parent_node);
            let mut parent_parent_node = None;
            if let Some(pnode) = parent_node {
                parent_parent_node = pnode.parent();
            }
            debug!(
                "providers::completion - parent node {:?}",
                parent_parent_node
            );
            let prev_sibling_node = node.prev_sibling();
            debug!(
                "providers::completion - prev sibling node {:?}",
                prev_sibling_node
            );
            let prev_named_node = node.prev_named_sibling();
            debug!(
                "providers::completion - prev named node {:?}",
                prev_named_node
            );

            if let Some(char) = trigger_character {
                debug!("providers::completion - handle trigger char");
                match char {
                    '2' => complete_date(),
                    _ => Ok(None),
                }
            } else {
                debug!("providers::completion - handle node");
                if parent_parent_node.is_some()
                    && parent_parent_node.unwrap().kind() == "posting_or_kv_list"
                    && *char < 10
                {
                    complete_account(snapshot.beancount_data)
                } else {
                    match node.kind() {
                        "ERROR" => {
                            debug!("providers::completion - handle node - handle error");
                            debug!(
                                "providers::completion - handle node - handle error {}",
                                text
                            );
                            let prefix = text.chars().next().unwrap();
                            debug!("providers::completion - handle node - prefix {}", prefix);
                            if prefix == '"' {
                                complete_txn_string(snapshot.beancount_data)
                            } else {
                                Ok(None)
                            }
                        }
                        "identifier" => {
                            debug!("providers::completion - handle node - handle identifier");
                            // if parent_parent_node.is_some() && parent_parent_node.unwrap().kind() ==
                            // "posting_or_kv_list" {
                            complete_account(snapshot.beancount_data)
                            //} else {
                            //    Ok(None)
                            //}
                        }
                        "narration" => {
                            debug!("providers::completion - handle node - handle string");
                            //if parent_node.is_some() && parent_node.unwrap().kind() == "txn_strings"
                            //{
                            complete_txn_string(snapshot.beancount_data)
                            //} else {
                            //    Ok(None)
                            //}
                        }
                        _ => Ok(None),
                    }
                }
            }
        }
        None => Ok(None),
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
    let mut items = Vec::new();
    items.push(lsp_types::CompletionItem::new_simple(
        today,
        "today".to_string(),
    ));
    items.push(lsp_types::CompletionItem::new_simple(
        cur_month,
        "this month".to_string(),
    ));
    items.push(lsp_types::CompletionItem::new_simple(
        prev_month,
        "prev month".to_string(),
    ));
    items.push(lsp_types::CompletionItem::new_simple(
        next_month,
        "next month".to_string(),
    ));
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

fn complete_txn_string(
    data: HashMap<lsp_types::Url, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::account");
    let mut completions = Vec::new();
    for data in data.values() {
        for txn_string in data.get_txn_strings() {
            completions.push(lsp_types::CompletionItem::new_simple(
                txn_string,
                "".to_string(),
            ));
        }
    }
    Ok(Some(completions))
}

fn complete_account(
    data: HashMap<lsp_types::Url, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::account");
    let mut completions = Vec::new();
    for data in data.values() {
        for account in data.get_accounts() {
            completions.push(lsp_types::CompletionItem::new_simple(
                account,
                "Beancount Account".to_string(),
            ));
        }
    }
    Ok(Some(completions))
}
