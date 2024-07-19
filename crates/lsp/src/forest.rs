use crate::beancount_data::BeancountData;
use crate::server::LspServerStateSnapshot;
use crate::server::ProgressMsg;
use crate::server::Task;
use crate::utils::ToFilePath;
use crossbeam_channel::Sender;
use glob::glob;
use std::collections::linked_list::LinkedList;
use std::fs;
use std::path;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::error;

// Issus to look at if running into issues with this
// https://github.com/silvanshade/lspower/issues/8
pub(crate) fn parse_initial_forest(
    snapshot: LspServerStateSnapshot,
    root_url: PathBuf,
    sender: Sender<Task>,
) -> anyhow::Result<bool, anyhow::Error> {
    let mut seen_files = LinkedList::new();
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

            let text = fs::read_to_string(file.clone())?;
            let bytes = text.as_bytes();

            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_beancount::language())?;
            let tree = parser.parse(&text, None).unwrap();
            let mut cursor = tree.root_node().walk();

            let content = ropey::Rope::from_str(text.as_str());
            let beancount_data = BeancountData::new(&tree, &content);

            sender
                .send(Task::Progress(ProgressMsg::ForestInit {
                    done: processed,
                    total,
                    data: Box::new(Some((file.clone(), tree.clone(), beancount_data))),
                }))
                .unwrap();

            //snapshot.forest.insert(file.clone(), tree.clone());

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
                    } else if file.is_absolute() {
                        file.parent().unwrap().join(path)
                    } else {
                        path.to_path_buf()
                    };
                    let path_url = lsp_types::Uri::from_str(
                        format!("file://{}", path.to_str().unwrap()).as_str(),
                    )
                    .unwrap();

                    Some(path_url)
                });

            // This could get in an infinite loop if there is a loop wtth the include files
            // TODO see if I can prevent this
            for include_url in include_filenames {
                for entry in glob(include_url.to_file_path().unwrap().to_str().unwrap())
                    .expect("Failed to read glob")
                {
                    match entry {
                        Ok(path) => {
                            let url = lsp_types::Uri::from_str(
                                format!("file://{}", path.to_str().unwrap()).as_str(),
                            )
                            .unwrap()
                            .to_file_path()
                            .unwrap();
                            if !snapshot.forest.contains_key(&url) {
                                total += 1;
                                new_to_processs.push_back(url);
                            }
                        }
                        Err(e) => error!("{:?}", e),
                    }
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
