use anyhow::{Context, Result};
use beancount_language_server::config::{Config, FormattingConfig, FormattingOptions};
use beancount_language_server::document::Document;
use beancount_language_server::providers::formatting::formatting;
use beancount_language_server::server::LspServerStateSnapshot;
use clap::builder::ValueHint;
use clap::{Arg, ArgAction, Command, arg, value_parser};
use lsp_types::{
    DocumentFormattingParams, FormattingOptions as LspFormattingOptions, TextDocumentIdentifier,
    TextEdit, Uri, WorkDoneProgressParams,
};
use ropey::Rope;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{EnvFilter, filter::Directive};
use tree_sitter_beancount::tree_sitter::Parser;
use url::Url;

fn main() {
    let matches = Command::new("beancount-language-server")
        .about("Beancount language server and utilities")
        .args(&[
            arg!(--stdio "specifies to use stdio to communicate with lsp"),
            arg!(--log [LOG_LEVEL] "write log to file with optional level (trace, debug, info, warn, error)"),
            arg!(version: -v --version),
        ])
        .subcommand(
            Command::new("format")
                .about("Format beancount files in place")
                .arg(
                    Arg::new("files")
                        .value_name("FILE")
                        .value_hint(ValueHint::FilePath)
                        .value_parser(value_parser!(PathBuf))
                        .num_args(1..)
                        .required(true)
                        .help("Beancount files or directories to format (searches recursively)"),
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .short('c')
                        .value_name("FILE")
                        .value_hint(ValueHint::FilePath)
                        .value_parser(value_parser!(PathBuf))
                        .help("Path to JSON or TOML file containing formatting configuration"),
                )
                .arg(
                    Arg::new("check")
                        .long("check")
                        .action(ArgAction::SetTrue)
                        .help("Check if files are formatted; exits non-zero if changes would be made"),
                )
                .arg(
                    Arg::new("verbose")
                        .long("verbose")
                        .action(ArgAction::SetTrue)
                        .help("Show all processed files, including unchanged ones"),
                ),
        )
        .get_matches();

    if let Some(format_matches) = matches.subcommand_matches("format") {
        if let Err(error) = handle_format_subcommand(format_matches) {
            eprintln!("Formatting failed: {error:?}");
            std::process::exit(1);
        }
        return;
    }

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

fn handle_format_subcommand(matches: &clap::ArgMatches) -> Result<()> {
    let files: Vec<PathBuf> = matches
        .get_many::<PathBuf>("files")
        .expect("files are required by clap")
        .cloned()
        .collect();

    let check_only = matches.get_flag("check");
    let verbose = matches.get_flag("verbose");

    let formatting_config = if let Some(config_path) = matches.get_one::<PathBuf>("config") {
        load_formatting_config(config_path)?
    } else {
        FormattingConfig::default()
    };

    let expanded_files = expand_paths(&files)?;

    if expanded_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No .bean or .beancount files found to format"
        ));
    }

    format_files(&expanded_files, formatting_config, check_only, verbose)
}

#[derive(Debug, Deserialize)]
struct CliConfigFile {
    formatting: Option<FormattingOptions>,
}

fn load_formatting_config(path: &Path) -> Result<FormattingConfig> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    let parsed: CliConfigFile = match ext.as_deref() {
        Some("json") => serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse JSON config file {}", path.display()))?,
        Some("toml") => toml::from_str(&contents)
            .with_context(|| format!("Failed to parse TOML config file {}", path.display()))?,
        Some(other) => {
            return Err(anyhow::anyhow!(
                "Unsupported config extension '{}'. Use .json or .toml",
                other
            ));
        }
        None => {
            return Err(anyhow::anyhow!(
                "Config file {} has no extension; expected .json or .toml",
                path.display()
            ));
        }
    };

    Ok(parsed.formatting.unwrap_or_default().into())
}

fn expand_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for path in paths {
        let meta =
            fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;

        if meta.is_dir() {
            collect_dir(path, &mut results, &mut seen)?;
        } else if meta.is_file() && is_beancount_file(path) && seen.insert(path.to_path_buf()) {
            results.push(path.to_path_buf());
        }
    }

    Ok(results)
}

fn collect_dir(
    dir: &Path,
    results: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) -> Result<()> {
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)
            .with_context(|| format!("Failed to read directory {}", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let meta = entry
                .metadata()
                .with_context(|| format!("Failed to stat {}", path.display()))?;

            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && is_beancount_file(&path) && seen.insert(path.clone()) {
                results.push(path);
            }
        }
    }

    Ok(())
}

fn is_beancount_file(path: &Path) -> bool {
    path.ends_with(".bean") || path.ends_with(".beancount")
}

fn format_files(
    files: &[PathBuf],
    formatting_config: FormattingConfig,
    check_only: bool,
    verbose: bool,
) -> Result<()> {
    let mut needs_change = false;

    for path in files {
        match format_single_file(path, &formatting_config, check_only)? {
            FormatOutcome::Updated => {
                needs_change = true;
                if check_only {
                    println!("Would format {}", path.display());
                } else {
                    println!("Formatted {}", path.display());
                }
            }
            FormatOutcome::Unchanged => {
                if verbose {
                    println!("Unchanged {}", path.display());
                }
            }
        }
    }

    if needs_change {
        std::process::exit(1);
    }

    Ok(())
}

enum FormatOutcome {
    Updated,
    Unchanged,
}

