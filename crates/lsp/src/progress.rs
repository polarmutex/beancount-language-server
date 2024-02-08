use async_lsp::ClientSocket;
use lsp_types::notification as notif;
use lsp_types::request as req;
use lsp_types::NumberOrString;
use lsp_types::ProgressParams;
use lsp_types::ProgressParamsValue;
use lsp_types::WorkDoneProgress;
use lsp_types::WorkDoneProgressBegin;
use lsp_types::WorkDoneProgressCreateParams;
use lsp_types::WorkDoneProgressEnd;
use lsp_types::WorkDoneProgressReport;

// trait ClientExt: BorrowMut<ClientSocket> {
//     fn show_message_ext(&mut self, typ: MessageType, msg: impl fmt::Display) {
//         // Maybe connect all tracing::* to LSP ShowMessage?
//         let _: Result<_, _> = self.borrow_mut().show_message(ShowMessageParams {
//             typ,
//             message: msg.to_string(),
//         });
//     }
// }
// impl ClientExt for ClientSocket {}

// #[derive(Debug, Eq, PartialEq)]
pub(crate) struct Progress {
    client: ClientSocket,
    token: Option<String>,
}

impl Progress {
    pub async fn new(client: &ClientSocket, token: String) -> Self {
        let created = client
            .request::<req::WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                token: NumberOrString::String(token.clone()),
            })
            .await
            .is_ok();
        Self {
            client: client.clone(),
            token: created.then_some(token),
        }
    }

    pub fn begin(&self, title: String, message: impl Into<Option<String>>) {
        self.notify(WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: title.to_string(),
            cancellable: None,
            message: message.into(),
            percentage: Some(0),
        }));
    }

    fn notify(&self, progress: WorkDoneProgress) {
        let Some(token) = &self.token else { return };
        let _: Result<_, _> = self.client.notify::<notif::Progress>(ProgressParams {
            token: NumberOrString::String(token.clone()),
            value: ProgressParamsValue::WorkDone(progress),
        });
    }

    pub fn report(&self, percentage: u32, message: String) {
        assert!((0..=100).contains(&percentage));
        self.notify(WorkDoneProgress::Report(WorkDoneProgressReport {
            cancellable: None,
            message: Some(message),
            percentage: Some(percentage),
        }));
    }

    pub fn done(mut self, message: Option<String>) {
        self.notify(WorkDoneProgress::End(WorkDoneProgressEnd { message }));
        // Don't drop again.
        self.token = None;
    }
    // Builds a fractional progress value
    // pub(crate) fn fraction(done: usize, total: usize) -> f64 {
    //     assert!(done <= total);
    //     done as f64 / total.max(1) as f64
    // }
}

// impl LspServerState {
//     // Reports progress to the user via the `WorkDoneProgress` protocol.
//     pub(crate) fn report_progress(
//         &mut self,
//         title: &str,
//         state: Progress,
//         message: Option<String>,
//         fraction: Option<f64>,
//     ) {
//         // TODO: Ensure that the client supports WorkDoneProgress
//
//         let percentage = fraction.map(|f| {
//             (0.0..=1.0).contains(&f);
//             (f * 100.0) as u32
//         });
//         let token = lsp_types::ProgressToken::String(format!("mun/{}", title));
//         let work_done_progress = match state {
//             Progress::Begin => {
//                 self.send_request::<lsp_types::request::WorkDoneProgressCreate>(
//                     lsp_types::WorkDoneProgressCreateParams {
//                         token: token.clone(),
//                     },
//                     |_, _| (),
//                 );
//
//                 lsp_types::WorkDoneProgress::Begin(lsp_types::WorkDoneProgressBegin {
//                     title: title.into(),
//                     cancellable: None,
//                     message,
//                     percentage,
//                 })
//             }
//             Progress::Report => {
//                 lsp_types::WorkDoneProgress::Report(lsp_types::WorkDoneProgressReport {
//                     cancellable: None,
//                     message,
//                     percentage,
//                 })
//             }
//             Progress::End => {
//                 lsp_types::WorkDoneProgress::End(lsp_types::WorkDoneProgressEnd { message })
//             }
//         };
//         self.send_notification::<lsp_types::notification::Progress>(lsp_types::ProgressParams {
//             token,
//             value: lsp_types::ProgressParamsValue::WorkDone(work_done_progress),
//         });
//     }
// }
/*pub async fn progress_begin(client: &Client, title: &str) -> ProgressToken {
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
*/
