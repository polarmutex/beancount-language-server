use clap::Parser;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::str::FromStr;
use std::sync::Arc;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{EnvFilter, filter::Directive};

pub mod beancount_data;
mod capabilities;
pub mod checkers;
mod config;
mod dispatcher;
pub mod document;
//pub mod error;
pub mod forest;
pub mod handlers;
pub mod progress;
pub mod providers;
mod query_utils;
pub mod server;
//pub mod session;
mod treesitter_utils;
mod utils;

use crate::config::Config;
use crate::server::LspServerState;
use anyhow::Result;
use lsp_server::Connection;
use lsp_types::InitializeParams;
use serde::{Serialize, de::DeserializeOwned};
use utils::ToFilePath;

pub fn run_server() -> Result<()> {
    tracing::info!("beancount-language-server started");

    //Setup IO connections
    tracing::debug!("Setting up stdio connections");
    let (connection, io_threads) = lsp_server::Connection::stdio();

    //wait for client to connection
    tracing::debug!("Waiting for client initialization");
    let (request_id, initialize_params) = connection.initialize_start()?;
    tracing::debug!("Received initialize request: id={}", request_id);

    let initialize_params = match serde_json::from_value::<InitializeParams>(initialize_params) {
        Ok(params) => {
            tracing::debug!("Successfully parsed initialization parameters");
            params
        }
        Err(e) => {
            tracing::error!("Failed to parse initialization parameters: {}", e);
            return Err(e.into());
        }
    };

    if let Some(client_info) = &initialize_params.client_info {
        tracing::info!(
            "Connected to client: '{}' version {}",
            client_info.name,
            client_info.version.as_deref().unwrap_or("unknown")
        );
    } else {
        tracing::warn!("Client did not provide client info");
    }

    // Parse config first so we can conditionally register capabilities
    let config = {
        let root_file = if let Some(workspace_folders) = &initialize_params.workspace_folders {
            let root = workspace_folders
                .first()
                .and_then(|folder| folder.uri.to_file_path().ok())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            tracing::info!("Using workspace folder as root: {}", root.display());
            root
        } else {
            #[allow(deprecated)]
            let root = match initialize_params
                .root_uri
                .and_then(|it| it.to_file_path().ok())
            {
                Some(it) => it,
                None => std::env::current_dir()?,
            };
            tracing::info!("Using root URI as root: {}", root.display());
            root
        };

        let mut config = Config::new(root_file);
        if let Some(json) = initialize_params.initialization_options {
            tracing::info!("Applying initialization options: {}", json);
            match config.update(json) {
                Ok(()) => tracing::debug!("Configuration updated successfully"),
                Err(e) => {
                    tracing::warn!("Failed to update configuration: {}", e);
                    return Err(e);
                }
            }
        } else {
            tracing::debug!("No initialization options provided, using default config");
        }
        config
    };

    let server_capabilities = capabilities::server_capabilities();
    tracing::debug!("Server capabilities configured");

    let initialize_result = lsp_types::InitializeResult {
        capabilities: server_capabilities,
        server_info: Some(lsp_types::ServerInfo {
            name: String::from("beancount-language-server"),
            version: Some(String::from(env!("CARGO_PKG_VERSION"))),
        }),
    };

    let initialize_result =
        serde_json::to_value(initialize_result).expect("Failed to serialize InitializeResult");

    connection.initialize_finish(request_id, initialize_result)?;
    tracing::info!("Initialization completed successfully");

    tracing::debug!("Starting main loop");
    main_loop(connection, config)?;

    tracing::debug!("Waiting for IO threads to complete");
    io_threads.join()?;
    tracing::info!("Language server stopped");

    Ok(())
}

fn main_loop(connection: Connection, config: Config) -> Result<()> {
    tracing::info!("initial config: {:#?}", config);
    LspServerState::new(connection.sender, config).run(connection.receiver)
}

pub fn from_json<T: DeserializeOwned>(what: &'static str, json: serde_json::Value) -> Result<T> {
    T::deserialize(&json)
        .map_err(|e| anyhow::anyhow!("could not deserialize {}: {} - {}", what, e, json))
}

pub fn to_json<T: Serialize>(value: T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|e| anyhow::anyhow!("could not serialize to json {}", e))
}

#[derive(Parser, Debug)]
#[command(name = "beancount-language-server", about = "Beancount LSP", version, long_about = None)]
struct Cli {
    #[arg(long, help = "Use stdio to communicate with the LSP")]
    stdio: bool,

