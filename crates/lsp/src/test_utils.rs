use crate::beancount_data::BeancountData;
use crate::config::Config;
use crate::document::Document;
use crate::server::LspServerStateSnapshot;
use crate::utils::ToFilePath;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

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
    pub snapshot: LspServerStateSnapshot,
    pub fixture: Fixture,
}

impl TestState {
    /// Converts a test fixture path to a PathBuf, handling cross-platform compatibility.
    /// On Windows, converts Unix-style paths like "/main.beancount" to "C:\main.beancount"
    pub fn path_from_fixture(path: &str) -> Result<PathBuf> {
        let uri_str = if cfg!(windows) && path.starts_with('/') {
            // On Windows, convert Unix-style absolute paths to Windows-style
            format!("file://C:{path}")
        } else {
            format!("file://{path}")
        };

        lsp_types::Uri::from_str(&uri_str)
            .map_err(|e| anyhow::anyhow!("Invalid URI: {}", e))?
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("Failed to convert URI to file path: {}", uri_str))
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
                config: Config::new(std::env::current_dir()?),
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
        let uri = lsp_types::Uri::from_str(format!("file://{path}").as_str()).unwrap();
        let id = lsp_types::TextDocumentIdentifier::new(uri);
        Some(lsp_types::TextDocumentPositionParams::new(id, cursor))
    }
}
