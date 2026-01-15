//! Forest parsing for beancount files
//!
//! This module handles initial parsing of beancount file forests, including:
//! - Recursive include directive resolution
//! - File caching for performance
//! - Progress tracking during parsing
//!
//! # Query Usage
//!
//! Uses tree-sitter queries for extracting include directives:
//! - `(include (string) @string)` - Extract string nodes from include directives
//!
//! This is more efficient and clearer than manual tree walking.

use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::server::ProgressMsg;
use crate::server::Task;
use crossbeam_channel::Sender;
use glob::glob;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path;
use std::path::PathBuf;
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
    let mut seen_files = HashSet::new();
    let mut file_cache = FileCacheMap::new();
    seen_files.insert(root_url.clone());

    let mut to_process = VecDeque::new();
    to_process.push_back(root_url);
    let mut processed = 0;
    let mut total = 1;

    sender
        .send(Task::Progress(ProgressMsg::ForestInit {
            done: processed,
            total,
            data: Box::new(None),
        }))
        .unwrap();

    while let Some(file) = to_process.pop_front() {
        tracing::info!("processing {:#?}", file);

        processed += 1;

        let text = read_file_cached(&file, &mut file_cache)?;
        let bytes = text.as_bytes();

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_beancount::language())?;
        let tree = match parser.parse(&text, None) {
            Some(tree) => tree,
            None => {
                error!("Failed to parse {:?}, skipping file", file);
                continue;
            }
        };
        let tree_arc = Arc::new(tree);

        let content = ropey::Rope::from_str(text.as_str());
        let beancount_data = BeancountData::new(&tree_arc, &content);

        // Always send data for the parsed file (server needs it)
        // But we could batch progress updates in the future if needed
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

        // Extract include patterns using tree-sitter query
        let include_query_string = r#"
        (include (string) @string)
        "#;
        let include_query =
            tree_sitter::Query::new(&tree_sitter_beancount::language(), include_query_string)
                .unwrap_or_else(|_| panic!("Invalid query for includes: {include_query_string}"));
        let mut cursor_qry = tree_sitter::QueryCursor::new();
        let mut include_matches = cursor_qry.matches(&include_query, tree_arc.root_node(), bytes);

        let include_patterns: Vec<String> = {
            use tree_sitter::StreamingIterator;
            let mut patterns = Vec::new();

            while let Some(qmatch) = include_matches.next() {
                for capture in qmatch.captures {
                    let filename = capture
                        .node
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

                    patterns.push(path.to_string_lossy().to_string());
                }
            }

            patterns
        };

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
            // Use url crate for proper cross-platform file URI handling
            let url = match url::Url::from_file_path(&path) {
                Ok(url) => url,
                Err(_) => {
                    error!("Failed to convert path to URL: {:?}", path);
                    continue;
                }
            };

            let path_buf = match url.to_file_path() {
                Ok(path_buf) => path_buf,
                Err(_) => {
                    error!("Failed to convert URL back to path: {}", url);
                    continue;
                }
            };

            if !snapshot.forest.contains_key(&path_buf) && !seen_files.contains(&path_buf) {
                total += 1;
                to_process.push_back(path_buf.clone());
                seen_files.insert(path_buf);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    /// Helper to create a test snapshot
    fn create_test_snapshot() -> LspServerStateSnapshot {
        LspServerStateSnapshot {
            beancount_data: HashMap::new(),
            config: Config::new(PathBuf::from("/tmp/test.bean")),
            forest: HashMap::new(),
            open_docs: HashMap::new(),
            checker: None,
        }
    }

    /// Helper to create a temporary beancount file
    fn create_temp_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(name);
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file_path
    }

    #[test]
    fn test_read_file_cached_initial_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = create_temp_file(&temp_dir, "test.bean", "2023-01-01 open Assets:Cash");

        let mut cache = FileCacheMap::new();
        let content = read_file_cached(&file_path, &mut cache).unwrap();

        assert_eq!(content, "2023-01-01 open Assets:Cash");
        assert!(cache.contains_key(&file_path));
    }

    #[test]
    fn test_read_file_cached_cache_hit() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = create_temp_file(&temp_dir, "test.bean", "2023-01-01 open Assets:Cash");

        let mut cache = FileCacheMap::new();

        // First read - should populate cache
        let content1 = read_file_cached(&file_path, &mut cache).unwrap();

        // Second read - should hit cache (same content)
        let content2 = read_file_cached(&file_path, &mut cache).unwrap();

        assert_eq!(content1, content2);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_read_file_cached_invalidation_on_modification() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = create_temp_file(&temp_dir, "test.bean", "original content");

        let mut cache = FileCacheMap::new();

        // First read
        let content1 = read_file_cached(&file_path, &mut cache).unwrap();
        assert_eq!(content1, "original content");

        // Modify file
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(b"modified content").unwrap();
        drop(file);

        // Second read - should detect modification and re-read
        let content2 = read_file_cached(&file_path, &mut cache).unwrap();
        assert_eq!(content2, "modified content");
    }

    #[test]
    fn test_read_file_cached_nonexistent_file() {
        let mut cache = FileCacheMap::new();
        let result = read_file_cached(&PathBuf::from("/nonexistent/file.bean"), &mut cache);

        assert!(result.is_err());
        assert!(cache.is_empty());
    }

    #[test]
    fn test_parse_initial_forest_single_file() {
        let temp_dir = TempDir::new().unwrap();
        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            r#"2023-01-01 open Assets:Cash
2023-01-02 * "Payee" "Narration"
  Assets:Cash  100.00 USD
"#,
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Verify progress messages
        let mut messages = vec![];
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(msg) = task {
                messages.push(msg);
            }
        }

        // Should have initial, file processed, and final messages
        assert!(messages.len() >= 2);
    }

    #[test]
    fn test_parse_initial_forest_with_simple_include() {
        let temp_dir = TempDir::new().unwrap();

        let included_file =
            create_temp_file(&temp_dir, "included.bean", "2023-01-01 open Assets:Bank\n");

        let included_path = included_file.to_str().unwrap();
        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            &format!(
                r#"include "{}"
2023-01-01 open Assets:Cash
"#,
                included_path
            ),
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Collect parsed files from progress messages
        let mut parsed_files = HashSet::new();
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { data, .. }) = task
                && let Some((path, _, _)) = *data
            {
                parsed_files.insert(path);
            }
        }

        // Should have parsed both files
        assert_eq!(parsed_files.len(), 2);
    }

    #[test]
    fn test_parse_initial_forest_with_glob_include() {
        let temp_dir = TempDir::new().unwrap();

        // Create multiple files matching a pattern
        create_temp_file(
            &temp_dir,
            "accounts.bean",
            "2023-01-01 open Assets:Account1\n",
        );
        create_temp_file(
            &temp_dir,
            "transactions.bean",
            "2023-01-01 open Assets:Account2\n",
        );

        let glob_pattern = temp_dir.path().join("*.bean");
        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            &format!(
                r#"include "{}"
"#,
                glob_pattern.to_str().unwrap()
            ),
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Collect parsed files
        let mut parsed_files = HashSet::new();
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { data, .. }) = task
                && let Some((path, _, _)) = *data
            {
                parsed_files.insert(path);
            }
        }

        // Should have parsed main.bean + accounts.bean + transactions.bean = 3 files
        assert!(parsed_files.len() >= 3);
    }

    #[test]
    fn test_parse_initial_forest_with_relative_include() {
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();

        let included_file = subdir.join("included.bean");
        let mut file = fs::File::create(&included_file).unwrap();
        file.write_all(b"2023-01-01 open Assets:SubAccount\n")
            .unwrap();

        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            r#"include "subdir/included.bean"
2023-01-01 open Assets:Cash
"#,
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Collect parsed files
        let mut parsed_files = HashSet::new();
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { data, .. }) = task
                && let Some((path, _, _)) = *data
            {
                parsed_files.insert(path);
            }
        }

        // Should have parsed both files
        assert_eq!(parsed_files.len(), 2);
    }

    #[test]
    fn test_parse_initial_forest_skips_malformed_file() {
        let temp_dir = TempDir::new().unwrap();

        // Create a file with invalid content that tree-sitter can't parse
        let malformed_file = create_temp_file(
            &temp_dir,
            "malformed.bean",
            "\x00\x01\x02\x03\x04", // Binary garbage
        );

        let malformed_path = malformed_file.to_str().unwrap();
        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            &format!(
                r#"include "{}"
2023-01-01 open Assets:Cash
"#,
                malformed_path
            ),
        );

        let snapshot = create_test_snapshot();
        let (sender, _receiver) = crossbeam_channel::unbounded();

        // Should not panic, should handle gracefully
        let result = parse_initial_forest(snapshot, root_file, sender);

        // Should succeed even with malformed file
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_initial_forest_deduplicates_files() {
        let temp_dir = TempDir::new().unwrap();

        let included_file =
            create_temp_file(&temp_dir, "common.bean", "2023-01-01 open Assets:Common\n");

        let included_path = included_file.to_str().unwrap();
        // Include the same file twice
        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            &format!(
                r#"include "{}"
include "{}"
2023-01-01 open Assets:Cash
"#,
                included_path, included_path
            ),
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Collect parsed files
        let mut parsed_files = HashSet::new();
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { data, .. }) = task
                && let Some((path, _, _)) = *data
            {
                parsed_files.insert(path);
            }
        }

        // Should have parsed only 2 unique files (main + common once)
        assert_eq!(parsed_files.len(), 2);
    }

    #[test]
    fn test_parse_initial_forest_circular_includes() {
        let temp_dir = TempDir::new().unwrap();

        let file_a_path = temp_dir.path().join("a.bean");
        let file_b_path = temp_dir.path().join("b.bean");

        // Create file A that includes B
        let mut file_a = fs::File::create(&file_a_path).unwrap();
        file_a
            .write_all(
                format!(
                    r#"include "{}"
2023-01-01 open Assets:A
"#,
                    file_b_path.to_str().unwrap()
                )
                .as_bytes(),
            )
            .unwrap();

        // Create file B that includes A (circular)
        let mut file_b = fs::File::create(&file_b_path).unwrap();
        file_b
            .write_all(
                format!(
                    r#"include "{}"
2023-01-01 open Assets:B
"#,
                    file_a_path.to_str().unwrap()
                )
                .as_bytes(),
            )
            .unwrap();

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        // Should handle circular includes without infinite loop
        let result = parse_initial_forest(snapshot, file_a_path, sender);

        assert!(result.is_ok());

        // Collect parsed files
        let mut parsed_files = HashSet::new();
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { data, .. }) = task
                && let Some((path, _, _)) = *data
            {
                parsed_files.insert(path);
            }
        }

        // Should have parsed both files exactly once
        assert_eq!(parsed_files.len(), 2);
    }

    #[test]
    fn test_parse_initial_forest_nonexistent_include() {
        let temp_dir = TempDir::new().unwrap();

        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            r#"include "/nonexistent/file.bean"
2023-01-01 open Assets:Cash
"#,
        );

        let snapshot = create_test_snapshot();
        let (sender, _receiver) = crossbeam_channel::unbounded();

        // Should handle missing include gracefully
        let result = parse_initial_forest(snapshot, root_file, sender);

        // Should succeed (just skip the missing file)
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_initial_forest_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let root_file = create_temp_file(&temp_dir, "empty.bean", "");

        let snapshot = create_test_snapshot();
        let (sender, _receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Should still send progress messages
        let mut message_count = 0;
        while _receiver.try_recv().is_ok() {
            message_count += 1;
        }

        assert!(message_count > 0);
    }

    #[test]
    fn test_parse_initial_forest_progress_tracking() {
        let temp_dir = TempDir::new().unwrap();

        // Create multiple files to track progress
        let file1 = create_temp_file(&temp_dir, "file1.bean", "2023-01-01 open Assets:A\n");
        let file2 = create_temp_file(&temp_dir, "file2.bean", "2023-01-01 open Assets:B\n");

        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            &format!(
                r#"include "{}"
include "{}"
2023-01-01 open Assets:Main
"#,
                file1.to_str().unwrap(),
                file2.to_str().unwrap()
            ),
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        // Verify progress tracking is correct
        let mut max_total = 0;
        let mut max_done = 0;
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { done, total, .. }) = task {
                max_total = max_total.max(total);
                max_done = max_done.max(done);
            }
        }

        assert_eq!(max_total, 3); // main + file1 + file2
        assert_eq!(max_done, 3); // All files processed
    }

    #[test]
    fn test_parse_initial_forest_with_absolute_path() {
        let temp_dir = TempDir::new().unwrap();

        let included_file =
            create_temp_file(&temp_dir, "included.bean", "2023-01-01 open Assets:Bank\n");

        let absolute_path = included_file.canonicalize().unwrap();
        let root_file = create_temp_file(
            &temp_dir,
            "main.bean",
            &format!(
                r#"include "{}"
2023-01-01 open Assets:Cash
"#,
                absolute_path.to_str().unwrap()
            ),
        );

        let snapshot = create_test_snapshot();
        let (sender, receiver) = crossbeam_channel::unbounded();

        let result = parse_initial_forest(snapshot, root_file, sender);

        assert!(result.is_ok());

        let mut parsed_files = HashSet::new();
        while let Ok(task) = receiver.try_recv() {
            if let Task::Progress(ProgressMsg::ForestInit { data, .. }) = task
                && let Some((path, _, _)) = *data
            {
                parsed_files.insert(path);
            }
        }

        assert_eq!(parsed_files.len(), 2);
    }

    #[test]
    fn test_file_cache_preserves_across_multiple_reads() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_temp_file(&temp_dir, "file1.bean", "content 1");
        let file2 = create_temp_file(&temp_dir, "file2.bean", "content 2");

        let mut cache = FileCacheMap::new();

        // Read both files
        read_file_cached(&file1, &mut cache).unwrap();
        read_file_cached(&file2, &mut cache).unwrap();

        assert_eq!(cache.len(), 2);

        // Read again - should hit cache
        read_file_cached(&file1, &mut cache).unwrap();
        read_file_cached(&file2, &mut cache).unwrap();

        assert_eq!(cache.len(), 2);
    }
}
