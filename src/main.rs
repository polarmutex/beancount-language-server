use clap::{Arg, Command};
use lspower::{LspService, Server};
mod core;
mod handlers;
mod providers;
mod server;

use crate::core::logger::Logger;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("beancount-language-server")
        .arg(Arg::new("stdio").long("stdio").help("use std io for lang server"))
        //TODO let the user specify the file
        .arg(Arg::new("log").long("log").help("Write logs to file"))
        .get_matches();

    if matches.is_present("log") {
        let mut logger = Logger::new().unwrap();
        logger
            .set_path(Some(PathBuf::from("beancount-langserver.log")))
            .expect("Could not open log file");
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, messages) = LspService::new(|client| server::Server::new(client).unwrap());
    Server::new(stdin, stdout).interleave(messages).serve(service).await;
    Ok(())
}
