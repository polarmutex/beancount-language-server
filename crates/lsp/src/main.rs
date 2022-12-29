use clap::{Arg, Command};
use std::fs;
use std::io;
use std::sync::Arc;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{filter::Directive, EnvFilter};

fn main() {
    let matches = Command::new("beancount-language-server")
        .arg(
            Arg::new("stdio")
                .long("stdio")
                .help("use std io for lang server"),
        )
        //TODO let the user specify the file
        .arg(Arg::new("log").long("log").help("Write logs to file"))
        .get_matches();

    setup_logging(matches.contains_id("log"));

    beancount_language_server::run_server()
        .map_err(|e| anyhow::anyhow!("{}", e))
        .unwrap();
}

fn setup_logging(file: bool) {
    let file = if file {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("beancount-language-server.log")
            .ok()
    } else {
        None
    };

    let writer = match file {
        Some(file) => BoxMakeWriter::new(Arc::new(file)),
        None => BoxMakeWriter::new(io::stderr),
    };

    let filter = EnvFilter::default().add_directive(Directive::from(LevelFilter::DEBUG));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .init();
}
