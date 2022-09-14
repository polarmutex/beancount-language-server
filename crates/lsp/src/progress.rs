use tower_lsp::lsp_types::notification::Progress;
use tower_lsp::lsp_types::request::WorkDoneProgressCreate;
use tower_lsp::lsp_types::{
    NumberOrString, ProgressParams, ProgressParamsValue, ProgressToken, WorkDoneProgress,
    WorkDoneProgressBegin, WorkDoneProgressCreateParams, WorkDoneProgressEnd,
    WorkDoneProgressReport,
};
use tower_lsp::Client;

pub async fn progress_begin(client: &Client, title: &str) -> ProgressToken {
    let token = NumberOrString::String(format!("beancount-language-server/{}", title));
    let begin = WorkDoneProgressBegin {
        title: title.to_string(),
        cancellable: Some(false),
        message: None,
        percentage: Some(100),
    };

    client
        .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
            token: token.clone(),
        })
        .await
        .unwrap();

    client
        .send_notification::<Progress>(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(begin)),
        })
        .await;
    token
}

pub async fn progress(client: &Client, token: ProgressToken, message: String) {
    let step = WorkDoneProgressReport {
        cancellable: Some(false),
        message: Some(message),
        percentage: None, //Some(pcnt),
    };
    client
        .send_notification::<Progress>(ProgressParams {
            token,
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(step)),
        })
        .await;
}

pub async fn progress_end(client: &Client, token: ProgressToken) {
    client
        .send_notification::<Progress>(ProgressParams {
            token,
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                message: Some("Finished parsing".to_string()),
            })),
        })
        .await;
}
