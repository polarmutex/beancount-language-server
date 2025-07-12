use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use chrono::Datelike;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;
use tree_sitter_beancount::tree_sitter;

/// Provider function for LSP completion.
pub(crate) fn completion(
    snapshot: LspServerStateSnapshot,
    trigger_character: Option<char>,
    cursor: lsp_types::TextDocumentPositionParams,
) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion");

    let uri = match cursor.text_document.uri.to_file_path() {
        Ok(path) => path,
        Err(_) => {
            debug!("URI conversion failed for: {:?}", cursor.text_document.uri);
            return Ok(None);
        }
    };
    let line = &cursor.position.line;
    let char = &cursor.position.character;
    debug!("providers::completion - line {} char {}", line, char);

    let tree = snapshot.forest.get(&uri).unwrap();
    let doc = snapshot.open_docs.get(&uri).unwrap();
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

    // Extract the current prefix for filtering completions
    let current_line_text = content.line(*line as usize).to_string();
    let prefix = extract_completion_prefix(&current_line_text, *char as usize);
    debug!("providers::completion - prefix: '{}'", prefix);

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
                    complete_narration_with_quotes(
                        snapshot.beancount_data,
                        &current_line_text,
                        cursor.position.character as usize,
                    )
                } else {
                    Ok(None)
                }
            }
            '#' => complete_tag(snapshot.beancount_data),
            '^' => complete_link(snapshot.beancount_data),
            ':' => {
                // Handle colon in account names - continue account completion
                complete_account_with_prefix(snapshot.beancount_data, &prefix)
            }
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
                    "ERROR" => {
                        debug!("providers::completion - handle node - handle error");
                        debug!(
                            "providers::completion - handle node - handle error {}",
                            text
                        );
                        // For ERROR nodes, try account completion as it might be an incomplete account name
                        complete_account_with_prefix(snapshot.beancount_data, &prefix)
                    }
                    "identifier" => {
                        debug!("providers::completion - handle node - handle identifier");
                        if prev_sibling_node.is_some()
                            && prev_sibling_node.unwrap().kind() == "date"
                        {
                            complete_kind()
                        } else {
                            // if parent_parent_node.is_some() && parent_parent_node.unwrap().kind() ==
                            // "posting_or_kv_list" {
                            complete_account_with_prefix(snapshot.beancount_data, &prefix)
                            //} else {
                            //    Ok(None)
                        }
                    }
                    "payee" => {
                        debug!("providers::completion - handle node - handle payee");
                        complete_narration_with_quotes(
                            snapshot.beancount_data,
                            &current_line_text,
                            cursor.position.character as usize,
                        )
                    }
                    "narration" => {
                        debug!("providers::completion - handle node - handle narration");
                        complete_narration_with_quotes(
                            snapshot.beancount_data,
                            &current_line_text,
                            cursor.position.character as usize,
                        )
                    }
                    _ => Ok(None),
                }
                //}
            }
            None => Ok(None),
        }
    }
}

pub(crate) fn complete_date() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
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
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: cur_month,
            detail: Some("this month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: prev_month,
            detail: Some("prev month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: next_month,
            detail: Some("next month".to_string()),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
    ];
    Ok(Some(items))
}

