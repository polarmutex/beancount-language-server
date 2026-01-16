mod beancount_data;
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

    let initialize_result = serde_json::to_value(initialize_result).unwrap();

    connection.initialize_finish(request_id, initialize_result)?;
    tracing::info!("Initialization completed successfully");

    tracing::debug!("Starting main loop");
    main_loop(connection, config)?;

    tracing::debug!("Waiting for IO threads to complete");
    io_threads.join()?;
    tracing::info!("Language server stopped");

    Ok(())
}

pub fn main_loop(connection: Connection, config: Config) -> Result<()> {
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
