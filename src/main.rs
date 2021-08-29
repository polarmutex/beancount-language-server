use clap::App;
use lspower::{LspService, Server};
mod core;
mod handlers;
mod server;

fn cli() {
    App::new("beancount-language-server").get_matches();
}

#[tokio::main]
async fn main() {
    cli();
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, messages) = LspService::new(|client| server::Server::new(client).unwrap());
    Server::new(stdin, stdout).interleave(messages).serve(service).await;
}
