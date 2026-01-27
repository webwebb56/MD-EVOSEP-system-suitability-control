//! Error types for the MD Local QC Agent.

#![allow(dead_code)]

use thiserror::Error;

/// Main error type for the agent.
#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Watcher error: {0}")]
    Watcher(#[from] WatcherError),

    #[error("Classification error: {0}")]
    Classification(#[from] ClassificationError),

    #[error("Extraction error: {0}")]
    Extraction(#[from] ExtractionError),

    #[error("Spool error: {0}")]
    Spool(#[from] SpoolError),

    #[error("Upload error: {0}")]
    Upload(#[from] UploadError),

    #[error("Baseline error: {0}")]
    Baseline(#[from] BaselineError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Service error: {0}")]
    Service(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(String),

    #[error("Invalid config: {0}")]
    Invalid(String),

    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[derive(Error, Debug)]
pub enum WatcherError {
    #[error("Watch path does not exist: {0}")]
    PathNotFound(String),

    #[error("Watch path is not accessible: {0}")]
    PathNotAccessible(String),

    #[error("Filesystem notification error: {0}")]
    Notify(#[from] notify::Error),

    #[error("Finalization timeout for: {0}")]
    FinalizationTimeout(String),
}

#[derive(Error, Debug)]
pub enum ClassificationError {
    #[error("Unable to parse filename: {0}")]
    FilenameParse(String),

    #[error("Unknown control type: {0}")]
    UnknownControlType(String),

    #[error("Invalid well position: {0}")]
    InvalidWellPosition(String),
}

#[derive(Error, Debug)]
pub enum ExtractionError {
    #[error("Skyline not found at: {0}")]
    SkylineNotFound(String),

    #[error("Skyline execution failed: {0}")]
    SkylineExecution(String),

    #[error("Skyline timeout after {0} seconds")]
    SkylineTimeout(u64),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Vendor reader not available for: {0}")]
    VendorReaderMissing(String),

    #[error("Report parse error: {0}")]
    ReportParse(String),
}

#[derive(Error, Debug)]
pub enum SpoolError {
    #[error("Spool directory not writable: {0}")]
    NotWritable(String),

    #[error("Spool full: {0} MB used of {1} MB limit")]
    Full(u64, u64),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("File operation failed: {0}")]
    FileOperation(String),
}

#[derive(Error, Debug)]
pub enum UploadError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },

    #[error("Certificate error: {0}")]
    Certificate(String),

    #[error("Retry exhausted after {0} attempts")]
    RetryExhausted(u32),
}

#[derive(Error, Debug)]
pub enum BaselineError {
    #[error("No active baseline for instrument: {0}")]
    NoActiveBaseline(String),

    #[error("Baseline validation failed: {0}")]
    ValidationFailed(String),

    #[error("Cannot reset baseline: {0}")]
    ResetFailed(String),
}

/// Result type alias for agent operations.
pub type AgentResult<T> = Result<T, AgentError>;
