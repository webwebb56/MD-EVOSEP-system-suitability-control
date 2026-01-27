//! Core types for the MD Local QC Agent.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Control types aligned with EvoSep kit controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlType {
    /// System Suitability Control - baseline reference
    Ssc0,
    /// 500ng lysate full workflow control
    QcA,
    /// 50ng digest LCMS + loading control
    QcB,
    /// Normal sample (ignored by default)
    Sample,
    /// Blank injection
    Blank,
}

impl ControlType {
    /// Returns true if this is a QC control type (not SAMPLE).
    pub fn is_qc(&self) -> bool {
        !matches!(self, ControlType::Sample)
    }

    /// Parse control type from string token.
    pub fn from_token(token: &str) -> Option<Self> {
        let normalized = token.to_uppercase().replace(['-', '_'], "");
        match normalized.as_str() {
            "SSC0" | "SSC" => Some(ControlType::Ssc0),
            "QCA" | "QA" => Some(ControlType::QcA),
            "QCB" | "QB" => Some(ControlType::QcB),
            "BLANK" | "BLK" => Some(ControlType::Blank),
            "SAMPLE" | "SPL" => Some(ControlType::Sample),
            _ => None,
        }
    }
}

impl std::fmt::Display for ControlType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlType::Ssc0 => write!(f, "SSC0"),
            ControlType::QcA => write!(f, "QC_A"),
            ControlType::QcB => write!(f, "QC_B"),
            ControlType::Sample => write!(f, "SAMPLE"),
            ControlType::Blank => write!(f, "BLANK"),
        }
    }
}

/// Supported MS vendors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Thermo,
    Bruker,
    Sciex,
    Waters,
    Agilent,
}

impl Vendor {
    /// Returns the file extension(s) for this vendor.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Vendor::Thermo => &["raw"],
            Vendor::Bruker => &["d"],
            Vendor::Sciex => &["wiff", "wiff2"],
            Vendor::Waters => &["raw"], // Directory
            Vendor::Agilent => &["d"],  // Directory
        }
    }

    /// Returns true if this vendor uses directory-based files.
    pub fn is_directory_format(&self) -> bool {
        matches!(self, Vendor::Bruker | Vendor::Waters | Vendor::Agilent)
    }
}

impl std::fmt::Display for Vendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Vendor::Thermo => write!(f, "thermo"),
            Vendor::Bruker => write!(f, "bruker"),
            Vendor::Sciex => write!(f, "sciex"),
            Vendor::Waters => write!(f, "waters"),
            Vendor::Agilent => write!(f, "agilent"),
        }
    }
}

impl std::str::FromStr for Vendor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "thermo" => Ok(Vendor::Thermo),
            "bruker" => Ok(Vendor::Bruker),
            "sciex" => Ok(Vendor::Sciex),
            "waters" => Ok(Vendor::Waters),
            "agilent" => Ok(Vendor::Agilent),
            _ => Err(format!("Unknown vendor: {}", s)),
        }
    }
}

/// Well position on a plate (A1-H12).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WellPosition {
    pub row: char,  // A-H
    pub column: u8, // 1-12
}

impl WellPosition {
    pub fn new(row: char, column: u8) -> Option<Self> {
        let row = row.to_ascii_uppercase();
        if ('A'..='H').contains(&row) && (1..=12).contains(&column) {
            Some(Self { row, column })
        } else {
            None
        }
    }

    /// Parse from string like "A1", "A3", "E5".
    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.trim().to_uppercase();
        if s.len() < 2 || s.len() > 3 {
            return None;
        }

        let row = s.chars().next()?;
        let column: u8 = s[1..].parse().ok()?;
        Self::new(row, column)
    }
}

impl std::fmt::Display for WellPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.row, self.column)
    }
}

/// Classification confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClassificationConfidence {
    High,
    Medium,
    Low,
}

/// Source of classification decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClassificationSource {
    Filename,
    Metadata,
    Position,
    Default,
}