    #[arg(
        long,
        value_name = "LOG_LEVEL",
        default_value = None,
        help = "Deprecated: log to default file with optional level (use --log-file and --log-level instead)",
    )]
    log: Option<String>,

    #[arg(
        long = "log-file",
        value_name = "LOG_FILE",
        default_value = None,
        help = "Write log output to the specified file instead of stderr"
    )]
    log_file: Option<String>,

    #[arg(
        long = "log-level",
        value_name = "LOG_LEVEL",
        default_value = None,
        help = "Set log level (trace, debug, info, warn, error, off); defaults to info"
    )]
    log_level: Option<String>,
}

pub fn main<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(args);

    let deprecated_log_used = cli.log.is_some();

    if deprecated_log_used {
        eprintln!("[deprecated]: --log is deprecated, use --log-file and --log-level instead.",);
    }

    let log_file = cli.log_file.clone().or_else(|| {
        if deprecated_log_used {
            Some("beancount-language-server.log".to_owned())
        } else {
            None
        }
    });

    let log_level = cli.log_level.clone().or(cli.log.clone());

    setup_logging(log_file.as_deref(), log_level.as_deref());

    tracing::info!(
        "Starting beancount-language-server v{}",
        env!("CARGO_PKG_VERSION")
    );
    tracing::debug!(
        "Command line args: stdio={}, log_target={}, log_level={:?}",
        cli.stdio,
        log_file.as_deref().unwrap_or("/dev/stderr"),
        log_level
    );

    match run_server() {
        Ok(()) => {
            tracing::info!("Language server shutdown gracefully");
            0
        }
        Err(e) => {
            tracing::error!("Language server failed with error: {}", e);
            1
        }
    }
}

fn setup_logging(log_file: Option<&str>, log_level_arg: Option<&str>) {
    let log_to_file = log_file.is_some();

    let level = match parse_log_level(log_level_arg) {
        Some(lvl) => lvl,
        None => {
            if log_to_file {
                LevelFilter::DEBUG // Default level when logging to file
            } else {
                LevelFilter::INFO // Default level when logging to stderr
            }
        }
    };

    let file = match log_file {
        Some(path) => match fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => {
                eprintln!("Logging to file: {path}");
                Some(f)
            }
            Err(e) => {
                eprintln!("Failed to open log file '{path}': {e}. Falling back to stderr.");
                None
            }
        },
        None => None,
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

fn parse_log_level(level_str: Option<&str>) -> Option<LevelFilter> {
    let level_str = level_str?;

    if level_str.is_empty() {
        return None;
    }

    Some(LevelFilter::from_str(level_str).unwrap_or_else(|_| {
        eprintln!(
            "Invalid log level '{level_str}'. Using 'info' as default. Valid levels: trace, debug, info, warn, error, off",
        );
        LevelFilter::INFO
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_log_level_valid_lowercase() {
        assert_eq!(parse_log_level(Some("trace")), Some(LevelFilter::TRACE));
        assert_eq!(parse_log_level(Some("debug")), Some(LevelFilter::DEBUG));
        assert_eq!(parse_log_level(Some("info")), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level(Some("warn")), Some(LevelFilter::WARN));
        assert_eq!(parse_log_level(Some("error")), Some(LevelFilter::ERROR));
        assert_eq!(parse_log_level(Some("off")), Some(LevelFilter::OFF));
    }

    #[test]
    fn test_parse_log_level_valid_uppercase() {
        assert_eq!(parse_log_level(Some("TRACE")), Some(LevelFilter::TRACE));
        assert_eq!(parse_log_level(Some("DEBUG")), Some(LevelFilter::DEBUG));
        assert_eq!(parse_log_level(Some("INFO")), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level(Some("WARN")), Some(LevelFilter::WARN));
        assert_eq!(parse_log_level(Some("ERROR")), Some(LevelFilter::ERROR));
        assert_eq!(parse_log_level(Some("OFF")), Some(LevelFilter::OFF));
    }

    #[test]
    fn test_parse_log_level_valid_mixed_case() {
        assert_eq!(parse_log_level(Some("Trace")), Some(LevelFilter::TRACE));
        assert_eq!(parse_log_level(Some("Debug")), Some(LevelFilter::DEBUG));
        assert_eq!(parse_log_level(Some("Info")), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level(Some("Warn")), Some(LevelFilter::WARN));
        assert_eq!(parse_log_level(Some("Error")), Some(LevelFilter::ERROR));
        assert_eq!(parse_log_level(Some("Off")), Some(LevelFilter::OFF));
    }

    #[test]
    fn test_parse_log_level_invalid_defaults_to_info() {
        assert_eq!(parse_log_level(Some("invalid")), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level(Some("unknown")), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level(Some("")), None);
        assert_eq!(parse_log_level(Some("123")), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level(None), None);
    }
}