pub(crate) fn complete_kind() -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::kind");
    let items = vec![
        lsp_types::CompletionItem {
            label: String::from("txn"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("balance"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("open"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            ..Default::default()
        },
        lsp_types::CompletionItem {
            label: String::from("close"),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
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

fn complete_narration_with_quotes(
    data: HashMap<PathBuf, BeancountData>,
    line_text: &str,
    cursor_char: usize,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::narration");

    // Check if there's already a closing quote after the cursor
    let has_closing_quote = line_text.chars().skip(cursor_char).any(|c| c == '"');
    debug!(
        "providers::completion::narration - has_closing_quote: {}",
        has_closing_quote
    );

    let mut completions = Vec::new();
    for data in data.values() {
        for txn_string in data.get_narration() {
            let insert_text = if has_closing_quote {
                // Remove the quotes from the stored string and don't add closing quote
                txn_string.trim_matches('"').to_string()
            } else {
                // Keep the full quoted string as stored
                txn_string.clone()
            };

            completions.push(lsp_types::CompletionItem {
                label: txn_string.clone(),
                detail: Some("Beancount Narration".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                insert_text: Some(insert_text),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

fn complete_account_with_prefix(
    data: HashMap<PathBuf, BeancountData>,
    prefix: &str,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::account with prefix: '{}'", prefix);
    let mut completions = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    for data in data.values() {
        for account in data.get_accounts() {
            // Case-insensitive prefix matching
            if prefix.is_empty() || account.to_lowercase().starts_with(&prefix_lower) {
                completions.push(lsp_types::CompletionItem {
                    label: account.clone(),
                    detail: Some("Beancount Account".to_string()),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    // Set filter_text to enable proper LSP client filtering
                    filter_text: Some(account.clone()),
                    ..Default::default()
                });
            }
        }
    }
    Ok(Some(completions))
}

/// Extract the current word/prefix being typed for completion
pub(crate) fn extract_completion_prefix(line_text: &str, cursor_char: usize) -> String {
    let chars: Vec<char> = line_text.chars().collect();
    if cursor_char == 0 || cursor_char > chars.len() {
        return String::new();
    }

    let mut start = cursor_char.saturating_sub(1);

    // Find the start of the current word (account name)
    // Account names can contain letters, numbers, colons, and hyphens
    while start > 0 {
        let c = chars[start.saturating_sub(1)];
        if !c.is_alphanumeric() && c != ':' && c != '-' && c != '_' {
            break;
        }
        start = start.saturating_sub(1);
    }

    // Extract the prefix from start to cursor
    let end = cursor_char.min(chars.len());
    chars[start..end].iter().collect()
}

pub(crate) fn complete_tag(
    data: HashMap<PathBuf, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for tag in data.get_tags() {
            completions.push(lsp_types::CompletionItem {
                label: tag,
                detail: Some("Beancount Tag".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

pub(crate) fn complete_link(
    data: HashMap<PathBuf, BeancountData>,
) -> anyhow::Result<Option<Vec<lsp_types::CompletionItem>>> {
    debug!("providers::completion::tag");
    let mut completions = Vec::new();
    for data in data.values() {
        for link in data.get_links() {
            completions.push(lsp_types::CompletionItem {
                label: link,
                detail: Some("Beancount Link".to_string()),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                ..Default::default()
            });
        }
    }
    Ok(Some(completions))
}

#[cfg(test)]
mod tests {
    use crate::providers::completion::add_one_month;
    use crate::providers::completion::completion;
    use crate::providers::completion::extract_completion_prefix;
    use crate::providers::completion::sub_one_month;
    use crate::server::LspServerStateSnapshot;
    use tree_sitter_beancount::tree_sitter;
    //use insta::assert_yaml_snapshot;
    use crate::beancount_data::BeancountData;
    use crate::config::Config;
    use crate::document::Document;
    use crate::utils::ToFilePath;
    use anyhow::Result;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;
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
        // pub ranges: Vec<lsp_types::Range>,
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
                // ranges,
            }
        }
    }

    pub struct TestState {
        fixture: Fixture,
        snapshot: LspServerStateSnapshot,
    }
    impl TestState {
        /// Converts a test fixture path to a PathBuf, handling cross-platform compatibility.
        /// Uses a simpler approach that should work on all platforms.
        fn path_from_fixture(path: &str) -> Result<PathBuf> {
            // For empty paths, return a default path that should work on all platforms
            if path.is_empty() {
                return Ok(std::path::PathBuf::from("/"));
            }

            // Try to create the URI and convert to path
            // First try the path as-is (works for absolute paths on Unix and relative paths)
            let uri_str = if path.starts_with('/') {
                // Unix-style absolute path
                if cfg!(windows) {
                    format!("file:///C:{path}")
                } else {
                    format!("file://{path}")
                }
            } else if cfg!(windows) && path.len() > 1 && path.chars().nth(1) == Some(':') {
                // Windows-style absolute path like "C:\path"
                format!("file:///{}", path.replace('\\', "/"))
            } else {
                // Relative path or other format - this will likely fail but let's try
                format!("file://{path}")
            };

            let uri = lsp_types::Uri::from_str(&uri_str)
                .map_err(|e| anyhow::anyhow!("Invalid URI: {}", e))?;

            // Check if this is a problematic URI format that would cause to_file_path() to panic
            // URIs like "file://bare-filename" (without path separators) are problematic because
            // they treat the filename as a hostname. Paths with "./" or "../" are typically OK.
            if uri_str.starts_with("file://") && !uri_str.starts_with("file:///") {
                let after_protocol = &uri_str[7..]; // Remove "file://"
                if !after_protocol.is_empty()
                    && !after_protocol.starts_with('/')
                    && !after_protocol.starts_with('.')
                {
                    return Err(anyhow::anyhow!(
                        "Invalid file URI format (contains hostname): {}",
                        uri_str
                    ));
                }
            }

            let file_path = uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("Failed to convert URI to file path: {}", uri_str))?;

            Ok(file_path)
        }

        pub fn new(fixture: &str) -> Result<Self> {
            let fixture = Fixture::parse(fixture);
            let forest: HashMap<PathBuf, tree_sitter::Tree> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = Self::path_from_fixture(path)?;
                    let mut parser = tree_sitter::Parser::new();
                    parser
                        .set_language(&tree_sitter_beancount::language())
                        .unwrap();
                    let v = parser.parse(document.text.clone(), None).unwrap();
                    Ok((k, v))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            let beancount_data: HashMap<PathBuf, BeancountData> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = Self::path_from_fixture(path)?;
                    let content = ropey::Rope::from(document.text.clone());
                    let v = BeancountData::new(forest.get(&k).unwrap(), &content);
                    Ok((k, v))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            let open_docs: HashMap<PathBuf, Document> = fixture
                .documents
                .iter()
                .map(|document| {
                    let path = document.path.as_str();
                    let k = Self::path_from_fixture(path)?;
                    let v = Document {
                        content: ropey::Rope::from(document.text.clone()),
                    };
                    Ok((k, v))
                })
                .collect::<Result<HashMap<_, _>>>()?;
            Ok(TestState {
                fixture,
                snapshot: LspServerStateSnapshot {
                    beancount_data,
                    config: Config::new(Self::path_from_fixture("/test.beancount")?),
                    forest,
                    open_docs,
                },
            })
        }

        pub fn cursor(&self) -> Option<lsp_types::TextDocumentPositionParams> {
            let (document, cursor) = self
                .fixture
                .documents
                .iter()
                .find_map(|document| document.cursor.map(|cursor| (document, cursor)))?;

            let path = document.path.as_str();
            // Use the same path conversion logic as in TestState::new() to ensure consistency
            let file_path = Self::path_from_fixture(path).ok()?;

            // Convert PathBuf back to URI string for cross-platform compatibility
            let path_str = file_path.to_string_lossy();
            let uri_str = if cfg!(windows) {
                // On Windows, paths start with drive letter, need file:/// prefix
                format!("file:///{}", path_str.replace('\\', "/"))
            } else {
                format!("file://{path_str}")
            };

            let uri = lsp_types::Uri::from_str(&uri_str).ok()?;
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
%! /main.beancount
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
        let items = completion(test_state.snapshot, Some('2'), text_document_position)
            .unwrap()
            .unwrap_or_default();
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
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("today")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: cur_month,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("this month")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: prev_month,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("prev month")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: next_month,
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("next month")),
                    ..Default::default()
                }
            ]
        )
    }

    #[test]
    fn handle_txn_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 t
            |
            ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: String::from("txn"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("balance"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("open"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("close"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    ..Default::default()
                },
            ]
        )
    }

    #[test]
    fn handle_narration_completion() {
        let fixure = r#"
%! /main.beancount
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
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("\"Test Co\""),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Narration")),
                insert_text: Some(String::from("\"Test Co\"")), // No closing quote exists, so keep full quoted string
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_payee_completion() {
        let fixure = r#"
%! /main.beancount
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
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(items, [])
    }

    #[test]
    fn handle_narration_completion_with_existing_closing_quote() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "Test Co"
2023-10-01 txn ""
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        // Should have completions with insert_text without quotes since closing quote exists
        assert!(!items.is_empty());
        let test_co_completion = items
            .iter()
            .find(|item| item.label == "\"Test Co\"")
            .unwrap();
        assert_eq!(
            test_co_completion.insert_text,
            Some(String::from("Test Co"))
        );

        let foo_bar_completion = items
            .iter()
            .find(|item| item.label == "\"Foo Bar\"")
            .unwrap();
        assert_eq!(
            foo_bar_completion.insert_text,
            Some(String::from("Foo Bar"))
        );
    }

    #[test]
    fn handle_narration_completion_without_closing_quote() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:Test 1 USD
    Expenses:Test
2023-10-01 txn "
                |
                ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some('"'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("\"Foo Bar\""),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Narration")),
                insert_text: Some(String::from("\"Foo Bar\"")), // Keep full quotes since no closing quote
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_account_completion() {
        let fixure = r#"
%! /main.beancount
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
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("Assets:Test"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Account")),
                filter_text: Some(String::from("Assets:Test")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_account_completion_with_colon() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Assets:Checking USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Assets:
           |
           ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, Some(':'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [
                lsp_types::CompletionItem {
                    label: String::from("Assets:Test"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("Beancount Account")),
                    filter_text: Some(String::from("Assets:Test")),
                    ..Default::default()
                },
                lsp_types::CompletionItem {
                    label: String::from("Assets:Checking"),
                    kind: Some(lsp_types::CompletionItemKind::ENUM),
                    detail: Some(String::from("Beancount Account")),
                    filter_text: Some(String::from("Assets:Checking")),
                    ..Default::default()
                },
            ]
        )
    }

    #[test]
    fn handle_case_insensitive_completion() {
        let fixure = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
2023-10-01 open Expenses:Test USD
2023-10-01 txn  "Test Co" "Foo Bar"
    Asse
        |
        ^
"#;
        let test_state = TestState::new(fixure).unwrap();
        let cursor = test_state.cursor().unwrap();
        println!("{} {}", cursor.position.line, cursor.position.character);
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("Assets:Test"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Account")),
                filter_text: Some(String::from("Assets:Test")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_tag_completion() {
        let fixure = r#"
%! /main.beancount
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
        let items = completion(test_state.snapshot, Some('#'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("#tag"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Tag")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn handle_link_completion() {
        let fixure = r#"
%! /main.beancount
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
        let items = completion(test_state.snapshot, Some('^'), cursor)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            items,
            [lsp_types::CompletionItem {
                label: String::from("^link"),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(String::from("Beancount Link")),
                ..Default::default()
            },]
        )
    }

    #[test]
    fn test_path_from_fixture_unix_style() {
        let result = TestState::path_from_fixture("/main.beancount");
        assert!(result.is_ok());
        let path = result.unwrap();

        if cfg!(windows) {
            // On Windows, should convert to C:\main.beancount
            assert_eq!(path.to_string_lossy(), "C:\\main.beancount");
        } else {
            // On Unix, should remain /main.beancount
            assert_eq!(path.to_string_lossy(), "/main.beancount");
        }
    }

    #[test]
    fn test_path_from_fixture_relative_path() {
        // Relative paths without leading slash create invalid file URIs
        // (they become hostnames), so they should fail
        let result = TestState::path_from_fixture("main.beancount");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_from_fixture_dot_relative_path() {
        // Test relative path starting with ./
        // On Windows, this succeeds and creates a UNC path like \\.\main.beancount
        // On Unix, this fails because the dot becomes a hostname in the file URI
        let result = TestState::path_from_fixture("./main.beancount");
        if cfg!(windows) {
            // On Windows, this succeeds and creates a UNC path
            assert!(result.is_ok());
            let path = result.unwrap();
            assert!(path.to_string_lossy().contains("main.beancount"));
        } else {
            // On Unix, this should fail
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_path_from_fixture_nested_unix_path() {
        let result = TestState::path_from_fixture("/some/nested/path.beancount");
        assert!(result.is_ok());
        let path = result.unwrap();

        if cfg!(windows) {
            // On Windows, should convert to C:\some\nested\path.beancount
            assert_eq!(path.to_string_lossy(), "C:\\some\\nested\\path.beancount");
        } else {
            // On Unix, should remain /some/nested/path.beancount
            assert_eq!(path.to_string_lossy(), "/some/nested/path.beancount");
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_path_from_fixture_windows_style() {
        // Test that Windows-style paths work correctly
        let result = TestState::path_from_fixture("C:\\main.beancount");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert_eq!(path.to_string_lossy(), "C:\\main.beancount");
    }

    #[test]
    fn test_path_from_fixture_invalid_uri() {
        // Test with a path that would create an invalid URI
        let result = TestState::path_from_fixture("invalid uri with spaces and special chars: <>");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_from_fixture_empty_path() {
        let result = TestState::path_from_fixture("");
        // Empty paths create file:// which should be handled gracefully
        assert!(result.is_ok());
        let path = result.unwrap();
        // Path should exist and be some kind of root/base path
        assert!(!path.to_string_lossy().is_empty());
        // Don't make specific assertions about the exact path format as it's platform-dependent
    }

    #[test]
    fn test_complete_kind_function() {
        // Test the complete_kind function directly
        use crate::providers::completion::complete_kind;

        let items = complete_kind().unwrap().unwrap();
        assert_eq!(items.len(), 4);

        let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
        assert!(labels.contains(&"txn".to_string()));
        assert!(labels.contains(&"balance".to_string()));
        assert!(labels.contains(&"open".to_string()));
        assert!(labels.contains(&"close".to_string()));
    }

    #[test]
    fn test_extract_completion_prefix_functionality() {
        // Test that the extract_completion_prefix function works correctly
        // This tests the actual implementation without relying on complex fixtures
        assert_eq!(extract_completion_prefix("Assets:Test", 11), "Assets:Test");
        assert_eq!(extract_completion_prefix("Assets:Test", 6), "Assets");
        assert_eq!(extract_completion_prefix("Assets:Test", 7), "Assets:");
        assert_eq!(extract_completion_prefix("Assets:Test", 0), "");
        assert_eq!(
            extract_completion_prefix("    Assets:Test", 15),
            "Assets:Test"
        );
        assert_eq!(
            extract_completion_prefix("Assets:Test-USD", 15),
            "Assets:Test-USD"
        );
    }

    #[test]
    fn test_completion_functions_directly() {
        // Test the completion functions directly rather than through complex fixtures
        use crate::providers::completion::{complete_date, complete_link, complete_tag};
        use std::collections::HashMap;

        let data = HashMap::new();

        // Test tag completion - with empty data should return empty list
        let tag_items = complete_tag(data.clone()).unwrap().unwrap();
        assert_eq!(tag_items.len(), 0); // No tags in empty data

        // Test link completion - with empty data should return empty list
        let link_items = complete_link(data).unwrap().unwrap();
        assert_eq!(link_items.len(), 0); // No links in empty data

        // Test date completion - this doesn't depend on data
        let date_items = complete_date().unwrap().unwrap();
        assert_eq!(date_items.len(), 4);
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("today".to_string()))
        );
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("this month".to_string()))
        );
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("prev month".to_string()))
        );
        assert!(
            date_items
                .iter()
                .any(|item| item.detail == Some("next month".to_string()))
        );
    }

    #[test]
    fn test_unsupported_trigger_character() {
        // Test that unsupported trigger characters return None
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Test USD
"#;
        let test_state = TestState::new(fixture).unwrap();

        // Use the proper path conversion to ensure consistency with TestState
        let file_path = TestState::path_from_fixture("/main.beancount").unwrap();
        let path_str = file_path.to_string_lossy();
        let uri_str = if cfg!(windows) {
            format!("file:///{}", path_str.replace('\\', "/"))
        } else {
            format!("file://{path_str}")
        };
        let uri = lsp_types::Uri::from_str(&uri_str).unwrap();

        let cursor = lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            position: lsp_types::Position {
                line: 0,
                character: 26,
            },
        };
        let items = completion(test_state.snapshot, Some('x'), cursor).unwrap();

        // Should return None for unsupported trigger characters
        assert!(items.is_none());
    }
}
