use crate::beancount_data::BeancountData;
use crate::progress::Progress;
use crate::providers::completion;
use crate::providers::diagnostics;
use crate::providers::formatting;
use crate::server::LspServerStateSnapshot;
use anyhow::Result;
use async_lsp::lsp_types;
use async_lsp::ClientSocket;
use async_lsp::LanguageClient;
use lsp_types::PublishDiagnosticsParams;
use std::collections::HashMap;
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
