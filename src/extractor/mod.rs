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
        // Handle "auto" path - treat it as None to trigger auto-discovery
        let skyline_path = config
            .path
            .as_ref()
            .filter(|p| !p.eq_ignore_ascii_case("auto") && !p.is_empty())
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
        _classification: &RunClassification,
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

        // Get template path - use absolute path if provided, otherwise look in template dir
        let template_path = {
            let path = PathBuf::from(&instrument.template);
            if path.is_absolute() && path.exists() {
                path
            } else {
                // Try relative to template directory
                let template_dir = crate::config::paths::template_dir();
                template_dir.join(&instrument.template)
            }
        };

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
        // Note: Template must have a report named "MD_QC_Report" defined
        // SkylineCmd requires --name=value format for arguments
        let mut cmd = Command::new(skyline_path);
        cmd.current_dir(&work_dir) // Set working directory to spool/work
            .arg(format!("--in={}", template_path.display()))
            .arg(format!("--import-file={}", raw_path.display()))
            .arg("--report-name=MD_QC_Report")
            .arg("--report-invariant") // Use language-independent column names
            .arg(format!("--report-file={}", report_path.display()))
            .arg("--report-format=csv")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set process priority on Windows
        // Note: CREATE_NO_WINDOW (0x08000000) causes "os error 50" with Skyline/ClickOnce apps
        // so we only use priority class flags here
        #[cfg(windows)]
        {
            #[allow(unused_imports)]
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
            let stdout = String::from_utf8_lossy(&output.stdout);
            let exit_code = output.status.code().unwrap_or(-1);

            // Skyline often writes errors to stdout, not stderr
            let mut error_msg = if !stderr.is_empty() {
                stderr.to_string()
            } else if !stdout.is_empty() {
                stdout.to_string()
            } else {
                format!("Skyline exited with code {}", exit_code)
            };

            // Add helpful message if report is missing
            if error_msg.contains("does not exist") && error_msg.contains("report") {
                error_msg.push_str(
                    "\n\nHint: Your Skyline template needs a report named 'MD_QC_Report'. ",
                );
                error_msg.push_str("Open the template in Skyline, go to View > Document Grid > Reports > Edit Reports, ");
                error_msg.push_str("and create a report with columns: Peptide Sequence, Precursor Mz, Retention Time, Total Area, Max Height, Fwhm, Mass Error PPM.");
            }

            error!(
                stderr = %stderr,
                stdout = %stdout,
                exit_code = exit_code,
                "Skyline extraction failed"
            );
            return Err(ExtractionError::SkylineExecution(error_msg));
        }

        // Parse the report
        let target_metrics = self.parse_report(&report_path)?;

        // Calculate run metrics
        let run_metrics = self.calculate_run_metrics(&target_metrics);

        // Get Skyline version
        let skyline_version =
            skyline::get_version(skyline_path).unwrap_or_else(|_| "unknown".to_string());

        // Calculate raw file hash
        let raw_file_hash = calculate_file_hash(raw_path).unwrap_or_else(|_| "error".to_string());

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
    ///
    /// Uses header-based column detection to be flexible with different report formats.
    fn parse_report(&self, report_path: &Path) -> Result<Vec<TargetMetrics>, ExtractionError> {
        let file = std::fs::File::open(report_path)
            .map_err(|e| ExtractionError::ReportParse(e.to_string()))?;

        let mut reader = csv::Reader::from_reader(file);
        let mut metrics = Vec::new();

        // Build column index map from headers
        let headers = reader
            .headers()
            .map_err(|e| ExtractionError::ReportParse(format!("Failed to read headers: {}", e)))?
            .clone();

        let col_map = build_column_map(&headers);
        debug!(?col_map, "Parsed report column mapping");

        for (row_idx, result) in reader.records().enumerate() {
            let record = result.map_err(|e| ExtractionError::ReportParse(e.to_string()))?;

            // Generate target_id from peptide sequence + mz, or use row number
            let peptide_seq = get_string(&record, col_map.get("peptide_sequence"));
            let mz = get_float(&record, col_map.get("precursor_mz")).unwrap_or(0.0);
            let target_id = if let Some(ref seq) = peptide_seq {
                format!("{}_{:.2}", seq, mz)
            } else {
                format!("target_{}", row_idx + 1)
            };

            let peak_area = get_float(&record, col_map.get("peak_area")).unwrap_or(0.0);

            let target_metrics = TargetMetrics {
                target_id,
                peptide_sequence: peptide_seq,
                precursor_mz: mz,
                retention_time: get_float(&record, col_map.get("retention_time")).unwrap_or(0.0),
                rt_expected: get_float(&record, col_map.get("rt_expected")),
                rt_delta: get_float(&record, col_map.get("rt_delta")),
                peak_area,
                peak_height: get_float(&record, col_map.get("peak_height")).unwrap_or(0.0),
                peak_width_fwhm: get_float(&record, col_map.get("fwhm")),
                peak_symmetry: get_float(&record, col_map.get("peak_symmetry")),
                mass_error_ppm: get_float(&record, col_map.get("mass_error_ppm")),
                isotope_dot_product: get_float(&record, col_map.get("isotope_dot_product")),
                detected: peak_area > 0.0,
            };

            metrics.push(target_metrics);
        }

        info!(targets_parsed = metrics.len(), "Parsed Skyline report");
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
        let mut rt_deltas: Vec<f64> = targets.iter().filter_map(|t| t.rt_delta).collect();
        rt_deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_rt_shift = if !rt_deltas.is_empty() {
            let mid = rt_deltas.len() / 2;
            if rt_deltas.len().is_multiple_of(2) {
                Some((rt_deltas[mid - 1] + rt_deltas[mid]) / 2.0)
            } else {
                Some(rt_deltas[mid])
            }
        } else {
            None
        };

        // Calculate median mass error
        let mut mass_errors: Vec<f64> = targets.iter().filter_map(|t| t.mass_error_ppm).collect();
        mass_errors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_mass_error_ppm = if !mass_errors.is_empty() {
            let mid = mass_errors.len() / 2;
            if mass_errors.len().is_multiple_of(2) {
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

        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let name = entry.file_name();
            hasher.update(name.to_string_lossy().as_bytes());

            if let Ok(meta) = entry.metadata() {
                hasher.update(meta.len().to_le_bytes());
            }
        }

        Ok(hex::encode(hasher.finalize()))
    } else {
        anyhow::bail!("Path is neither file nor directory: {}", path.display())
    }
}

