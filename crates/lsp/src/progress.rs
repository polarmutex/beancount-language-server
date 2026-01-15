use super::LspServerState;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Progress {
    Begin,
    Report,
    End,
}

impl Progress {
    /// Builds a fractional progress value
    pub(crate) fn fraction(done: usize, total: usize) -> f64 {
        assert!(done <= total);
        done as f64 / total.max(1) as f64
    }
}

impl LspServerState {
    // Reports progress to the user via the `WorkDoneProgress` protocol.
    pub(crate) fn report_progress(
        &mut self,
        title: &str,
        state: Progress,
        message: Option<String>,
        fraction: Option<f64>,
    ) {
        // TODO: Ensure that the client supports WorkDoneProgress

        let percentage = fraction.map(|f| {
            (0.0..=1.0).contains(&f);
            (f * 100.0) as u32
        });
        let token = lsp_types::ProgressToken::String(format!("beancount/{title}"));
        let work_done_progress = match state {
            Progress::Begin => {
                self.send_request::<lsp_types::request::WorkDoneProgressCreate>(
                    lsp_types::WorkDoneProgressCreateParams {
                        token: token.clone(),
                    },
                    |_, _| (),
                );

                lsp_types::WorkDoneProgress::Begin(lsp_types::WorkDoneProgressBegin {
                    title: title.into(),
                    cancellable: None,
                    message,
                    percentage,
                })
            }
            Progress::Report => {
                lsp_types::WorkDoneProgress::Report(lsp_types::WorkDoneProgressReport {
                    cancellable: None,
                    message,
                    percentage,
                })
            }
            Progress::End => {
                lsp_types::WorkDoneProgress::End(lsp_types::WorkDoneProgressEnd { message })
            }
        };
        self.send_notification::<lsp_types::notification::Progress>(lsp_types::ProgressParams {
            token,
            value: lsp_types::ProgressParamsValue::WorkDone(work_done_progress),
        });
    }
}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_fraction_basic() {
        assert_eq!(Progress::fraction(0, 10), 0.0);
        assert_eq!(Progress::fraction(5, 10), 0.5);
        assert_eq!(Progress::fraction(10, 10), 1.0);
    }

    #[test]
    fn test_progress_fraction_edge_cases() {
        // When total is 0, should use max(1) to avoid division by zero
        assert_eq!(Progress::fraction(0, 0), 0.0);

        // Full progress
        assert_eq!(Progress::fraction(100, 100), 1.0);

        // Partial progress
        assert_eq!(Progress::fraction(1, 3), 1.0 / 3.0);
        assert_eq!(Progress::fraction(2, 3), 2.0 / 3.0);
    }

    #[test]
    fn test_progress_fraction_large_numbers() {
        assert_eq!(Progress::fraction(500, 1000), 0.5);
        assert_eq!(Progress::fraction(999, 1000), 0.999);
        assert_eq!(Progress::fraction(1, 1000), 0.001);
    }

    #[test]
    #[should_panic(expected = "assertion failed")]
    fn test_progress_fraction_invalid_done_greater_than_total() {
        // Should panic when done > total
        Progress::fraction(11, 10);
    }

    #[test]
    fn test_progress_enum_variants() {
        let begin = Progress::Begin;
        let report = Progress::Report;
        let end = Progress::End;

        // Just verify they exist and can be created
        assert_eq!(begin, Progress::Begin);
        assert_eq!(report, Progress::Report);
        assert_eq!(end, Progress::End);
    }

    #[test]
    fn test_progress_enum_equality() {
        assert_eq!(Progress::Begin, Progress::Begin);
        assert_eq!(Progress::Report, Progress::Report);
        assert_eq!(Progress::End, Progress::End);

        assert_ne!(Progress::Begin, Progress::Report);
        assert_ne!(Progress::Report, Progress::End);
        assert_ne!(Progress::Begin, Progress::End);
    }
}
