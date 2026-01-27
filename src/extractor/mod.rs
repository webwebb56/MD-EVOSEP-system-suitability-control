//! Extraction backend using Skyline.
//!
//! Invokes SkylineCmd.exe to extract QC metrics from raw files.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::{InstrumentConfig, SkylineConfig};
use crate::error::ExtractionError;
use crate::types::{ExtractionResult, RunClassification, RunMetrics, TargetMetrics};

pub mod skyline;

/// Extractor for QC metrics.
pub struct Extractor {
    config: SkylineConfig,
    skyline_path: Option<PathBuf>,
}

impl Extractor {
    pub fn new(config: &SkylineConfig) -> Result<Self> {
        // Discover Skyline path
        let skyline_path = config
            .path
            .as_ref()
            .map(PathBuf::from)
            .or_else(skyline::discover_skyline);

        if skyline_path.is_none() {
            warn!("Skyline not found during extractor initialization");
        }

        Ok(Self {
            config: config.clone(),
            skyline_path,
        })
    }

    /// Extract QC metrics from a raw file.
    pub async fn extract(
        &self,
        raw_path: &Path,
        instrument: &InstrumentConfig,
        classification: &RunClassification,
    ) -> Result<ExtractionResult, ExtractionError> {
        let skyline_path = self
            .skyline_path
            .as_ref()
            .ok_or_else(|| ExtractionError::SkylineNotFound("not configured".to_string()))?;

        if !skyline_path.exists() {
            return Err(ExtractionError::SkylineNotFound(
                skyline_path.display().to_string(),
            ));
        }

        // Get template path
        let template_dir = crate::config::paths::template_dir();
        let template_path = template_dir.join(&instrument.template);

        if !template_path.exists() {
            return Err(ExtractionError::TemplateNotFound(
                template_path.display().to_string(),
            ));
        }

        // Calculate template hash
        let template_hash = skyline::hash_template(&template_path)
            .map_err(|e| ExtractionError::TemplateNotFound(e.to_string()))?;

        // Create temporary output file for the report
        let run_id = Uuid::new_v4();
        let work_dir = crate::config::paths::spool_dir().join("work");
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| ExtractionError::SkylineExecution(e.to_string()))?;

        let report_path = work_dir.join(format!("{}_report.csv", run_id));

        info!(
            raw_file = %raw_path.display(),
            template = %instrument.template,
            "Starting Skyline extraction"
        );

        let start = Instant::now();

