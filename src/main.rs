use clap::App;
use lspower::{LspService, Server};
mod core;
mod handlers;
mod server;

use crate::core::logger::Logger;
use std::path::PathBuf;

fn cli() {
    App::new("beancount-language-server").get_matches();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli();

    let mut logger = Logger::new().unwrap();
    logger.set_path(Some(PathBuf::from("beancount-langserver.log")));

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, messages) = LspService::new(|client| server::Server::new(client).unwrap());
    Server::new(stdin, stdout).interleave(messages).serve(service).await;
    Ok(())
}
