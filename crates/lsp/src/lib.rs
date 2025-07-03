mod beancount_data;
mod capabilities;
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
    let (connection, io_threads) = lsp_server::Connection::stdio();

    //wait for client to connection
    let (request_id, initialize_params) = connection.initialize_start()?;
    tracing::info!("initialize params: {}", initialize_params);

    let initialize_params = serde_json::from_value::<InitializeParams>(initialize_params)?;

    let server_capabilities = capabilities::server_capabilities();

    let initialize_result = lsp_types::InitializeResult {
        capabilities: server_capabilities,
        server_info: Some(lsp_types::ServerInfo {
            name: String::from("beancount-language-server"),
            version: Some(String::from(env!("CARGO_PKG_VERSION"))),
        }),
    };

    let initialize_result = serde_json::to_value(initialize_result).unwrap();

    connection.initialize_finish(request_id, initialize_result)?;

    if let Some(client_info) = initialize_params.client_info {
        tracing::info!(
            "client '{}' {}",
            client_info.name,
            client_info.version.unwrap_or_default()
        );
    }

    let config = {
        let root_file = match initialize_params
            .workspace_folders
            .and_then(|wfs| wfs.first().and_then(|f| f.uri.to_file_path().ok()))
        {
            Some(it) => it,
            None => std::env::current_dir()?,
        };
        let mut config = Config::new(root_file);
        if let Some(json) = initialize_params.initialization_options {
            config.update(json).unwrap();
        }
        config
    };

    main_loop(connection, config)?;

    io_threads.join()?;

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
