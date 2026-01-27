//! Configuration management for the MD Local QC Agent.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::types::Vendor;

pub mod paths;

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the config file (set after loading)
    #[serde(skip)]
    pub path: PathBuf,

    /// Agent configuration
    #[serde(default)]
    pub agent: AgentConfig,

    /// Cloud connection configuration
    #[serde(default)]
    pub cloud: CloudConfig,

    /// Skyline configuration
    #[serde(default)]
    pub skyline: SkylineConfig,

    /// File watcher configuration
    #[serde(default)]
    pub watcher: WatcherConfig,

    /// Spool configuration
    #[serde(default)]
    pub spool: SpoolConfig,

    /// Configured instruments
    #[serde(default)]
    pub instruments: Vec<InstrumentConfig>,
}

impl Config {
    /// Load configuration from the default path or environment.
    pub fn load() -> Result<Self> {
        let config_path = paths::config_file();
        Self::load_from(&config_path)
    }

    /// Load configuration from a specific path.
    pub fn load_from(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let mut config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        config.path = path.clone();

        // Validate
        config.validate()?;

        Ok(config)
    }

    /// Validate the configuration.
    fn validate(&self) -> Result<()> {
        // Validate instruments
        for (i, inst) in self.instruments.iter().enumerate() {
            if inst.id.is_empty() {
                anyhow::bail!("Instrument {} has empty id", i);
            }
            if inst.watch_path.is_empty() {
                anyhow::bail!("Instrument '{}' has empty watch_path", inst.id);
            }
            if inst.template.is_empty() {
                anyhow::bail!("Instrument '{}' has empty template", inst.id);
            }
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            agent: AgentConfig::default(),
            cloud: CloudConfig::default(),
            skyline: SkylineConfig::default(),
            watcher: WatcherConfig::default(),
            spool: SpoolConfig::default(),
            instruments: Vec::new(),
        }
    }
}

/// Agent-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique identifier for this agent instance ("auto" = generate from hardware ID)
    #[serde(default = "default_agent_id")]
    pub agent_id: String,

    /// Log level
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Enable Windows toast notifications for critical errors
    #[serde(default)]
    pub enable_toast_notifications: bool,
}

fn default_agent_id() -> String {
    "auto".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            agent_id: default_agent_id(),
            log_level: default_log_level(),
            enable_toast_notifications: false,
        }
    }
}

/// Cloud connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    /// Cloud endpoint URL
    #[serde(default = "default_endpoint")]
    pub endpoint: String,

    /// Certificate thumbprint (from Windows cert store)
    pub certificate_thumbprint: Option<String>,

    /// Proxy URL (optional)
    pub proxy: Option<String>,
}

fn default_endpoint() -> String {
    "https://qc-ingest.massdynamics.com/v1/".to_string()
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            endpoint: default_endpoint(),
            certificate_thumbprint: None,
            proxy: None,
        }
    }
}

/// Skyline configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkylineConfig {
    /// Path to SkylineCmd.exe (optional, will auto-discover)
    pub path: Option<String>,

    /// Extraction timeout in seconds
    #[serde(default = "default_skyline_timeout")]
    pub timeout_seconds: u64,

    /// Process priority
    #[serde(default = "default_process_priority")]
    pub process_priority: String,
}

fn default_skyline_timeout() -> u64 {
    300
}

fn default_process_priority() -> String {
    "below_normal".to_string()
}

impl Default for SkylineConfig {
    fn default() -> Self {
        Self {
            path: None,
            timeout_seconds: default_skyline_timeout(),
            process_priority: default_process_priority(),
        }
    }
}

/// File watcher configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Enable filesystem event watching
    #[serde(default = "default_true")]
    pub use_filesystem_events: bool,

    /// Fallback scan interval in seconds
    #[serde(default = "default_scan_interval")]
    pub scan_interval_seconds: u64,

    /// Stability window before processing in seconds
    #[serde(default = "default_stability_window")]
    pub stability_window_seconds: u64,

    /// Maximum stabilization wait in seconds
    #[serde(default = "default_stabilization_timeout")]
    pub stabilization_timeout_seconds: u64,
}

fn default_true() -> bool {
    true
}

fn default_scan_interval() -> u64 {
    30
}

fn default_stability_window() -> u64 {
    60
}

fn default_stabilization_timeout() -> u64 {
    600
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            use_filesystem_events: true,
            scan_interval_seconds: default_scan_interval(),
            stability_window_seconds: default_stability_window(),
            stabilization_timeout_seconds: default_stabilization_timeout(),
        }
    }
}

/// Spool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoolConfig {
    /// Maximum pending spool size in MB
    #[serde(default = "default_max_pending_mb")]
    pub max_pending_mb: u64,

    /// Maximum age of spooled items in days
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u64,

    /// Number of completed items to retain
    #[serde(default = "default_completed_retention")]
    pub completed_retention_count: usize,
}

fn default_max_pending_mb() -> u64 {
    1000
}

fn default_max_age_days() -> u64 {
    30
}

fn default_completed_retention() -> usize {
    10
}

impl Default for SpoolConfig {
    fn default() -> Self {
        Self {
            max_pending_mb: default_max_pending_mb(),
            max_age_days: default_max_age_days(),
            completed_retention_count: default_completed_retention(),
        }
    }
}

/// Instrument configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentConfig {
    /// Unique identifier for this instrument
    pub id: String,

    /// Vendor type
    pub vendor: Vendor,

    /// Path to watch for raw files
    pub watch_path: String,

    /// File pattern (glob)
    #[serde(default = "default_file_pattern")]
    pub file_pattern: String,

    /// Skyline template filename
    pub template: String,

    /// Vendor-specific watcher overrides
    #[serde(default)]
    pub watcher_overrides: Option<WatcherConfig>,
}

fn default_file_pattern() -> String {
    "*".to_string()
}
