use crate::beancount_data::BeancountData;
use crate::progress::Progress;
use async_lsp::lsp_types;
use async_lsp::ClientSocket;
use glob::glob;
use std::collections::linked_list::LinkedList;
use std::collections::HashMap;
use std::fs;
use std::path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::error;

// Issus to look at if running into issues with this
// https://github.com/silvanshade/lspower/issues/8
pub(crate) async fn parse_initial_forest(
    client: ClientSocket,
    forest: Arc<RwLock<HashMap<lsp_types::Url, tree_sitter::Tree>>>,
    data: Arc<RwLock<HashMap<lsp_types::Url, BeancountData>>>,
    root_url: lsp_types::Url,
) {
    let progress = Progress::new(&client, String::from("blsp/forest")).await;
    progress.begin(
        String::from("Fetching flake with inputs"),
        String::from("nix flake archive"),
    );

    let mut seen_files = LinkedList::new();
    seen_files.push_back(root_url.clone());

    let mut processed = 0;
    let mut done = false;
    let mut total = 1;

    let mut to_processs = LinkedList::new();
    to_processs.push_back(root_url);
    let mut new_to_processs = LinkedList::new();

    tokio::time::sleep(Duration::from_millis(1)).await;

    while !done {
        let mut temp = to_processs.clone();
        let mut iter = temp.iter_mut().peekable();

        while iter.peek().is_some() {
            let file = iter.next().unwrap();
            let file_path = file.to_file_path().ok().unwrap();

            processed += 1;
            // if processed % 10 == 0 {
            // need sleep for notif to go through
            tokio::time::sleep(Duration::from_nanos(1)).await;
            // }
            tracing::info!("processing {}", file.to_string());

            // tokio::time::sleep(Duration::from_millis(100)).await;

            let text = fs::read_to_string(file_path.clone()).expect("");
            let bytes = text.as_bytes();
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(tree_sitter_beancount::language())
                .expect("");
            let tree = parser.parse(&text, None).unwrap();
            let mut cursor = tree.root_node().walk();

            let content = ropey::Rope::from_str(text.as_str());
            let beancount_data = BeancountData::new(&tree, &content);

            forest.write().unwrap().insert(file.clone(), tree.clone());
            data.write().unwrap().insert(file.clone(), beancount_data);

            let include_filenames = tree
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
                    } else if file_path.is_absolute() {
                        file_path.parent().unwrap().join(path)
                    } else {
                        path.to_path_buf()
                    };
                    let path_url = lsp_types::Url::from_file_path(path).unwrap();

                    Some(path_url)
                });
            // This could get in an infinite loop if there is a loop wtth the include files
            // TODO see if I can prevent this
            for include_url in include_filenames {
                for entry in glob(include_url.path()).expect("Failed to read glob") {
                    match entry {
                        Ok(path) => {
                            let url = lsp_types::Url::from_file_path(path).unwrap();
                            if !forest.read().unwrap().contains_key(&url) {
                                total += 1;
                                new_to_processs.push_back(url);
                            }
                        }
                        Err(e) => error!("{:?}", e),
                    }
                }
            }

            progress.report(
                (processed * 100 / total) as u32,
                format!("[{processed}/{total}]"),
            );
        }

        if new_to_processs.is_empty() {
            done = true;
        } else {
            to_processs.clear();
            to_processs = new_to_processs.clone();
            new_to_processs.clear();
        }
    }

    progress.done(None);
    // // let root_pathbuf: String = self.root_journal_path.into_inner().unwrap().as_ref().as_os_str();
    // // let temp = self.root_journal_path.read().await;
    // // let root_url = lsp::Url::from_file_path(temp.clone().unwrap()).unwrap();
}
