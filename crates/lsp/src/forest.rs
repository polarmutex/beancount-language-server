use crate::{progress, session::Session};
use glob::glob;
use log::error;
use std::collections::linked_list::LinkedList;
use std::fs;
use std::path;
use tokio::sync::Mutex;
use tower_lsp::lsp_types as lsp;

// Issus to look at if running into issues with this
// https://github.com/silvanshade/lspower/issues/8
pub(crate) async fn parse_initial_forest(session: &Session, root_url: lsp::Url) -> anyhow::Result<bool, anyhow::Error> {
    let progress_token = progress::progress_begin(&session.client, "Generating Forest").await;

    let mut seen_files = LinkedList::new();
    // let root_pathbuf: String = self.root_journal_path.into_inner().unwrap().as_ref().as_os_str();
    // let temp = self.root_journal_path.read().await;
    // let root_url = lsp::Url::from_file_path(temp.clone().unwrap()).unwrap();
    seen_files.push_back(root_url.clone());
    let mut done = false;

    let mut to_processs = LinkedList::new();
    let mut new_to_processs = LinkedList::new();
    to_processs.push_back(root_url.clone());

    while !done {
        let mut temp = to_processs.clone();
        let mut iter = temp.iter_mut().peekable();

        while iter.peek().is_some() {
            let file = iter.next().unwrap();
            session
                .client
                .log_message(lsp::MessageType::INFO, format!("parsing {}", file.as_ref()))
                .await;

            let file_path = file.to_file_path().ok().unwrap();

            progress::progress(&session.client, progress_token.clone(), file.to_string()).await;

            let text = fs::read_to_string(file_path.clone())?;
            let bytes = text.as_bytes();

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(tree_sitter_beancount::language())?;
            let tree = parser.parse(&text, None).unwrap();
            session.parsers.insert(file.clone(), Mutex::new(parser));
            let mut cursor = tree.root_node().walk();

            session.forest.insert(file.clone(), Mutex::new(tree.clone()));

            let content = ropey::Rope::from_str(text.as_str());
            session.beancount_data.update_data(file.clone(), &tree, &content);

            let include_filenames = tree
                .root_node()
                .children(&mut cursor)
                .filter(|c| c.kind() == "include")
                .filter_map(|include_node| {
                    let mut node_cursor = include_node.walk();
                    let node = include_node.children(&mut node_cursor).find(|c| c.kind() == "string")?;

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
                    let path_url = lsp::Url::from_file_path(path).unwrap();

                    Some(path_url)
                });

            // This could get in an infinite loop if there is a loop wtth the include files
            // TODO see if I can prevent this
            for include_url in include_filenames {
                for entry in glob(include_url.path()).expect("Failed to read glob") {
                    match entry {
                        Ok(path) => {
                            let url = lsp::Url::from_file_path(path).unwrap();
                            if !session.forest.contains_key(&url) {
                                new_to_processs.push_back(url);
                            }
                        },
                        Err(e) => error!("{:?}", e),
                    }
                }
            }
        }

        if new_to_processs.len() == 0 {
            done = true;
        } else {
            to_processs.clear();
            to_processs = new_to_processs.clone();
            new_to_processs.clear();
        }
    }

    progress::progress_end(&session.client, progress_token).await;

    Ok(true)
}
