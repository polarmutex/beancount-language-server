use async_lsp::client_monitor::ClientProcessMonitorLayer;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use beancount_language_server::server::LspServerState;
use clap::{arg, Command};
use std::fs;
use std::io;
use std::sync::Arc;
use tower::ServiceBuilder;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{filter::Directive, EnvFilter};

#[tokio::main]
async fn main() {
    let matches = Command::new("beancount-language-server")
        .args(&[
            arg!(--stdio "specifies to use stdio to communicate with lsp"),
            arg!(--log "write log to file"),
        ])
        .get_matches();

    setup_logging(matches.get_flag("log"));

    #[cfg(unix)]
    let (stdin, stdout) = (
        async_lsp::stdio::PipeStdin::lock_tokio().unwrap(),
        async_lsp::stdio::PipeStdout::lock_tokio().unwrap(),
    );

    #[cfg(not(unix))]
    let (stdin, stdout) = (
        tokio_util::compat::TokioAsyncReadCompatExt::compat(tokio::io::stdin()),
        tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tokio::io::stdout()),
    );

    let concurrency = match std::thread::available_parallelism() {
        // Double the concurrency limit since many handlers are blocking anyway.
        Ok(n) => n,
        Err(err) => {
            tracing::error!("Failed to get available parallelism: {err}");
            2.try_into().expect("2 is not 0")
        }
    };
    tracing::info!("Max concurrent requests: {concurrency}");

    let (mainloop, _) = async_lsp::MainLoop::new_server(|client| {
        ServiceBuilder::new()
            .layer(
                TracingLayer::new()
                    .request(|r| tracing::info_span!("request", method = r.method))
                    .notification(|n| tracing::info_span!("notification", method = n.method))
                    .event(|e| tracing::info_span!("event", method = e.type_name())),
            )
            .layer(LifecycleLayer::default())
            .layer(ConcurrencyLayer::new(concurrency))
            .layer(ClientProcessMonitorLayer::new(client.clone()))
            .service(LspServerState::new_router(client))
    });

    mainloop.run_buffered(stdin, stdout).await.unwrap();
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
