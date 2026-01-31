use crate::providers::semantic_tokens;
use lsp_types::FoldingRangeProviderCapability;
use lsp_types::InlayHintOptions;
use lsp_types::InlayHintServerCapabilities;
use lsp_types::RenameOptions;
use lsp_types::SemanticTokensFullOptions;
use lsp_types::SemanticTokensOptions;
use lsp_types::SemanticTokensServerCapabilities;
use lsp_types::WorkDoneProgressOptions;
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
                will_save: None,
                will_save_wait_until: None,
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
                "#".into(),
                "^".into(),
                ":".into(),
            ]),
            ..Default::default()
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(false),
            work_done_progress_options: WorkDoneProgressOptions {
                work_done_progress: None,
            },
        })),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: semantic_tokens::legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: None,
                ..Default::default()
            },
        )),
        inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
            InlayHintOptions {
                resolve_provider: Some(false),
                work_done_progress_options: WorkDoneProgressOptions {
                    work_done_progress: None,
                },
            },
        ))),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_document_sync_capabilities() {
        let caps = server_capabilities();

        // Verify text_document_sync is configured
        let sync = caps
            .text_document_sync
            .expect("text_document_sync should be set");

        match sync {
            TextDocumentSyncCapability::Options(options) => {
                assert_eq!(
                    options.open_close,
                    Some(true),
                    "open_close should be enabled"
                );
                assert_eq!(
                    options.change,
                    Some(TextDocumentSyncKind::INCREMENTAL),
                    "incremental sync should be enabled"
                );
                assert!(options.save.is_some(), "save should be configured");
            }
            _ => panic!("Expected TextDocumentSyncOptions"),
        }
    }

    #[test]
    fn test_will_save_capabilities() {
        // Neither will_save nor will_save_wait_until are implemented
        // Formatting is controlled by the client via documentFormattingProvider
        let caps = server_capabilities();

        let sync = caps
            .text_document_sync
            .expect("text_document_sync should be set");

        match sync {
            TextDocumentSyncCapability::Options(options) => {
                assert_eq!(
                    options.will_save, None,
                    "will_save should be disabled (not implemented)"
                );
                assert_eq!(
                    options.will_save_wait_until, None,
                    "will_save_wait_until should be disabled - formatting controlled by client"
                );
            }
            _ => panic!("Expected TextDocumentSyncOptions"),
        }
    }

    #[test]
    fn test_completion_capabilities() {
        let caps = server_capabilities();

        let completion = caps
            .completion_provider
            .expect("completion_provider should be set");

        // Verify trigger characters
        let triggers = completion
            .trigger_characters
            .expect("trigger_characters should be set");

        assert!(
            triggers.contains(&"2".to_string()),
            "Should trigger on '2' (dates)"
        );
        assert!(
            triggers.contains(&"\"".to_string()),
            "Should trigger on '\"' (payees/narration)"
        );
        assert!(
            triggers.contains(&"#".to_string()),
            "Should trigger on '#' (tags)"
        );
        assert!(
            triggers.contains(&"^".to_string()),
            "Should trigger on '^' (links)"
        );
        assert!(
            triggers.contains(&":".to_string()),
            "Should trigger on ':' (accounts)"
        );
        assert_eq!(
            triggers.len(),
            5,
            "Should have exactly 5 trigger characters"
        );
    }

    #[test]
    fn test_formatting_capability() {
        let caps = server_capabilities();

        assert!(
            caps.document_formatting_provider.is_some(),
            "document_formatting_provider should be enabled by default"
        );

        match caps.document_formatting_provider {
            Some(OneOf::Left(enabled)) => {
                assert!(enabled, "formatting should be enabled by default");
            }
            _ => panic!("Expected simple boolean for formatting capability"),
        }
    }

    #[test]
    fn test_definition_capability() {
        let caps = server_capabilities();

        assert!(
            caps.definition_provider.is_some(),
            "definition_provider should be enabled"
        );

        match caps.definition_provider {
            Some(OneOf::Left(enabled)) => {
                assert!(enabled, "definition should be enabled");
            }
            _ => panic!("Expected simple boolean for definition capability"),
        }
    }

    #[test]
    fn test_references_capability() {
        let caps = server_capabilities();

        assert!(
            caps.references_provider.is_some(),
            "references_provider should be enabled"
        );

        match caps.references_provider {
            Some(OneOf::Left(enabled)) => {
                assert!(enabled, "references should be enabled");
            }
            _ => panic!("Expected simple boolean for references capability"),
        }
    }

    #[test]
    fn test_rename_capability() {
        let caps = server_capabilities();

        let rename = caps.rename_provider.expect("rename_provider should be set");

        match rename {
            OneOf::Right(options) => {
                assert_eq!(
                    options.prepare_provider,
                    Some(false),
                    "prepare_provider should be disabled"
                );
            }
            _ => panic!("Expected RenameOptions"),
        }
    }

    #[test]
    fn test_semantic_tokens_capability() {
        let caps = server_capabilities();

        let semantic = caps
            .semantic_tokens_provider
            .expect("semantic_tokens_provider should be set");

        match semantic {
            SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
                // Verify full document semantic tokens is enabled
                match options.full {
                    Some(SemanticTokensFullOptions::Bool(enabled)) => {
                        assert!(enabled, "full semantic tokens should be enabled");
                    }
                    _ => panic!("Expected boolean for full semantic tokens"),
                }

                // Verify range is not supported
                assert_eq!(
                    options.range, None,
                    "range semantic tokens should be disabled"
                );

                // Verify legend is properly configured
                let legend = options.legend;
                assert!(
                    !legend.token_types.is_empty(),
                    "token_types should not be empty"
                );
            }
            _ => panic!("Expected SemanticTokensOptions"),
        }
    }

    #[test]
    fn test_capabilities_match_implemented_features() {
        // This test documents which capabilities are advertised
        // and serves as a regression test to ensure we don't advertise
        // capabilities without implementing handlers
        let caps = server_capabilities();

        // Implemented capabilities (have handlers in server.rs)
        assert!(
            caps.text_document_sync.is_some(),
            "text_document_sync is implemented"
        );
        assert!(
            caps.completion_provider.is_some(),
            "completion is implemented"
        );
        assert!(
            caps.document_formatting_provider.is_some(),
            "formatting is implemented"
        );
        assert!(caps.hover_provider.is_some(), "hover is implemented");
        assert!(
            caps.references_provider.is_some(),
            "references is implemented"
        );
        assert!(caps.rename_provider.is_some(), "rename is implemented");
        assert!(
            caps.semantic_tokens_provider.is_some(),
            "semantic_tokens is implemented"
        );
        assert!(
            caps.inlay_hint_provider.is_some(),
            "inlay_hint is implemented"
        );
        assert!(
            caps.folding_range_provider.is_some(),
            "folding_range is implemented"
        );

        // Verify NOT implemented capabilities are disabled
        assert!(
            caps.definition_provider.is_some(),
            "definition is implemented"
        );
        assert_eq!(
            caps.type_definition_provider, None,
            "type_definition is not implemented"
        );
        assert_eq!(
            caps.implementation_provider, None,
            "implementation is not implemented"
        );
        assert!(
            caps.document_symbol_provider.is_some(),
            "document_symbol is implemented"
        );
        assert!(
            caps.workspace_symbol_provider.is_some(),
            "workspace_symbol is implemented"
        );
        assert_eq!(
            caps.code_action_provider, None,
            "code_action is not implemented"
        );
        assert_eq!(
            caps.code_lens_provider, None,
            "code_lens is not implemented"
        );
        assert_eq!(
            caps.document_link_provider, None,
            "document_link is not implemented"
        );
        assert!(
            caps.folding_range_provider.is_some(),
            "folding_range is implemented"
        );
    }

    #[test]
    fn test_semantic_tokens_legend() {
        // Verify the semantic tokens legend is properly structured
        let legend = semantic_tokens::legend();

        // Basic sanity checks
        assert!(
            !legend.token_types.is_empty(),
            "Legend should have token types"
        );

        // Verify token types are unique
        let mut seen = std::collections::HashSet::new();
        for token_type in &legend.token_types {
            assert!(
                seen.insert(token_type.as_str()),
                "Duplicate token type: {}",
                token_type.as_str()
            );
        }

        // Verify token modifiers are unique (currently empty, but check anyway)
        let mut seen = std::collections::HashSet::new();
        for modifier in &legend.token_modifiers {
            assert!(
                seen.insert(modifier.as_str()),
                "Duplicate token modifier: {}",
                modifier.as_str()
            );
        }
    }

    #[test]
    fn test_advertised_capabilities_have_handlers() {
        // This test verifies that for each capability we advertise,
        // there exists a corresponding handler function with the correct signature.
        // This is a compile-time check that prevents advertising capabilities
        // without implementing them (like the willSaveWaitUntil bug in issue #741).

        use crate::handlers;
        use crate::server::LspServerStateSnapshot;

        // Get the advertised capabilities
        let caps = server_capabilities();

        // Completion capability -> handlers::text_document::completion
        if caps.completion_provider.is_some() {
            // Verify the handler function exists and has the correct signature
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::CompletionParams,
            ) -> anyhow::Result<Option<lsp_types::CompletionResponse>> =
                handlers::text_document::completion;
        }

        // Formatting capability -> handlers::text_document::formatting
        if caps.document_formatting_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::DocumentFormattingParams,
            ) -> anyhow::Result<Option<Vec<lsp_types::TextEdit>>> =
                handlers::text_document::formatting;
        }

        // References capability -> handlers::text_document::handle_references
        if caps.references_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::ReferenceParams,
            ) -> anyhow::Result<Option<Vec<lsp_types::Location>>> =
                handlers::text_document::handle_references;
        }

        // Definition capability -> handlers::text_document::handle_definition
        if caps.definition_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::GotoDefinitionParams,
            )
                -> anyhow::Result<Option<lsp_types::GotoDefinitionResponse>> =
                handlers::text_document::handle_definition;
        }
        // Hover capability -> handlers::text_document::hover
        if caps.hover_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::HoverParams,
            ) -> anyhow::Result<Option<lsp_types::Hover>> = handlers::text_document::hover;
        }

        // Rename capability -> handlers::text_document::handle_rename
        if caps.rename_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::RenameParams,
            ) -> anyhow::Result<Option<lsp_types::WorkspaceEdit>> =
                handlers::text_document::handle_rename;
        }

        // Semantic tokens capability -> handlers::text_document::semantic_tokens_full
        if caps.semantic_tokens_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::SemanticTokensParams,
            )
                -> anyhow::Result<Option<lsp_types::SemanticTokensResult>> =
                handlers::text_document::semantic_tokens_full;
        }

        // Inlay hint capability -> handlers::text_document::inlay_hint
        if caps.inlay_hint_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::InlayHintParams,
            ) -> anyhow::Result<Option<Vec<lsp_types::InlayHint>>> =
                handlers::text_document::inlay_hint;
        }

        // Folding range capability -> handlers::text_document::folding_range
        if caps.folding_range_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::FoldingRangeParams,
            ) -> anyhow::Result<Option<Vec<lsp_types::FoldingRange>>> =
                handlers::text_document::folding_range;
        }

        // Document symbol capability -> handlers::text_document::document_symbol
        if caps.document_symbol_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::DocumentSymbolParams,
            )
                -> anyhow::Result<Option<lsp_types::DocumentSymbolResponse>> =
                handlers::text_document::document_symbol;
        }

        // Workspace symbol capability -> handlers::text_document::workspace_symbol
        if caps.workspace_symbol_provider.is_some() {
            let _handler: fn(
                LspServerStateSnapshot,
                lsp_types::WorkspaceSymbolParams,
            )
                -> anyhow::Result<Option<lsp_types::WorkspaceSymbolResponse>> =
                handlers::text_document::workspace_symbol;
        }

        // Text document sync notifications (these don't return responses)
        if let Some(TextDocumentSyncCapability::Options(sync_options)) = &caps.text_document_sync {
            // did_open handler
            if sync_options.open_close == Some(true) {
                let _handler: fn(
                    &mut crate::server::LspServerState,
                    lsp_types::DidOpenTextDocumentParams,
                ) -> anyhow::Result<()> = handlers::text_document::did_open;
            }

            // did_close handler
            if sync_options.open_close == Some(true) {
                let _handler: fn(
                    &mut crate::server::LspServerState,
                    lsp_types::DidCloseTextDocumentParams,
                ) -> anyhow::Result<()> = handlers::text_document::did_close;
            }

            // did_change handler
            if sync_options.change.is_some() {
                let _handler: fn(
                    &mut crate::server::LspServerState,
                    lsp_types::DidChangeTextDocumentParams,
                ) -> anyhow::Result<()> = handlers::text_document::did_change;
            }

            // did_save handler
            if sync_options.save.is_some() {
                let _handler: fn(
                    &mut crate::server::LspServerState,
                    lsp_types::DidSaveTextDocumentParams,
                ) -> anyhow::Result<()> = handlers::text_document::did_save;
            }

            // will_save and will_save_wait_until are not implemented
            assert_eq!(
                sync_options.will_save, None,
                "will_save should not be advertised without a handler implementation"
            );
            assert_eq!(
                sync_options.will_save_wait_until, None,
                "will_save_wait_until should not be used for formatting (client controls formatting)"
            );
        }

        // Workspace notifications (dynamically registered, not in static capabilities)
        // didChangeWatchedFiles handler - registered dynamically for *.beancount files
        {
            let _handler: fn(
                &mut crate::server::LspServerState,
                lsp_types::DidChangeWatchedFilesParams,
            ) -> anyhow::Result<()> = handlers::workspace::did_change_watched_files;
        }

        // This test will fail to compile if:
        // 1. A capability is advertised but the handler function doesn't exist
        // 2. A handler function exists but has the wrong signature
        // 3. A handler is imported from the wrong module
    }
}