/// Build a mapping from our field names to CSV column indices.
///
/// Handles various Skyline column name variations.
fn build_column_map(headers: &csv::StringRecord) -> std::collections::HashMap<&'static str, usize> {
    let mut map = std::collections::HashMap::new();

    for (idx, header) in headers.iter().enumerate() {
        let header_lower = header.to_lowercase();
        let header_normalized = header_lower.replace(" ", "").replace("_", "");

        // Match various column name patterns to our canonical field names
        let field = match header_normalized.as_str() {
            // Peptide/Molecule identification
            "peptidesequence" | "peptide" | "modifiedsequence" | "sequence" => {
                Some("peptide_sequence")
            }
            "moleculename" | "molecule" | "compoundname" => Some("peptide_sequence"),

            // Precursor m/z
            "mz" | "precursormz" | "precursormass" | "mass" => Some("precursor_mz"),

            // Retention time
            "retentiontime" | "rt" | "peptideretentiontime" | "bestretentiontime" => {
                Some("retention_time")
            }
            "predictedretentiontime" | "expectedrt" | "rtexpected" => Some("rt_expected"),
            "rtdelta" | "retentiontimedelta" | "rtdifference" => Some("rt_delta"),

            // Peak metrics
            "totalarea" | "area" | "peakarea" | "sumarea" => Some("peak_area"),
            "maxheight" | "height" | "peakheight" | "maxintensity" => Some("peak_height"),
            "fwhm" | "maxfwhm" | "peakwidth" | "width" => Some("fwhm"),
            "peaksymmetry" | "symmetry" => Some("peak_symmetry"),

            // Mass accuracy
            "masserrorppm" | "averagemasserrorppm" | "ppm" | "deltamass" => Some("mass_error_ppm"),

            // Quality scores
            "isotopedotproduct" | "idotp" | "dotproduct" => Some("isotope_dot_product"),

            _ => None,
        };

        if let Some(field_name) = field {
            map.insert(field_name, idx);
        }
    }

    map
}

/// Get a string value from a CSV record by column index.
fn get_string(record: &csv::StringRecord, col: Option<&usize>) -> Option<String> {
    col.and_then(|&idx| record.get(idx))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Get a float value from a CSV record by column index.
fn get_float(record: &csv::StringRecord, col: Option<&usize>) -> Option<f64> {
    col.and_then(|&idx| record.get(idx))
        .and_then(|s| s.parse().ok())
}
