use lsp_types::{
    CompletionOptions, OneOf, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions,
};

pub(crate) fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                will_save: Some(true),
                will_save_wait_until: Some(true),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::SaveOptions(
                    lsp_types::SaveOptions {
                        include_text: Some(false),
                    },
                )),
            },
        )),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![
                "2".into(),
                "\"".into(),
                //":".into(),
                //"#".into(),
                //"\\".into(),
            ]),
            ..Default::default()
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}
