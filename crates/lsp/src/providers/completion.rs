use crate::{core, core::RopeExt, session::Session};
use chrono::Datelike;
use log::debug;
use tower_lsp::lsp_types;

/// Provider function for LSP ``.
pub(crate) async fn completion(
    session: &Session,
    params: lsp_types::CompletionParams,
) -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
    debug!("providers::completion");

    let uri = params.text_document_position.text_document.uri;
    let tree = session.get_mut_tree(&uri).await?;
    let tree = tree.lock().await;
    let doc = session.get_document(&uri).await?;
    let content = doc.clone().content;
    let line = params.text_document_position.position.line as usize;
    debug!("providers::completion - line {}", line);
    let char = params.text_document_position.position.character as usize;
    debug!("providers::completion - char {}", char);
    let start = tree_sitter::Point {
        row: line,
        column: if char == 0 { char } else { char - 1 },
    };
    debug!("providers::completion - start {}", start);
    let end = tree_sitter::Point {
        row: line,
        column: char,
    };
    debug!("providers::completion - end {}", end);
    let trigger_character = params.context.and_then(|c| c.trigger_character).and_then(|c| {
        // Make sure 2 trigger only for first col
        if c == "2" {
            debug!("checking 2 - {}", char);
            if char > 1 {
                debug!("clearing 2");
                None
            } else {
                debug!("keeping 2");
                Some(c)
            }
        } else {
            None
        }
    });
    debug!("providers::completion - is_char_triggered {:?}", trigger_character);
    let node = tree.root_node().named_descendant_for_point_range(start, end);
    debug!("providers::completion - node {:?}", node);

    match node {
        Some(node) => {
            let text = &content.utf8_text_for_tree_sitter_node(&node);
            debug!("providers::completion - text {}", text);
            let parent_node = node.parent();
            debug!("providers::completion - parent node {:?}", parent_node);
            let mut parent_parent_node = None;
            if parent_node.is_some() {
                parent_parent_node = parent_node.unwrap().parent();
            }
            debug!("providers::completion - parent node {:?}", parent_parent_node);
            let prev_sibling_node = node.prev_sibling();
            debug!("providers::completion - prev sibling node {:?}", prev_sibling_node);
            let prev_named_node = node.prev_named_sibling();
            debug!("providers::completion - prev named node {:?}", prev_named_node);

            if trigger_character.is_some() {
                debug!("providers::completion - handle trigger char");
                match trigger_character.unwrap().as_str() {
                    "2" => complete_date(),
                    _ => Ok(None),
                }
            } else {
                debug!("providers::completion - handle node");
                if parent_parent_node.is_some()
                    && parent_parent_node.unwrap().kind() == "posting_or_kv_list"
                    && char < 10
                {
                    complete_account(&session.beancount_data)
                } else {
                    match node.kind() {
                        "ERROR" => {
                            debug!("providers::completion - handle node - handle error");
                            debug!("providers::completion - handle node - handle error {}", text);
                            let prefix = text.chars().next().unwrap();
                            debug!("providers::completion - handle node - prefix {}", prefix);
                            if prefix == '"' {
                                complete_txn_string(&session.beancount_data)
                            } else {
                                Ok(None)
                            }
                        },
                        "identifier" => {
                            debug!("providers::completion - handle node - handle identifier");
                            // if parent_parent_node.is_some() && parent_parent_node.unwrap().kind() ==
                            // "posting_or_kv_list" {
                            complete_account(&session.beancount_data)
                            //} else {
                            //    Ok(None)
                            //}
                        },
                        "string" => {
                            debug!("providers::completion - handle node - handle string");
                            if parent_node.is_some() && parent_node.unwrap().kind() == "txn_strings" {
                                complete_txn_string(&session.beancount_data)
                            } else {
                                Ok(None)
                            }
                        },
                        _ => Ok(None),
                    }
                }
            }
        },
        None => Ok(None),
    }
}

fn complete_date() -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
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
    Ok(Some(lsp_types::CompletionResponse::Array(vec![
        lsp_types::CompletionItem::new_simple(today, "today".to_string()),
        lsp_types::CompletionItem::new_simple(cur_month, "this month".to_string()),
        lsp_types::CompletionItem::new_simple(prev_month, "prev month".to_string()),
        lsp_types::CompletionItem::new_simple(next_month, "next month".to_string()),
    ])))
}

fn add_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 12 {
        year += 1;
        month = 1;
    } else {
        month += 1;
    }
    chrono::NaiveDate::from_ymd(year, month, 1)
}

fn sub_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 1 {
        year -= 1;
        month = 12;
    } else {
        month -= 1;
    }
    chrono::NaiveDate::from_ymd(year, month, 1)
}

fn complete_txn_string(data: &core::BeancountData) -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
    debug!("providers::completion::account");
    let mut completions = Vec::new();
    for txn_string in data.get_txn_strings() {
        completions.push(lsp_types::CompletionItem::new_simple(txn_string, "".to_string()));
    }
    Ok(Some(lsp_types::CompletionResponse::Array(completions)))
}

fn complete_account(data: &core::BeancountData) -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
    debug!("providers::completion::account");
    let mut completions = Vec::new();
    for account in data.get_accounts() {
        completions.push(lsp_types::CompletionItem::new_simple(
            account,
            "Beancount Account".to_string(),
        ));
    }
    Ok(Some(lsp_types::CompletionResponse::Array(completions)))
}
