use crate::beancount_data::BeancountData;
use crate::document::Document;
use crate::treesitter_utils::text_for_tree_sitter_node;
use anyhow::Result;
use chrono::Datelike;
use std::collections::HashMap;
use tracing::debug;

/// Provider function for LSP ``.
pub(crate) fn completion(
    forest: &HashMap<lsp_types::Url, tree_sitter::Tree>,
    beancount_data: &HashMap<lsp_types::Url, BeancountData>,
    open_docs: &HashMap<lsp_types::Url, Document>,
    trigger_character: Option<char>,
    cursor: lsp_types::TextDocumentPositionParams,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion");

    let uri = &cursor.text_document.uri;
    let line = &cursor.position.line;
    let char = &cursor.position.character;
    debug!("providers::completion - line {} char {}", line, char);

    let tree = forest.get(uri).unwrap();
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
    let node = tree
        .root_node()
        .named_descendant_for_point_range(start, end);

    let prev_sibling_node = match node {
        Some(node) => node.prev_sibling(),
        None => None,
    };
    debug!(
        "providers::completion - prev sibling node {:?}",
        prev_sibling_node
    );

    let prev_named_sibling_node = match node {
        Some(node) => node.prev_named_sibling(),
        None => None,
    };
    debug!(
        "providers::completion - prev named sibling node {:?}",
        prev_named_sibling_node
    );

    let parent_node = match node {
        Some(node) => node.parent(),
        None => None,
    };
    debug!("providers::completion - parent node {:?}", parent_node);

    if let Some(char) = trigger_character {
        debug!(
            "providers::completion - handle trigger_character {:?}",
            trigger_character
        );
        match char {
            '2' => complete_date(),
            '"' => {
                if prev_sibling_node.is_some() && prev_sibling_node.unwrap().kind() == "txn" {
                    complete_narration(beancount_data.clone())
                } else {
                    Ok(None)
                }
            }
            '#' => complete_tag(beancount_data.clone()),
            '^' => complete_link(beancount_data.clone()),
            _ => Ok(None),
        }
    } else {
        debug!("providers::completion - handle node {:?}", node);
        match node {
            Some(node) => {
                let text = text_for_tree_sitter_node(&content, &node);
                debug!("providers::completion - text {}", text);

                debug!("providers::completion - handle node");

                //if parent_parent_node.is_some()
                //    && parent_parent_node.unwrap().kind() == "posting_or_kv_list"
                //    && *char < 10
                //{
                //   complete_account(snapshot.beancount_data)
                //} else {
                match node.kind() {
                    /*"ERROR" => {
                        debug!("providers::completion - handle node - handle error");
                        debug!(
                            "providers::completion - handle node - handle error {}",
                            text
                        );
                        let prefix = text.chars().next().unwrap();
                        debug!("providers::completion - handle node - prefix {}", prefix);
                        if prefix == '"' {
                            complete_txn_string(snapshot.beancount_data)
                        } else {
                            Ok(None)
                        }
                    }*/
                    "identifier" => {
                        debug!("providers::completion - handle node - handle identifier");
                        if prev_sibling_node.is_some()
                            && prev_sibling_node.unwrap().kind() == "date"
                        {
                            complete_kind()
                        } else {
                            // if parent_parent_node.is_some() && parent_parent_node.unwrap().kind() ==
                            // "posting_or_kv_list" {
                            complete_account(beancount_data.clone())
                            //} else {
                            //    Ok(None)
                        }
                    }
                    "narration" => {
                        debug!("providers::completion - handle node - handle narration");
                        complete_narration(beancount_data.clone())
                    }
                    "payee" => {
                        debug!("providers::completion - handle node - handle payee");
                        complete_narration(beancount_data.clone())
                    }
                    _ => Ok(None),
                }
                //}
            }
            None => Ok(None),
        }
    }
}

