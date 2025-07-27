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
            arg!(--log [LOG_LEVEL] "write log to file with optional level (trace, debug, info, warn, error)"),
            arg!(version: -v --version),
        ])
        .get_matches();

    if matches.args_present() && matches.get_flag("version") {
        print!("{}", std::env!("CARGO_PKG_VERSION"));
        return;
    }

    let log_to_file = matches.contains_id("log");
    let log_level = matches.get_one::<String>("log");
    setup_logging(log_to_file, log_level);

    tracing::info!(
        "Starting beancount-language-server v{}",
        env!("CARGO_PKG_VERSION")
    );
    tracing::debug!(
        "Command line args: stdio={}, log_to_file={}, log_level={:?}",
        matches.get_flag("stdio"),
        log_to_file,
        log_level
    );

    match beancount_language_server::run_server() {
        Ok(()) => {
            tracing::info!("Language server shutdown gracefully");
        }
        Err(e) => {
            tracing::error!("Language server failed with error: {}", e);
            std::process::exit(1);
        }
    }
}

fn setup_logging(log_to_file: bool, log_level_arg: Option<&String>) {
    let level = match log_level_arg {
        Some(level_str) => parse_log_level(level_str),
        None => {
            if log_to_file {
                LevelFilter::DEBUG  // Default level when logging to file
            } else {
                LevelFilter::INFO   // Default level when logging to stderr
            }
        }
    };

    let file = if log_to_file {
        match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("beancount-language-server.log")
        {
            Ok(f) => {
                eprintln!("Logging to file: beancount-language-server.log");
                Some(f)
            }
            Err(e) => {
                eprintln!("Failed to open log file: {e}. Falling back to stderr.");
                None
            }
        }
    } else {
        None
    };

    let writer = match file {
        Some(file) => BoxMakeWriter::new(Arc::new(file)),
        None => BoxMakeWriter::new(io::stderr),
    };

    let filter = EnvFilter::default().add_directive(Directive::from(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true)
        .init();
}

fn parse_log_level(level_str: &str) -> LevelFilter {
    match level_str.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        "off" => LevelFilter::OFF,
        _ => {
            eprintln!("Invalid log level '{}'. Using 'info' as default. Valid levels: trace, debug, info, warn, error, off", level_str);
            LevelFilter::INFO
        }
    }
}
