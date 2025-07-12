use clap::{arg, Command};
use std::fs;
use std::io;
use std::sync::Arc;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{filter::Directive, EnvFilter};

fn main() {
    let matches = Command::new("beancount-language-server")
        .args(&[
            arg!(--stdio "specifies to use stdio to communicate with lsp"),
            arg!(--log "write log to file"),
            arg!(version: -v --version),
        ])
        .get_matches();

    if matches.args_present() && matches.get_flag("version") {
        print!("{}", std::env!("CARGO_PKG_VERSION"));
        return;
    }

    setup_logging(matches.get_flag("log"));

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

    let filter = EnvFilter::default().add_directive(Directive::from(LevelFilter::INFO));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .init();
}
