use clap::Command;
use lspower::{LspService, Server};
mod core;
mod handlers;
mod providers;
mod server;

use crate::core::logger::Logger;
use std::path::PathBuf;

fn cli() {
    Command::new("beancount-language-server").get_matches();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli();

    let mut logger = Logger::new().unwrap();
    logger
        .set_path(Some(PathBuf::from("beancount-langserver.log")))
        .expect("Could not open log file");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, messages) = LspService::new(|client| server::Server::new(client).unwrap());
    Server::new(stdin, stdout).interleave(messages).serve(service).await;
    Ok(())
}
