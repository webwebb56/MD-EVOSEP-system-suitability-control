//! CLI command definitions and handlers.

use clap::{Parser, Subcommand, ValueEnum};

pub mod baseline;
pub mod classify;
pub mod config;
pub mod doctor;
pub mod run;
pub mod status;

/// MD Local QC Agent - System suitability monitoring for mass spectrometry.
#[derive(Parser, Debug)]
#[command(name = "mdqc")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Log level
    #[arg(long, default_value = "info", env = "MDQC_LOG_LEVEL")]
    pub log_level: LogLevel,

    /// Path to config file
    #[arg(long, env = "MDQC_CONFIG")]
    pub config_path: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the agent (normally called by service)
    Run {
        /// Run in foreground instead of as service
        #[arg(long, short)]
        foreground: bool,
    },

    /// Check system health and dependencies
    Doctor,

    /// Preview run classification without processing
    Classify {
        /// Path to raw file or directory
        path: String,
    },

    /// Show agent status and queue
    Status,

    /// Manage baselines
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },

    /// Configuration commands
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Run system tray icon (Windows only)
    Tray,

    /// Open configuration editor GUI
    Gui,

    /// Show version information
    Version,
}

#[derive(Subcommand, Debug)]
pub enum BaselineAction {
    /// List all baselines
    List {
        /// Filter by instrument ID
        #[arg(long)]
        instrument: Option<String>,
    },

    /// Show details of a specific baseline
    Show {
        /// Baseline ID
        baseline_id: String,
    },

    /// Reset (archive) current baseline for an instrument
    Reset {
        /// Instrument ID
        #[arg(long)]
        instrument: String,

        /// Skip confirmation prompt
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Validate configuration file
    Validate,

    /// Show current configuration
    Show,

    /// Show configuration file path
    Path,
}
