use clap::{Arg, Command};
mod core;
mod handlers;
mod providers;
mod server;

use crate::core::logger::Logger;
use server::run_server;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
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

    run_server(stdin, stdout).await
}
