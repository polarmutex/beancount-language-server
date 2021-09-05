use crate::core;
use chrono::{Datelike, NaiveDate};
use log::debug;
use lspower::lsp;
use std::sync::Arc;

/// Provider function for LSP ``.
pub async fn completion(
    session: Arc<core::Session>,
    params: lsp::CompletionParams,
) -> anyhow::Result<Option<lsp::CompletionResponse>> {
    debug!("providers::completion");

    let tree = session
        .get_mut_tree(&params.text_document_position.text_document.uri)
        .await?;
    let mut tree = tree.lock().await;
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
            if trigger_character.is_some() {
                debug!("providers::completion - handle trigger char");
                match trigger_character.unwrap().as_str() {
                    "2" => complete_date(),
                    _ => Ok(None),
                }
            } else {
                debug!("providers::completion - handle node");
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

fn complete_date() -> anyhow::Result<Option<lsp::CompletionResponse>> {
    let today = chrono::offset::Local::now().naive_local().date();
    let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
    let cur_month = today.format("%Y-%m-").to_string();
    let next_month = add_one_month(today).format("%Y-%m-").to_string();
    let today = today.format("%Y-%m-%d").to_string();
    Ok(Some(lsp::CompletionResponse::Array(vec![
        lsp::CompletionItem::new_simple(today, "today".to_string()),
        lsp::CompletionItem::new_simple(cur_month, "this month".to_string()),
        lsp::CompletionItem::new_simple(prev_month, "prev month".to_string()),
        lsp::CompletionItem::new_simple(next_month, "next month".to_string()),
    ])))
}

fn add_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    let day = date.day();
    if month == 12 {
        year += 1;
        month = 1;
    } else {
        month += 1;
    }
    chrono::NaiveDate::from_ymd(year, month, day)
}

fn sub_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    let day = date.day();
    if month == 1 {
        year -= 1;
        month = 12;
    } else {
        month += 1;
    }
    chrono::NaiveDate::from_ymd(year, month, day)
}
