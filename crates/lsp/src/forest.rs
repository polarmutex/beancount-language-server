use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::server::ProgressMsg;
use crate::server::Task;
use crate::utils::ToFilePath;
use crossbeam_channel::Sender;
use glob::glob;
use std::collections::{HashMap, HashSet, linked_list::LinkedList};
use std::fs;
use std::path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::error;
use tree_sitter_beancount::tree_sitter;

#[derive(Debug, Clone)]
struct FileCache {
    content: String,
    modified: SystemTime,
}

type FileCacheMap = HashMap<PathBuf, FileCache>;

fn read_file_cached(path: &PathBuf, cache: &mut FileCacheMap) -> anyhow::Result<String> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;

    if let Some(cached) = cache.get(path)
        && cached.modified >= modified
    {
        tracing::debug!("Cache hit for file: {:?}", path);
        return Ok(cached.content.clone());
    }

    tracing::debug!("Reading file from disk: {:?}", path);
    let content = fs::read_to_string(path)?;
    cache.insert(
        path.clone(),
        FileCache {
            content: content.clone(),
            modified,
        },
    );
    Ok(content)
}

// Issus to look at if running into issues with this
// https://github.com/silvanshade/lspower/issues/8
pub(crate) fn parse_initial_forest(
    snapshot: LspServerStateSnapshot,
    root_url: PathBuf,
    sender: Sender<Task>,
) -> anyhow::Result<bool, anyhow::Error> {
    let mut seen_files = LinkedList::new();
    let mut file_cache = FileCacheMap::new();
    // let root_pathbuf: String = self.root_journal_path.into_inner().unwrap().as_ref().as_os_str();
    // let temp = self.root_journal_path.read().await;
    // let root_url = lsp::Url::from_file_path(temp.clone().unwrap()).unwrap();
    seen_files.push_back(root_url.clone());
    let mut done = false;

    let mut to_processs = LinkedList::new();
    let mut new_to_processs = LinkedList::new();
    to_processs.push_back(root_url);
    let mut processed = 0;
    let mut total = 1;

    sender
        .send(Task::Progress(ProgressMsg::ForestInit {
            done: processed,
            total,
            data: Box::new(None),
        }))
        .unwrap();

    while !done {
        let mut temp = to_processs.clone();
        let mut iter = temp.iter_mut().peekable();

        while iter.peek().is_some() {
            let file = iter.next().unwrap();
            tracing::info!("processing {:#?}", file);
            //session
            //    .client
            //    .log_message(lsp::MessageType::INFO, format!("parsing {}", file.as_ref()))
            //    .await;

            processed += 1;

            let text = read_file_cached(file, &mut file_cache)?;
            let bytes = text.as_bytes();

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(&text, None).unwrap();
            let tree_arc = Arc::new(tree);
            let mut cursor = tree_arc.root_node().walk();

            let content = ropey::Rope::from_str(text.as_str());
            let beancount_data = BeancountData::new(&tree_arc, &content);

            sender
                .send(Task::Progress(ProgressMsg::ForestInit {
                    done: processed,
                    total,
                    data: Box::new(Some((
                        file.clone(),
                        tree_arc.clone(),
                        Arc::new(beancount_data),
                    ))),
                }))
                .unwrap();

            //snapshot.forest.insert(file.clone(), tree.clone());

            let include_patterns: Vec<String> = tree_arc
                .root_node()
                .children(&mut cursor)
                .filter(|c| c.kind() == "include")
                .filter_map(|include_node| {
                    let mut node_cursor = include_node.walk();
                    let node = include_node
                        .children(&mut node_cursor)
                        .find(|c| c.kind() == "string")?;

                    let filename = node
                        .utf8_text(bytes)
                        .unwrap()
                        .trim_start_matches('"')
                        .trim_end_matches('"');

                    let path = path::Path::new(filename);

                    let path = if path.is_absolute() {
                        path.to_path_buf()
                    } else if file.is_absolute() {
                        file.parent().unwrap().join(path)
                    } else {
                        path.to_path_buf()
                    };

                    Some(path.to_string_lossy().to_string())
                })
                .collect();

            // Process all include patterns and deduplicate results
            let mut discovered_files = HashSet::new();
            for pattern in include_patterns {
                match glob(&pattern) {
                    Ok(paths) => {
                        for entry in paths {
                            match entry {
                                Ok(path) => {
                                    discovered_files.insert(path);
                                }
                                Err(e) => error!("Glob entry error: {:?}", e),
                            }
                        }
                    }
                    Err(e) => error!("Glob pattern error for '{}': {:?}", pattern, e),
                }
            }

            // Convert discovered files to URLs and add to processing queue
            for path in discovered_files {
                // Handle cross-platform file URI creation
                let path_str = path.to_str().unwrap();
                let uri_str = if cfg!(windows)
                    && path_str.len() > 1
                    && path_str.chars().nth(1) == Some(':')
                {
                    // Windows absolute path like "C:\path"
                    format!("file:///{}", path_str.replace('\\', "/"))
                } else if cfg!(windows) && path_str.starts_with('/') {
                    // Unix-style path on Windows, convert to Windows style
                    format!("file:///C:{}", path_str.replace('\\', "/"))
                } else {
                    // Unix path or other platforms
                    format!("file://{path_str}")
                };
                let url = lsp_types::Uri::from_str(&uri_str)
                    .unwrap()
                    .to_file_path()
                    .unwrap();
                if !snapshot.forest.contains_key(&url) && !seen_files.contains(&url) {
                    total += 1;
                    new_to_processs.push_back(url.clone());
                    seen_files.push_back(url);
                }
            }
        }

        if new_to_processs.is_empty() {
            done = true;
        } else {
            to_processs.clear();
            to_processs.clone_from(&new_to_processs);
            new_to_processs.clear();
        }
    }

    sender
        .send(Task::Progress(ProgressMsg::ForestInit {
            done: processed,
            total,
            data: Box::new(None),
        }))
        .unwrap();

    Ok(true)
}
