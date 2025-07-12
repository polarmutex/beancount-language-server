use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::treesitter_utils::text_for_tree_sitter_node;
use crate::utils::ToFilePath;
use anyhow::Result;
use chrono::Datelike;
use nucleo_matcher::{Matcher, Config, pattern::{Pattern, CaseMatching, Normalization}};
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

    // Determine search mode based on capitalization
    let search_mode = determine_search_mode(prefix);
    debug!("Search mode: {:?} for prefix: '{}'", search_mode, prefix);

    for data in data.values() {
        let accounts: Vec<String> = data.get_accounts().into_iter().collect();

        match search_mode {
            SearchMode::Prefix => {
                // Capital letter typed - show all accounts that start with the prefix
                for account in &accounts {
                    if prefix.is_empty() || account.starts_with(prefix) {
                        completions.push(create_completion_item(account.clone(), 1.0));
                    }
                }
            }
            SearchMode::Fuzzy => {
                // Lowercase letter typed - fuzzy search all accounts
                let fuzzy_matches = fuzzy_search_accounts(&accounts, prefix);
                for (account, score) in fuzzy_matches {
                    completions.push(create_completion_item(account, score));
                }
            }
            SearchMode::Exact => {
                // No prefix or mixed case - use exact prefix matching
                let prefix_lower = prefix.to_lowercase();
                for account in accounts {
                    if prefix.is_empty() || account.to_lowercase().starts_with(&prefix_lower) {
                        completions.push(create_completion_item(account, 1.0));
                    }
                }
            }
        }
    }

    // Sort by score (higher is better) and then alphabetically
    completions.sort_by(|a, b| {
        let score_a = a
            .sort_text
            .as_ref()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.0);
        let score_b = b
            .sort_text
            .as_ref()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.0);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });

    Ok(Some(completions))
}

#[derive(Debug, PartialEq)]
enum SearchMode {
    Prefix,   // Capital letter - show all accounts with exact prefix match
    Fuzzy,    // Lowercase letter - fuzzy search all accounts
    Exact,    // Empty or mixed case - exact prefix matching
}

fn determine_search_mode(prefix: &str) -> SearchMode {
    if prefix.is_empty() {
        SearchMode::Exact
    } else if prefix
        .chars()
        .all(|c| c.is_uppercase() || !c.is_alphabetic())
    {
        SearchMode::Prefix
    } else if prefix
        .chars()
        .all(|c| c.is_lowercase() || !c.is_alphabetic())
    {
        SearchMode::Fuzzy
    } else {
        SearchMode::Exact
    }
}

