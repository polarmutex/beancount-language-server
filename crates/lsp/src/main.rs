use beancount_language_server::server::run_server;
use clap::{Arg, Command};
use tracing::{debug_span, level_filters::LevelFilter, Instrument};
use tracing_subscriber::{filter::Directive, layer::SubscriberExt, EnvFilter};
use tracing_tree::HierarchicalLayer;

#[tokio::main]
async fn main() {
    let _matches = Command::new("beancount-language-server")
        .arg(
            Arg::new("stdio")
                .long("stdio")
                .help("use std io for lang server"),
        )
        //TODO let the user specify the file
        .arg(Arg::new("log").long("log").help("Write logs to file"))
        .get_matches();

    let filter = EnvFilter::default().add_directive(Directive::from(LevelFilter::DEBUG));

    let (file_logger, _guard, std_logger) = if _matches.is_present("log") {
        println!("enable logging");
        let file_appender = tracing_appender::rolling::never(".", "beancount-language-server.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        (
            Some(
                HierarchicalLayer::default()
                    .with_indent_lines(true)
                    .with_indent_amount(4)
                    .with_bracketed_fields(true)
                    .with_ansi(false)
                    .with_writer(non_blocking),
            ),
            Some(guard),
            None,
        )
    } else {
        (
            None,
            None,
            Some(
                HierarchicalLayer::default()
                    .with_indent_lines(true)
                    .with_indent_amount(4)
                    .with_bracketed_fields(true),
            ),
        )
    };

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(file_logger)
        .with(std_logger);
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let span = debug_span!("Running Beancount LSP Server", pid = std::process::id());
    tracing::debug!("Test");
    tracing::info!("Test");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    run_server(stdin, stdout).instrument(span).await
}
