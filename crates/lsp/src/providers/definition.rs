use std::collections::HashMap;

use crate::{
    server::LspServerStateSnapshot,
    treesitter_utils::{lsp_position_to_node, text_for_tree_sitter_node},
    utils::ToFilePath,
};
use anyhow::Result;
use lsp_types::{GotoDefinitionResponse, Location, Range};
use tracing::debug;

pub(crate) fn definition(
    snapshot: LspServerStateSnapshot,
    params: lsp_types::GotoDefinitionParams,
) -> Result<Option<lsp_types::GotoDefinitionResponse>> {
    debug!("providers::definition");

    let uri = params.text_document_position_params.text_document.uri;
    let path_buf = uri.to_file_path().unwrap();

    let doc = snapshot.open_docs.get(&path_buf).unwrap();
    let data = snapshot.beancount_data.get(&path_buf).unwrap();

    let tree = snapshot.forest.get(&path_buf).unwrap();
    let node = lsp_position_to_node(
        &doc.content,
        params.text_document_position_params.position,
        tree,
    )
    .unwrap();

    debug!("providers::definition - node {:?}", node);

    let map: &HashMap<String, Range> = match node.kind() {
        "account" => &data.accounts_definitions,
        "currency" => &data.commodities_definitions,
        _ => return Ok(None),
    };

    // FIXME: This returns only the first matching definition.

    let text = text_for_tree_sitter_node(&doc.content, &node);
    debug!("providers::definition - text {:?}", text);

    if let Some(range) = map.get(&text) {
        let location = Location {
            uri,
            range: Range {
                start: range.start,
                end: range.end,
            },
        };
        Ok(Some(GotoDefinitionResponse::Scalar(location)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::ToFilePath;
    use lsp_types::{
        GotoDefinitionParams, GotoDefinitionResponse, PartialResultParams, WorkDoneProgressParams,
    };

    use crate::handlers::text_document::definition;
    use crate::test_utils::TestState;

    #[test]
    fn handle_account_go_to_definition() {
        let _ = env_logger::builder().is_test(true).try_init();

        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
          |
          ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let text_document_position = test_state.cursor().unwrap();

        assert_eq!(text_document_position.position.line, 4);
        assert_eq!(text_document_position.position.character, 10);

        let params = GotoDefinitionParams {
            text_document_position_params: text_document_position,
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let definition = definition(test_state.snapshot, params).unwrap();
        assert!(definition.is_some());

        let GotoDefinitionResponse::Scalar(location) = definition.unwrap() else {
            panic!("wrong definition type")
        };
        assert_eq!(location.range.start.line, 1);
        assert_eq!(location.range.end.line, 1);
    }

    #[test]
    fn handle_commodity_go_to_definition() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 commodity USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
                   |
                   ^
    Expenses:Test
"#;
        let test_state = TestState::new(fixure).unwrap();
        let text_document_position = test_state.cursor().unwrap();
        println!(
            "{} {}",
            text_document_position.position.line, text_document_position.position.character
        );

        let params = GotoDefinitionParams {
            text_document_position_params: text_document_position,
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let uri = &params.text_document_position_params.text_document.uri;
        let path_buf = uri.to_file_path().unwrap();
        let data = test_state.snapshot.beancount_data.get(&path_buf).unwrap();

        data.commodities_definitions.get("USD").unwrap();

        let definition = definition(test_state.snapshot, params).unwrap();
        assert!(definition.is_some());

        let GotoDefinitionResponse::Scalar(location) = definition.unwrap() else {
            panic!("wrong definition type")
        };
        assert_eq!(location.range.start.line, 2);
        assert_eq!(location.range.end.line, 2);
    }
}