/// Result of classifying a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunClassification {
    pub control_type: ControlType,
    pub well_position: Option<WellPosition>,
    pub instrument_id: String,
    pub plate_id: Option<String>,
    pub confidence: ClassificationConfidence,
    pub source: ClassificationSource,
}

/// State of a file in the finalization process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationState {
    Detected,
    Stabilizing,
    Ready,
    Processing,
    Done,
    Failed,
}

/// A detected raw file being tracked.
#[derive(Debug, Clone)]
pub struct TrackedFile {
    pub path: PathBuf,
    pub state: FinalizationState,
    pub first_seen: DateTime<Utc>,
    pub last_size: u64,
    pub last_modified: DateTime<Utc>,
    pub stable_since: Option<DateTime<Utc>>,
    pub vendor: Vendor,
}

/// Metrics for a single target/peptide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetMetrics {
    pub target_id: String,
    pub peptide_sequence: Option<String>,
    pub precursor_mz: f64,
    pub retention_time: f64,
    pub rt_expected: Option<f64>,
    pub rt_delta: Option<f64>,
    pub peak_area: f64,
    pub peak_height: f64,
    pub peak_width_fwhm: Option<f64>,
    pub peak_symmetry: Option<f64>,
    pub mass_error_ppm: Option<f64>,
    pub isotope_dot_product: Option<f64>,
    pub detected: bool,
}

/// Run-level aggregate metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    pub targets_found: u32,
    pub targets_expected: u32,
    pub target_recovery_pct: f64,
    pub median_rt_shift: Option<f64>,
    pub median_mass_error_ppm: Option<f64>,
    pub chromatography_score: Option<f64>,
}

/// Extraction result from Skyline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub run_id: Uuid,
    pub raw_file_path: PathBuf,
    pub raw_file_name: String,
    pub raw_file_hash: String,
    pub extraction_time_ms: u64,
    pub backend: String,
    pub backend_version: String,
    pub template_name: String,
    pub template_hash: String,
    pub target_metrics: Vec<TargetMetrics>,
    pub run_metrics: RunMetrics,
}

/// Complete payload for upload to MD cloud.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcPayload {
    pub schema_version: String,
    pub payload_id: Uuid,
    pub correlation_id: String,
    pub agent_id: String,
    pub agent_version: String,
    pub timestamp: DateTime<Utc>,

    pub run: RunInfo,
    pub extraction: ExtractionInfo,
    pub baseline_context: Option<BaselineContext>,
    pub target_metrics: Vec<TargetMetrics>,
    pub run_metrics: RunMetrics,
    pub comparison_metrics: Option<ComparisonMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInfo {
    pub run_id: Uuid,
    pub raw_file_name: String,
    pub raw_file_hash: String,
    pub acquisition_time: Option<DateTime<Utc>>,
    pub instrument_id: String,
    pub vendor: Vendor,
    pub control_type: ControlType,
    pub well_position: Option<String>,
    pub plate_id: Option<String>,
    pub classification_confidence: ClassificationConfidence,
    pub classification_source: ClassificationSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionInfo {
    pub backend: String,
    pub backend_version: String,
    pub template_name: String,
    pub template_hash: String,
    pub extraction_time_ms: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineContext {
    pub baseline_id: String,
    pub baseline_established: DateTime<Utc>,
    pub baseline_template_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonMetrics {
    pub vs_baseline: BaselineComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineComparison {
    pub rt_shift_mean: f64,
    pub rt_shift_std: f64,
    pub area_ratio_mean: f64,
    pub area_ratio_std: f64,
    pub outlier_targets: Vec<String>,
}

/// Baseline state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BaselineState {
    Candidate,
    Validating,
    Active,
    Archived,
    Rejected,
    Failed,
}

/// Baseline record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub baseline_id: String,
    pub instrument_id: String,
    pub method_id: Option<String>,
    pub template_hash: String,
    pub kit_install_id: Option<String>,
    pub state: BaselineState,
    pub established: DateTime<Utc>,
    pub run_metrics: RunMetrics,
    pub target_metrics: Vec<TargetMetrics>,
}
