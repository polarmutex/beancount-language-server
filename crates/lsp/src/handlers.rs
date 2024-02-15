use crate::beancount_data::BeancountData;
use crate::document::Document;
use crate::progress::Progress;
use crate::providers::completion;
use crate::providers::diagnostics;
use crate::providers::formatting;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use anyhow::Result;
use async_lsp::lsp_types;
use async_lsp::ClientSocket;
use async_lsp::LanguageClient;
use itertools::Itertools;
use lsp_types::Location;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::ReferenceParams;
use lsp_types::WorkspaceEdit;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::CompletionParams,
) -> anyhow::Result<Option<lsp_types::CompletionResponse>> {
    let trigger_char = match &params.context {
        Some(context) => match &context.trigger_character {
            Some(trigger_character) => {
                if trigger_character == "2" {
                    if params.text_document_position.position.character > 1 {
                        None
                    } else {
                        trigger_character.chars().last()
                    }
                } else {
                    trigger_character.chars().last()
                }
            }
            None => None,
        },
        None => None,
    };
    let Some(items) = completion::completion(
        &snapshot.forest(),
        &snapshot.beancount_data(),
        &snapshot.open_docs(),
        trigger_char,
        params.text_document_position,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(lsp_types::CompletionResponse::Array(items)))
}

pub(crate) fn formatting(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::DocumentFormattingParams,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    formatting::formatting(snapshot, params)
}

pub(crate) fn ts_references(
    forest: &HashMap<lsp_types::Url, tree_sitter::Tree>,
    open_docs: &HashMap<lsp_types::Url, Document>,
    node_text: String,
) -> Vec<lsp_types::Location> {
    forest
        // .get(&uri)
        .iter()
        // .map(|x| (uri.clone(), x))
        .flat_map(|(url, tree)| {
            let query = match tree_sitter::Query::new(
                tree_sitter_beancount::language(),
                "(account)@account",
            ) {
                Ok(q) => q,
                Err(e) => return vec![],
            };
            let capture_account = query
                .capture_index_for_name("account")
                .expect("account should be captured");
            let text = if open_docs.get(&url).is_some() {
                open_docs.get(&url).unwrap().text().to_string()
            } else {
                fs::read_to_string(url.to_file_path().ok().unwrap()).expect("")
            };
            let source = text.as_bytes();
            tree_sitter::QueryCursor::new()
                .matches(&query, tree.root_node(), source)
                .filter_map(|m| {
                    let m = m.nodes_for_capture_index(capture_account).next()?;
                    let m_text = m.utf8_text(source).expect("");
                    if m_text == node_text {
                        Some((url.clone(), m.into()))
                    } else {
                        None
                    }
                })
                .collect()
            // vec![]
        })
        .map(|(url, node): (lsp_types::Url, tree_sitter::Node)| {
            let range = node.range();
            Location::new(
                url,
                lsp_types::Range {
                    start: lsp_types::Position {
                        line: range.start_point.row as u32,
                        character: range.start_point.column as u32,
                    },
                    end: lsp_types::Position {
                        line: range.end_point.row as u32,
                        character: range.end_point.column as u32,
                    },
                },
            )
        })
        // .filter(|x| true)
        .collect::<Vec<_>>()
}

pub(crate) fn references(
    snapshot: LspServerStateSnapshot,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let uri = params.text_document_position.text_document.uri;
    let line = params.text_document_position.position.line;
    let char = params.text_document_position.position.character;
    let forest = snapshot.forest();

    let start = tree_sitter::Point {
        row: line as usize,
        column: if char == 0 {
            char as usize
        } else {
            char as usize - 1
        },
    };
    let end = tree_sitter::Point {
        row: line as usize,
        column: char as usize,
    };

    let Some(node) = forest
        .get(&uri)
        .expect("to have tree found")
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };
    let content = snapshot.open_docs().get(&uri).unwrap().content.clone();
    let node_text = text_for_tree_sitter_node(&content, &node);
    let open_docs = snapshot.open_docs();

    let locs = ts_references(&forest, &open_docs, node_text);

    Ok(Some(locs))
}

pub(crate) fn rename(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::RenameParams,
) -> Result<Option<lsp_types::WorkspaceEdit>> {
    let uri = &params.text_document_position.text_document.uri;
    let line = &params.text_document_position.position.line;
    let char = &params.text_document_position.position.character;

    let forest = snapshot.forest();
    let tree = forest.get(uri).unwrap();
    let open_docs = snapshot.open_docs();
    let doc = open_docs.get(uri).unwrap();
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
    let Some(node) = forest
        .get(&uri)
        .expect("to have tree found")
        .root_node()
        .named_descendant_for_point_range(start, end)
    else {
        return Ok(None);
    };
    let content = snapshot.open_docs().get(&uri).unwrap().content.clone();
    let node_text = text_for_tree_sitter_node(&content, &node);

    let open_docs = snapshot.open_docs();
    let locs = ts_references(&forest, &open_docs, node_text);
    let new_name = params.new_name;

    let changes = locs
        .into_iter()
        .group_by(|t| t.uri.clone())
        .into_iter()
        .map(|(uri, g)| {
            let edits: Vec<_> = g
                // Send edits ordered from the back so we do not invalidate following positions.
                .sorted_by_key(|l| l.range.start)
                .rev()
                .map(|l| lsp_types::TextEdit::new(l.range, new_name.clone()))
                .collect();
            (uri, edits)
        })
        .collect();

    Ok(Some(WorkspaceEdit::new(changes)))
}

pub(crate) async fn handle_diagnostics(
    mut client: ClientSocket,
    forest: HashMap<lsp_types::Url, tree_sitter::Tree>,
    data: HashMap<lsp_types::Url, BeancountData>,
    path: PathBuf,
) {
    tracing::debug!("handlers::check_beancount");
    let bean_check_cmd = &PathBuf::from("bean-check");

    let progress = Progress::new(&client, String::from("blsp/check")).await;
    progress.begin(String::from("bean check"), String::from("check"));
    tokio::time::sleep(Duration::from_nanos(1)).await;

    let diags = diagnostics::diagnostics(data, bean_check_cmd, &path);

    progress.done(None);

    for file in forest.keys() {
        let diagnostics = if diags.contains_key(file) {
            diags.get(file).unwrap().clone()
        } else {
            vec![]
        };
        client
            .publish_diagnostics(PublishDiagnosticsParams {
                uri: file.clone(),
                diagnostics,
                version: None,
            })
            .expect("");
    }
}