fn fuzzy_search_accounts(accounts: &[String], query: &str) -> Vec<(String, f32)> {
    if query.is_empty() {
        return accounts.iter().map(|acc| (acc.clone(), 1.0)).collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    
    // Use the high-level match_list API for better performance
    let matches = pattern.match_list(accounts.iter().map(|s| s.as_str()), &mut matcher);
    
    // Convert to the expected format with f32 scores
    let mut result: Vec<(String, f32)> = matches
        .into_iter()
        .map(|(account, score)| (account.to_string(), score as f32))
        .collect();

    // Sort by score descending, then alphabetically  
    result.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    // Return top 20 matches to avoid overwhelming the user
    result.truncate(20);
    result
}

fn create_completion_item(account: String, score: f32) -> lsp_types::CompletionItem {
    lsp_types::CompletionItem {
        label: account.clone(),
        detail: Some("Beancount Account".to_string()),
        kind: Some(lsp_types::CompletionItemKind::ENUM),
        filter_text: Some(account.clone()),
        // Use score for sorting (higher scores first, so invert for lexicographic sort)
        sort_text: Some(format!("{:010.0}", 99999.0 - score.min(99999.0))),
        // Let the LSP client handle text replacement based on filter_text
        ..Default::default()
    }
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
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Assets:Test");
        assert_eq!(items[0].kind, Some(lsp_types::CompletionItemKind::ENUM));
        assert_eq!(items[0].detail, Some("Beancount Account".to_string()));
        assert_eq!(items[0].filter_text, Some("Assets:Test".to_string()));
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
        assert_eq!(items.len(), 2);

        // Should have both Assets accounts
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(labels.contains(&&"Assets:Test".to_string()));
        assert!(labels.contains(&&"Assets:Checking".to_string()));

        // Check properties of all items
        for item in &items {
            assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::ENUM));
            assert_eq!(item.detail, Some("Beancount Account".to_string()));
            assert!(item.label.starts_with("Assets:"));
        }
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
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Assets:Test");
        assert_eq!(items[0].kind, Some(lsp_types::CompletionItemKind::ENUM));
        assert_eq!(items[0].detail, Some("Beancount Account".to_string()));
        assert_eq!(items[0].filter_text, Some("Assets:Test".to_string()));
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
        assert!(date_items
            .iter()
            .any(|item| item.detail == Some("today".to_string())));
        assert!(date_items
            .iter()
            .any(|item| item.detail == Some("this month".to_string())));
        assert!(date_items
            .iter()
            .any(|item| item.detail == Some("prev month".to_string())));
        assert!(date_items
            .iter()
            .any(|item| item.detail == Some("next month".to_string())));
    }

    #[test]
    fn test_search_mode_determination() {
        use crate::providers::completion::{determine_search_mode, SearchMode};

        // Capital letters should trigger prefix search
        assert_eq!(determine_search_mode("A"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("AS"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("ASSETS"), SearchMode::Prefix);

        // Lowercase letters should trigger fuzzy search
        assert_eq!(determine_search_mode("a"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("as"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("assets"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("checking"), SearchMode::Fuzzy);

        // Mixed case should use exact matching
        assert_eq!(determine_search_mode("As"), SearchMode::Exact);
        assert_eq!(determine_search_mode("Assets"), SearchMode::Exact);
        assert_eq!(determine_search_mode("AssetS"), SearchMode::Exact);

        // Empty prefix should use exact matching
        assert_eq!(determine_search_mode(""), SearchMode::Exact);

        // Non-alphabetic characters should not affect mode determination
        assert_eq!(determine_search_mode("A:"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("a-"), SearchMode::Fuzzy);
    }

    #[test]
    fn test_fuzzy_search_accounts() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Cash:Savings".to_string(),
            "Assets:Investments:Stocks".to_string(),
            "Liabilities:CreditCard:Visa".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Expenses:Food:Restaurants".to_string(),
            "Income:Salary".to_string(),
        ];

        // Test exact match
        let matches = fuzzy_search_accounts(&accounts, "cash");
        assert!(!matches.is_empty());
        let cash_matches: Vec<&String> = matches
            .iter()
            .filter(|(acc, _)| acc.contains("Cash"))
            .map(|(acc, _)| acc)
            .collect();
        assert_eq!(cash_matches.len(), 2);

        // Test substring match
        let matches = fuzzy_search_accounts(&accounts, "food");
        assert!(!matches.is_empty());
        let food_matches: Vec<&String> = matches
            .iter()
            .filter(|(acc, _)| acc.contains("Food"))
            .map(|(acc, _)| acc)
            .collect();
        assert_eq!(food_matches.len(), 2);

        // Test fuzzy match (characters in order)
        let matches = fuzzy_search_accounts(&accounts, "chk");
        assert!(!matches.is_empty());
        let checking_match = matches.iter().find(|(acc, _)| acc.contains("Checking"));
        assert!(checking_match.is_some());

        // Test no matches
        let matches = fuzzy_search_accounts(&accounts, "xyz");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_nucleo_fuzzy_matching() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Cash:Savings".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Liabilities:CreditCard".to_string(),
        ];

        // Exact match should work
        let matches = fuzzy_search_accounts(&accounts, "cash");
        assert!(!matches.is_empty());
        
        // Should find accounts containing "cash"
        let cash_matches: Vec<&(String, f32)> = matches
            .iter()
            .filter(|(acc, _)| acc.to_lowercase().contains("cash"))
            .collect();
        assert!(!cash_matches.is_empty());

        // Should match against full account name - test "assets" should match "Assets:Cash:Checking"
        let assets_matches = fuzzy_search_accounts(&accounts, "assets");
        let assets_found = assets_matches
            .iter()
            .any(|(acc, _)| acc.starts_with("Assets"));
        assert!(assets_found, "Should find accounts starting with Assets");

        // Should match "assetchk" against "Assets:Cash:Checking" (fuzzy across full name)
        let fuzzy_full_matches = fuzzy_search_accounts(&accounts, "assetchk");
        let assetchk_found = fuzzy_full_matches
            .iter()
            .any(|(acc, _)| acc == "Assets:Cash:Checking");
        assert!(assetchk_found, "Should fuzzy match across full account name");

        // Fuzzy matching should work
        let fuzzy_matches = fuzzy_search_accounts(&accounts, "chk");
        assert!(!fuzzy_matches.is_empty());

        // No match should return empty
        let no_matches = fuzzy_search_accounts(&accounts, "xyz123");
        assert!(no_matches.is_empty());
    }

    #[test]
    fn test_fuzzy_matching_full_account_names() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Investments:Stocks".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Expenses:Transportation:Gas".to_string(),
            "Liabilities:CreditCard:Visa".to_string(),
        ];

        // Test matching across account segments
        let matches = fuzzy_search_accounts(&accounts, "assetsinv");
        let found = matches.iter().any(|(acc, _)| acc == "Assets:Investments:Stocks");
        assert!(found, "Should match 'assetsinv' to 'Assets:Investments:Stocks'");

        // Test matching with partial segments
        let matches = fuzzy_search_accounts(&accounts, "exptrans");
        let found = matches.iter().any(|(acc, _)| acc == "Expenses:Transportation:Gas");
        assert!(found, "Should match 'exptrans' to 'Expenses:Transportation:Gas'");

        // Test case insensitive matching across full name
        let matches = fuzzy_search_accounts(&accounts, "LIABCRED");
        let found = matches.iter().any(|(acc, _)| acc == "Liabilities:CreditCard:Visa");
        assert!(found, "Should match 'LIABCRED' to 'Liabilities:CreditCard:Visa'");

        // Test matching with mixed separators
        let matches = fuzzy_search_accounts(&accounts, "foodgroc");
        let found = matches.iter().any(|(acc, _)| acc == "Expenses:Food:Groceries");
        assert!(found, "Should match 'foodgroc' to 'Expenses:Food:Groceries'");
    }

    #[test]
    fn test_deep_account_fuzzy_matching() {
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Expenses:Fixed:Food:Groceries".to_string(),
            "Expenses:Variable:Food:Restaurants".to_string(),
            "Assets:Cash:Checking".to_string(),
            "Income:Salary:Base".to_string(),
        ];

        // Test that 'food' matches both food-related accounts
        let matches = fuzzy_search_accounts(&accounts, "food");
        println!("Matches for 'food': {:?}", matches);
        
        let food_groceries_found = matches.iter().any(|(acc, _)| acc == "Expenses:Fixed:Food:Groceries");
        let food_restaurants_found = matches.iter().any(|(acc, _)| acc == "Expenses:Variable:Food:Restaurants");
        
        assert!(food_groceries_found, "Should match 'food' to 'Expenses:Fixed:Food:Groceries'");
        assert!(food_restaurants_found, "Should match 'food' to 'Expenses:Variable:Food:Restaurants'");

        // Test that 'groceries' matches the groceries account
        let matches = fuzzy_search_accounts(&accounts, "groceries");
        println!("Matches for 'groceries': {:?}", matches);
        let groceries_found = matches.iter().any(|(acc, _)| acc == "Expenses:Fixed:Food:Groceries");
        assert!(groceries_found, "Should match 'groceries' to 'Expenses:Fixed:Food:Groceries'");

        // Test fuzzy matching across multiple segments
        let matches = fuzzy_search_accounts(&accounts, "expfoodgroc");
        println!("Matches for 'expfoodgroc': {:?}", matches);
        let fuzzy_found = matches.iter().any(|(acc, _)| acc == "Expenses:Fixed:Food:Groceries");
        assert!(fuzzy_found, "Should fuzzy match 'expfoodgroc' to 'Expenses:Fixed:Food:Groceries'");

        // Test search mode determination
        use crate::providers::completion::{determine_search_mode, SearchMode};
        assert_eq!(determine_search_mode("food"), SearchMode::Fuzzy);
        assert_eq!(determine_search_mode("FOOD"), SearchMode::Prefix);
        assert_eq!(determine_search_mode("Food"), SearchMode::Exact);
    }

    #[test]
    fn test_capital_letter_completion() {
        let fixture = r#"
%! /main.beancount
2023-10-01 open Assets:Cash:Checking USD
2023-10-01 open Assets:Investments:Stocks USD  
2023-10-01 open Liabilities:CreditCard:Visa USD
2023-10-01 open Expenses:Food:Groceries USD
2023-10-01 open Income:Salary USD
2023-10-01 txn "Test"
    A
     |
     ^
"#;
        let test_state = TestState::new(fixture).unwrap();
        let cursor = test_state.cursor().unwrap();
        let items = completion(test_state.snapshot, None, cursor)
            .unwrap()
            .unwrap_or_default();

        // Should show all accounts starting with "A"
        assert_eq!(items.len(), 2);
        let labels: Vec<&String> = items.iter().map(|item| &item.label).collect();
        assert!(labels.contains(&&"Assets:Cash:Checking".to_string()));
        assert!(labels.contains(&&"Assets:Investments:Stocks".to_string()));
    }

    #[test]
    fn test_lowercase_fuzzy_completion() {
        // Test the fuzzy search functionality directly
        use crate::providers::completion::fuzzy_search_accounts;

        let accounts = vec![
            "Assets:Cash:Checking".to_string(),
            "Assets:Investments:Stocks".to_string(),
            "Expenses:Petty:Cash".to_string(),
        ];

        let matches = fuzzy_search_accounts(&accounts, "cash");
        assert!(!matches.is_empty());

        // Should find both accounts containing "Cash"
        let cash_accounts: Vec<&(String, f32)> = matches
            .iter()
            .filter(|(acc, _)| acc.contains("Cash"))
            .collect();
        assert_eq!(cash_accounts.len(), 2);
    }

    #[test]
    fn test_mixed_case_exact_completion() {
        // Test the search mode determination for mixed case
        use crate::providers::completion::{determine_search_mode, SearchMode};

        // Mixed case should use exact matching
        assert_eq!(determine_search_mode("Assets"), SearchMode::Exact);
        assert_eq!(determine_search_mode("AssetS"), SearchMode::Exact);
        assert_eq!(determine_search_mode("As"), SearchMode::Exact);
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