fn complete_date() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
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
    let items = vec![
        lsp_types::CompletionItem {
            label: today,
            detail: Some("today".to_string()),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: cur_month,
            detail: Some("this month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: prev_month,
            detail: Some("prev month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: next_month,
            detail: Some("next month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

fn complete_kind() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::kind");
    let items = vec![
        lsp_types::CompletionItem {
            label: String::from("txn"),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("balance"),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("open"),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("close"),
            kind: Some(lsp_types::CompletionItemKind::TEXT),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

pub fn add_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 12 {
        year += 1;
        month = 1;
    } else {
        month += 1;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid date")
}

pub fn sub_one_month(date: chrono::NaiveDate) -> chrono::NaiveDate {
    let mut year = date.year();
    let mut month = date.month();
    if month == 1 {
        year -= 1;
        month = 12;
    } else {
        month -= 1;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid date")
}

fn complete_narration(
    data: HashMap<lsp_types::Url, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::narration");
    let mut completions = Vec::new();
    for data in data.values() {
        for txn_string in data.get_narration() {
            completions.push(lsp_types::CompletionItem {
                label: txn_string,
                detail: Some("Beancount Narration".to_string()),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

fn complete_account(
    data: HashMap<lsp_types::Url, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::account");
    let mut completions = Vec::new();
    for data in data.values() {
        for account in data.get_accounts() {
            completions.push(lsp_types::CompletionItem {
                label: account,
                detail: Some("Beancount Account".to_string()),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

fn complete_tag(
    data: HashMap<lsp_types::Url, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for tag in data.get_tags() {
            completions.push(lsp_types::CompletionItem {
                label: tag,
                detail: Some("Beancount Tag".to_string()),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

fn complete_link(
    data: HashMap<lsp_types::Url, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for link in data.get_links() {
            completions.push(lsp_types::CompletionItem {
                label: link,
                detail: Some("Beancount Link".to_string()),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

#[cfg(test)]
mod tests {
    use crate::beancount_data::BeancountData;
    use crate::document::Document;
    use crate::providers::completion::add_one_month;
    use crate::providers::completion::completion;
    use crate::providers::completion::sub_one_month;
    use anyhow::Result;
    use std::collections::HashMap;
    use test_log::test;

    #[derive(Debug)]
    pub struct Fixture {
        pub documents: Vec<TestDocument>,
    }
    impl Fixture {
        pub fn parse(input: &str) -> Self {
            let mut documents = Vec::new();
            let mut start = 0;
            if !input.is_empty() {
                for end in input
                    .match_indices("%!")
                    .skip(1)
                    .map(|(i, _)| i)
                    .chain(std::iter::once(input.len()))
                {
                    documents.push(TestDocument::parse(&input[start..end]));
                    start = end;
                }
            }
            Self { documents }
        }
    }

    #[derive(Debug)]
    pub struct TestDocument {
        pub path: String,
        pub text: String,
        pub cursor: Option<lsp_types::Position>,
        pub ranges: Vec<lsp_types::Range>,
    }
    impl TestDocument {
        pub fn parse(input: &str) -> Self {
            let mut lines = Vec::new();

            let (path, input) = input
                .trim()
                .strip_prefix("%! ")
                .map(|input| input.split_once('\n').unwrap_or((input, "")))
                .unwrap();

            let mut ranges = Vec::new();
            let mut cursor = None;

            for line in input.lines() {
                if line.chars().all(|c| matches!(c, ' ' | '^' | '|' | '!')) && !line.is_empty() {
                    let index = (lines.len() - 1) as u32;

                    cursor = cursor.or_else(|| {
                        let character = line.find('|')?;
                        Some(lsp_types::Position::new(index, character as u32))
                    });

                    if let Some(start) = line.find('!') {
                        let position = lsp_types::Position::new(index, start as u32);
                        ranges.push(lsp_types::Range::new(position, position));
                    }

                    if let Some(start) = line.find('^') {
                        let end = line.rfind('^').unwrap() + 1;
                        ranges.push(lsp_types::Range::new(
                            lsp_types::Position::new(index, start as u32),
                            lsp_types::Position::new(index, end as u32),
                        ));
                    }
                } else {
                    lines.push(line);
                }
            }

            Self {
                path: path.to_string(),
                text: lines.join("\n"),
                cursor,
                ranges,
            }
        }
    }

    pub struct TestState {
        fixture: Fixture,
        beancount_data: HashMap<lsp_types::Url, BeancountData>,
        forest: HashMap<lsp_types::Url, tree_sitter::Tree>,
        open_docs: HashMap<lsp_types::Url, Document>,
    }
    impl TestState {
        pub fn new(fixture: &str) -> Result<Self> {
            let fixture = Fixture::parse(fixture);
            let forest: HashMap<lsp_types::Url, tree_sitter::Tree> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = lsp_types::Url::parse(format!("file://{path}").as_str()).unwrap();
                    let mut parser = tree_sitter::Parser::new();
                    parser
                        .set_language(tree_sitter_beancount::language())
                        .unwrap();
                    let v = parser.parse(document.text.clone(), None).unwrap();
                    (k, v)
                })
                .collect();
            let beancount_data: HashMap<lsp_types::Url, BeancountData> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = lsp_types::Url::parse(format!("file://{path}").as_str()).unwrap();
                    let content = ropey::Rope::from(document.text.clone());
                    let v = BeancountData::new(forest.get(&k).unwrap(), &content);
                    (k, v)
                })
                .collect();
            let open_docs: HashMap<lsp_types::Url, Document> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = lsp_types::Url::parse(format!("file://{path}").as_str()).unwrap();
                    let v = Document {
                        content: ropey::Rope::from(document.text.clone()),
                    };
                    (k, v)
                })
                .collect();
            Ok(TestState {
                fixture,
                beancount_data,
                forest,
                open_docs,
            })
        }

        pub fn cursor(&self) -> Option<lsp_types::TextDocumentPositionParams> {
            let (document, cursor) = self
                .fixture
                .documents
                .iter()
                .find_map(|document| document.cursor.map(|cursor| (document, cursor)))?;

            let path = document.path.as_str();
            let uri = lsp_types::Url::parse(format!("file://{path}").as_str()).unwrap();
            let id = lsp_types::TextDocumentIdentifier::new(uri);
            Some(lsp_types::TextDocumentPositionParams::new(id, cursor))
        }
    }

    #[test]
    fn handle_sub_one_month() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 5, 1).expect("valid date");
        assert_eq!(sub_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_sub_one_month_in_jan() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
        assert_eq!(sub_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_add_one_month() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 7, 1).expect("valid date");
        assert_eq!(add_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_add_one_month_in_dec() {
        let input_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
        let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        assert_eq!(add_one_month(input_date), expected_date)
    }

    #[test]
    fn handle_date_completion() {
        let fixure = r#"
%! main.beancount
2
|
^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let text_document_position = test_state.cursor().unwrap();
        println!(
            "{} {}",
            text_document_position.position.line, text_document_position.position.character
        );
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            Some('2'),
            text_document_position,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        let today = chrono::offset::Local::now().naive_local().date();
        let prev_month = sub_one_month(today).format("%Y-%m-").to_string();
        let cur_month = today.format("%Y-%m-").to_string();
        let next_month = add_one_month(today).format("%Y-%m-").to_string();
        let today = today.format("%Y-%m-%d").to_string();
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: today,
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    detail: Some(String::from("today")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: cur_month,
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    detail: Some(String::from("this month")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: prev_month,
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    detail: Some(String::from("prev month")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: next_month,
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    detail: Some(String::from("next month")),
                    ..Default::default()
                }
            ]
        )
    }

    #[test]
    fn handle_txn_completion() {
        let fixure = r#"
%! main.beancount
2023-10-01 t
            |
            ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            None,
            cursor,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: String::from("txn"),
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("balance"),
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("open"),
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("close"),
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    ..Default::default()
                },
            ]
        )
    }

    #[test]
    fn handle_narration_completion() {
        let fixure = r#"
%! main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            Some('"'),
            cursor,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("\"Test Co\""),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                detail: Some(String::from("Beancount Narration")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_payee_completion() {
        let fixure = r#"
%! main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "Test" "
                       |
                       ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            Some('"'),
            cursor,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        assert_eq!(items, [])
    }

    #[test]
    fn handle_account_completion() {
        let fixure = r#"
%! main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    a
     |
     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            None,
            cursor,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: String::from("Assets:Test"),
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    detail: Some(String::from("Beancount Account")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("Expenses:Test"),
                    kind: Some(lsp_types::CompletionItemKind::TEXT),
                    detail: Some(String::from("Beancount Account")),
                    ..Default::default()
                }
            ]
        )
    }

    #[test]
    fn handle_tag_completion() {
        let fixure = r#"
%! main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar" #tag ^link
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn  "Test Co" "Foo Bar" #
                                     |
                                     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            Some('#'),
            cursor,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("#tag"),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                detail: Some(String::from("Beancount Tag")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_link_completion() {
        let fixure = r#"
%! main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar" #tag ^link
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn  "Test Co" "Foo Bar" #
                                     |
                                     ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = match completion(
            &test_state.forest,
            &test_state.beancount_data,
            &test_state.open_docs,
            Some('^'),
            cursor,
        )
        .unwrap()
        {
            Some(items) => items,
            None => Vec::new(),
        };
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("^link"),
                kind: Some(lsp_types::CompletionItemKind::TEXT),
                detail: Some(String::from("Beancount Link")),
                ..Default::default()
            },]
        )
    }
}