fn format_single_file(
    path: &Path,
    formatting_config: &FormattingConfig,
    check_only: bool,
) -> Result<FormatOutcome> {
    let original = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file {}", path.display()))?;
    let formatted = format_content(path, &original, formatting_config)?;

    if formatted != original {
        if !check_only {
            fs::write(path, formatted)?;
        }
        Ok(FormatOutcome::Updated)
    } else {
        Ok(FormatOutcome::Unchanged)
    }
}

fn format_content(
    path: &Path,
    content: &str,
    formatting_config: &FormattingConfig,
) -> Result<String> {
    let rope_content = Rope::from_str(content);

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_beancount::language())
        .context("Failed to load beancount grammar")?;
    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", path.display()))?;

    let mut forest = HashMap::new();
    forest.insert(path.to_path_buf(), Arc::new(tree));

    let mut open_docs = HashMap::new();
    open_docs.insert(
        path.to_path_buf(),
        Document {
            content: rope_content.clone(),
        },
    );

    let mut config = Config::new(
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    );
    config.formatting = formatting_config.clone();

    let snapshot = LspServerStateSnapshot {
        beancount_data: HashMap::new(),
        config,
        forest,
        open_docs,
    };

    let url = Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("Invalid path for URI: {}", path.display()))?;
    let uri =
        Uri::from_str(url.as_str()).map_err(|_| anyhow::anyhow!("Invalid URL for URI: {}", url))?;
    let params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri },
        options: LspFormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            properties: HashMap::new(),
            trim_trailing_whitespace: Some(false),
            insert_final_newline: Some(false),
            trim_final_newlines: Some(false),
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
    };

    let edits = formatting(snapshot, params)?;
    match edits {
        Some(edits) => Ok(apply_text_edits(content, &edits)),
        None => Ok(content.to_string()),
    }
}

fn apply_text_edits(content: &str, edits: &[TextEdit]) -> String {
    let mut result = Rope::from_str(content);
    let mut sorted_edits = edits.to_vec();

    // Apply from the bottom of the file to avoid shifting ranges.
    sorted_edits.sort_by(|a, b| {
        let line_cmp = b.range.start.line.cmp(&a.range.start.line);
        if line_cmp == std::cmp::Ordering::Equal {
            b.range.start.character.cmp(&a.range.start.character)
        } else {
            line_cmp
        }
    });

    for edit in sorted_edits {
        let start_line = edit.range.start.line as usize;
        let start_char = edit.range.start.character as usize;
        let end_line = edit.range.end.line as usize;
        let end_char = edit.range.end.character as usize;

        let start_char_idx = result.line_to_char(start_line) + start_char;
        let end_char_idx = result.line_to_char(end_line) + end_char;

        if start_char_idx < end_char_idx {
            result.remove(start_char_idx..end_char_idx);
        }

        if !edit.new_text.is_empty() {
            result.insert(start_char_idx, &edit.new_text);
        }
    }

    result.to_string()
}

fn setup_logging(log_to_file: bool, log_level_arg: Option<&String>) {
    let level = match log_level_arg {
        Some(level_str) => parse_log_level(level_str),
        None => {
            if log_to_file {
                LevelFilter::DEBUG // Default level when logging to file
            } else {
                LevelFilter::INFO // Default level when logging to stderr
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
            eprintln!(
                "Invalid log level '{level_str}'. Using 'info' as default. Valid levels: trace, debug, info, warn, error, off"
            );
            LevelFilter::INFO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_log_level_valid_lowercase() {
        assert_eq!(parse_log_level("trace"), LevelFilter::TRACE);
        assert_eq!(parse_log_level("debug"), LevelFilter::DEBUG);
        assert_eq!(parse_log_level("info"), LevelFilter::INFO);
        assert_eq!(parse_log_level("warn"), LevelFilter::WARN);
        assert_eq!(parse_log_level("error"), LevelFilter::ERROR);
        assert_eq!(parse_log_level("off"), LevelFilter::OFF);
    }

    #[test]
    fn test_parse_log_level_valid_uppercase() {
        assert_eq!(parse_log_level("TRACE"), LevelFilter::TRACE);
        assert_eq!(parse_log_level("DEBUG"), LevelFilter::DEBUG);
        assert_eq!(parse_log_level("INFO"), LevelFilter::INFO);
        assert_eq!(parse_log_level("WARN"), LevelFilter::WARN);
        assert_eq!(parse_log_level("ERROR"), LevelFilter::ERROR);
        assert_eq!(parse_log_level("OFF"), LevelFilter::OFF);
    }

    #[test]
    fn test_parse_log_level_valid_mixed_case() {
        assert_eq!(parse_log_level("Trace"), LevelFilter::TRACE);
        assert_eq!(parse_log_level("Debug"), LevelFilter::DEBUG);
        assert_eq!(parse_log_level("Info"), LevelFilter::INFO);
        assert_eq!(parse_log_level("Warn"), LevelFilter::WARN);
        assert_eq!(parse_log_level("Error"), LevelFilter::ERROR);
        assert_eq!(parse_log_level("Off"), LevelFilter::OFF);
    }

    #[test]
    fn test_parse_log_level_invalid_defaults_to_info() {
        assert_eq!(parse_log_level("invalid"), LevelFilter::INFO);
        assert_eq!(parse_log_level("unknown"), LevelFilter::INFO);
        assert_eq!(parse_log_level(""), LevelFilter::INFO);
        assert_eq!(parse_log_level("123"), LevelFilter::INFO);
    }
}