        // Build Skyline command
        let mut cmd = Command::new(skyline_path);
        cmd.arg("--in")
            .arg(&template_path)
            .arg("--import-file")
            .arg(raw_path)
            .arg("--report-name")
            .arg("MD_QC_Report")
            .arg("--report-file")
            .arg(&report_path)
            .arg("--report-format")
            .arg("csv")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set process priority on Windows
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // BELOW_NORMAL_PRIORITY_CLASS = 0x00004000
            if self.config.process_priority == "below_normal" {
                cmd.creation_flags(0x00004000);
            }
        }

        debug!(command = ?cmd, "Executing Skyline");

        // Run with timeout
        let timeout = tokio::time::Duration::from_secs(self.config.timeout_seconds);
        let result = tokio::time::timeout(timeout, cmd.output()).await;

        let output = match result {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(ExtractionError::SkylineExecution(e.to_string()));
            }
            Err(_) => {
                return Err(ExtractionError::SkylineTimeout(self.config.timeout_seconds));
            }
        };

        let extraction_time_ms = start.elapsed().as_millis() as u64;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(stderr = %stderr, "Skyline extraction failed");
            return Err(ExtractionError::SkylineExecution(stderr.to_string()));
        }

        // Parse the report
        let target_metrics = self.parse_report(&report_path)?;

        // Calculate run metrics
        let run_metrics = self.calculate_run_metrics(&target_metrics);

        // Get Skyline version
        let skyline_version = skyline::get_version(skyline_path).unwrap_or_else(|_| "unknown".to_string());

        // Calculate raw file hash
        let raw_file_hash = calculate_file_hash(raw_path)
            .unwrap_or_else(|_| "error".to_string());

        // Clean up work file
        let _ = std::fs::remove_file(&report_path);

        info!(
            raw_file = %raw_path.display(),
            targets_found = run_metrics.targets_found,
            extraction_time_ms = extraction_time_ms,
            "Extraction complete"
        );

        Ok(ExtractionResult {
            run_id,
            raw_file_path: raw_path.to_path_buf(),
            raw_file_name: raw_path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("unknown")
                .to_string(),
            raw_file_hash,
            extraction_time_ms,
            backend: "skyline".to_string(),
            backend_version: skyline_version,
            template_name: instrument.template.clone(),
            template_hash,
            target_metrics,
            run_metrics,
        })
    }

    /// Parse the Skyline report CSV.
    fn parse_report(&self, report_path: &Path) -> Result<Vec<TargetMetrics>, ExtractionError> {
        let file = std::fs::File::open(report_path)
            .map_err(|e| ExtractionError::ReportParse(e.to_string()))?;

        let mut reader = csv::Reader::from_reader(file);
        let mut metrics = Vec::new();

        for result in reader.records() {
            let record = result.map_err(|e| ExtractionError::ReportParse(e.to_string()))?;

            // Parse CSV fields - adjust column indices based on actual Skyline report format
            let target_metrics = TargetMetrics {
                target_id: record.get(0).unwrap_or("").to_string(),
                peptide_sequence: record.get(1).map(|s| s.to_string()),
                precursor_mz: record.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                retention_time: record.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                rt_expected: record.get(4).and_then(|s| s.parse().ok()),
                rt_delta: record.get(5).and_then(|s| s.parse().ok()),
                peak_area: record.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                peak_height: record.get(7).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                peak_width_fwhm: record.get(8).and_then(|s| s.parse().ok()),
                peak_symmetry: record.get(9).and_then(|s| s.parse().ok()),
                mass_error_ppm: record.get(10).and_then(|s| s.parse().ok()),
                isotope_dot_product: record.get(11).and_then(|s| s.parse().ok()),
                detected: record.get(6).and_then(|s| s.parse::<f64>().ok()).map(|a| a > 0.0).unwrap_or(false),
            };

            metrics.push(target_metrics);
        }

        Ok(metrics)
    }

    /// Calculate run-level metrics from target metrics.
    fn calculate_run_metrics(&self, targets: &[TargetMetrics]) -> RunMetrics {
        let targets_found = targets.iter().filter(|t| t.detected).count() as u32;
        let targets_expected = targets.len() as u32;

        let target_recovery_pct = if targets_expected > 0 {
            (targets_found as f64 / targets_expected as f64) * 100.0
        } else {
            0.0
        };

        // Calculate median RT shift
        let mut rt_deltas: Vec<f64> = targets
            .iter()
            .filter_map(|t| t.rt_delta)
            .collect();
        rt_deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_rt_shift = if !rt_deltas.is_empty() {
            let mid = rt_deltas.len() / 2;
            if rt_deltas.len() % 2 == 0 {
                Some((rt_deltas[mid - 1] + rt_deltas[mid]) / 2.0)
            } else {
                Some(rt_deltas[mid])
            }
        } else {
            None
        };

        // Calculate median mass error
        let mut mass_errors: Vec<f64> = targets
            .iter()
            .filter_map(|t| t.mass_error_ppm)
            .collect();
        mass_errors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_mass_error_ppm = if !mass_errors.is_empty() {
            let mid = mass_errors.len() / 2;
            if mass_errors.len() % 2 == 0 {
                Some((mass_errors[mid - 1] + mass_errors[mid]) / 2.0)
            } else {
                Some(mass_errors[mid])
            }
        } else {
            None
        };

        RunMetrics {
            targets_found,
            targets_expected,
            target_recovery_pct,
            median_rt_shift,
            median_mass_error_ppm,
            chromatography_score: None, // Could be calculated from peak metrics
        }
    }
}

/// Calculate SHA-256 hash of a file or directory.
fn calculate_file_hash(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    if path.is_file() {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok(hex::encode(hasher.finalize()))
    } else if path.is_dir() {
        // For directories, hash a consistent representation
        // (e.g., concatenation of filenames and sizes)
        let mut hasher = Sha256::new();

        let mut entries: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let name = entry.file_name();
            hasher.update(name.to_string_lossy().as_bytes());

            if let Ok(meta) = entry.metadata() {
                hasher.update(&meta.len().to_le_bytes());
            }
        }

        Ok(hex::encode(hasher.finalize()))
    } else {
        anyhow::bail!("Path is neither file nor directory: {}", path.display())
    }
}
