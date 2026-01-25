use clap::Parser;
use std::fs;
use std::io;
use std::str::FromStr;
use std::sync::Arc;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{EnvFilter, filter::Directive};

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

fn main() {
    let cli = Cli::parse_from(std::env::args_os());

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
