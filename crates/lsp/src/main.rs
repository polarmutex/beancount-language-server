use clap::{Arg, Command};

use beancount_language_server::server::run_server;

#[tokio::main]
async fn main() {
    let _matches = Command::new("beancount-language-server")
        .arg(Arg::new("stdio").long("stdio").help("use std io for lang server"))
        //TODO let the user specify the file
        //.arg(Arg::new("log").long("log").help("Write logs to file"))
        .get_matches();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    run_server(stdin, stdout).await
}
