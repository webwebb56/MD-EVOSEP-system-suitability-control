//! Local spool for reliable payload delivery.
//!
//! All extraction results are written to a local spool before upload.
//! This ensures no data loss even if the cloud is unreachable.

use anyhow::Result;
use chrono::{Duration, Utc};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::{paths, SpoolConfig};
use crate::error::SpoolError;
use crate::types::{
    ExtractionResult, QcPayload, RunClassification, RunInfo, ExtractionInfo, Vendor,
};

/// Spool manager for pending uploads.
#[derive(Clone)]
pub struct Spool {
    config: SpoolConfig,
    pending_dir: PathBuf,
    uploading_dir: PathBuf,
    failed_dir: PathBuf,
    completed_dir: PathBuf,
    agent_id: Arc<Mutex<String>>,
}

impl Spool {
    /// Create a new spool manager.
    pub fn new(config: &SpoolConfig) -> Result<Self> {
        let pending_dir = paths::spool_pending_dir();
        let uploading_dir = paths::spool_uploading_dir();
        let failed_dir = paths::spool_failed_dir();
        let completed_dir = paths::spool_completed_dir();

        // Ensure directories exist
        std::fs::create_dir_all(&pending_dir)?;
        std::fs::create_dir_all(&uploading_dir)?;
        std::fs::create_dir_all(&failed_dir)?;
        std::fs::create_dir_all(&completed_dir)?;

        Ok(Self {
            config: config.clone(),
            pending_dir,
            uploading_dir,
            failed_dir,
            completed_dir,
            agent_id: Arc::new(Mutex::new("unregistered".to_string())),
        })
    }

    /// Set the agent ID (call after initialization/enrollment).
    pub async fn set_agent_id(&self, agent_id: String) {
        *self.agent_id.lock().await = agent_id;
    }

    /// Get the current agent ID.
    pub async fn get_agent_id(&self) -> String {
        self.agent_id.lock().await.clone()
    }

    /// Generate a correlation ID for tracing.
    fn generate_correlation_id(&self, agent_id: &str) -> String {
        let timestamp = Utc::now().format("%Y%m%d%H%M%S");
        let random: u32 = rand::random();
        format!("{}-{}-{:08x}", agent_id, timestamp, random)
    }

    /// Enqueue an extraction result for upload.
    pub async fn enqueue(
        &self,
        result: &ExtractionResult,
        classification: &RunClassification,
        vendor: Vendor,
    ) -> Result<(), SpoolError> {
        // Check spool size limits
        self.check_limits()?;

        // Cleanup old payloads
        self.cleanup_old_payloads()?;

        // Get agent ID
        let agent_id = self.agent_id.lock().await.clone();

        // Generate correlation ID
        let correlation_id = self.generate_correlation_id(&agent_id);

        // Build payload
        let payload = QcPayload {
            schema_version: "1.0".to_string(),
            payload_id: Uuid::new_v4(),
            correlation_id: correlation_id.clone(),
            agent_id,
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: Utc::now(),

            run: RunInfo {
                run_id: result.run_id,
                raw_file_name: result.raw_file_name.clone(),
                raw_file_hash: result.raw_file_hash.clone(),
                acquisition_time: None, // Could be extracted from raw file
                instrument_id: classification.instrument_id.clone(),
                vendor, // Use the actual vendor from instrument config
                control_type: classification.control_type,
                well_position: classification.well_position.as_ref().map(|w| w.to_string()),
                plate_id: classification.plate_id.clone(),
                classification_confidence: classification.confidence,
                classification_source: classification.source,
            },

            extraction: ExtractionInfo {
                backend: result.backend.clone(),
                backend_version: result.backend_version.clone(),
                template_name: result.template_name.clone(),
                template_hash: result.template_hash.clone(),
                extraction_time_ms: result.extraction_time_ms,
                status: "SUCCESS".to_string(),
            },

            baseline_context: None, // TODO: fetch from baseline manager
            target_metrics: result.target_metrics.clone(),
            run_metrics: result.run_metrics.clone(),
            comparison_metrics: None, // TODO: compute if baseline exists
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&payload)?;

        // Write to pending directory
        let filename = format!("{}_payload.json", result.run_id);
        let temp_path = self.pending_dir.join(format!(".{}.tmp", filename));
        let final_path = self.pending_dir.join(&filename);

        // Write to temp file first, then rename (atomic on most filesystems)
        std::fs::write(&temp_path, &json)
            .map_err(|e| SpoolError::FileOperation(e.to_string()))?;

        std::fs::rename(&temp_path, &final_path)
            .map_err(|e| SpoolError::FileOperation(e.to_string()))?;

        info!(
            run_id = %result.run_id,
            correlation_id = %correlation_id,
            path = %final_path.display(),
            "Payload spooled"
        );

        Ok(())
    }

    /// Check spool size limits.
    fn check_limits(&self) -> Result<(), SpoolError> {
        let size_bytes = calculate_dir_size(&self.pending_dir);
        let size_mb = size_bytes / (1024 * 1024);

        if size_mb >= self.config.max_pending_mb {
            return Err(SpoolError::Full(size_mb, self.config.max_pending_mb));
        }

        Ok(())
    }

    /// Cleanup payloads older than max_age_days.
    fn cleanup_old_payloads(&self) -> Result<(), SpoolError> {
        let max_age = Duration::days(self.config.max_age_days as i64);
        let cutoff = Utc::now() - max_age;

        // Clean pending directory
        self.cleanup_old_in_dir(&self.pending_dir, cutoff)?;

        // Clean failed directory
        self.cleanup_old_in_dir(&self.failed_dir, cutoff)?;

        Ok(())
    }

    /// Remove files older than cutoff from a directory.
    fn cleanup_old_in_dir(
        &self,
        dir: &PathBuf,
        cutoff: chrono::DateTime<Utc>,
    ) -> Result<(), SpoolError> {
        if !dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| SpoolError::FileOperation(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();

            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    let modified: chrono::DateTime<Utc> = modified.into();
                    if modified < cutoff {
                        warn!(
                            path = %path.display(),
                            age_days = (Utc::now() - modified).num_days(),
                            "Removing stale payload (max_age_days exceeded)"
                        );
                        if let Err(e) = std::fs::remove_file(&path) {
                            error!(
                                path = %path.display(),
                                error = %e,
                                "Failed to remove stale payload"
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get all pending payloads.
    pub fn get_pending(&self) -> Result<Vec<PathBuf>> {
        let mut entries: Vec<_> = std::fs::read_dir(&self.pending_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();

        // Sort by modification time (oldest first)
        entries.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            a_time.cmp(&b_time)
        });

        Ok(entries)
    }

    /// Move a payload to the uploading directory.
    pub fn mark_uploading(&self, path: &PathBuf) -> Result<PathBuf> {
        let filename = path.file_name().ok_or_else(|| {
            anyhow::anyhow!("Invalid path")
        })?;
        let new_path = self.uploading_dir.join(filename);

        std::fs::rename(path, &new_path)?;
        debug!(path = %new_path.display(), "Payload marked as uploading");

        Ok(new_path)
    }

    /// Move a payload to the completed directory.
    pub fn mark_completed(&self, path: &PathBuf) -> Result<()> {
        let filename = path.file_name().ok_or_else(|| {
            anyhow::anyhow!("Invalid path")
        })?;
        let new_path = self.completed_dir.join(filename);

        std::fs::rename(path, &new_path)?;
        info!(path = %new_path.display(), "Payload uploaded successfully");

        // Cleanup old completed files
        self.cleanup_completed()?;

        Ok(())
    }

    /// Move a payload to the failed directory.
    pub fn mark_failed(&self, path: &PathBuf) -> Result<()> {
        let filename = path.file_name().ok_or_else(|| {
            anyhow::anyhow!("Invalid path")
        })?;
        let new_path = self.failed_dir.join(filename);

        std::fs::rename(path, &new_path)?;
        warn!(path = %new_path.display(), "Payload marked as failed");

        Ok(())
    }

    /// Move a payload back to pending (for retry).
    pub fn mark_pending(&self, path: &PathBuf) -> Result<PathBuf> {
        let filename = path.file_name().ok_or_else(|| {
            anyhow::anyhow!("Invalid path")
        })?;
        let new_path = self.pending_dir.join(filename);

        std::fs::rename(path, &new_path)?;
        debug!(path = %new_path.display(), "Payload returned to pending");

        Ok(new_path)
    }

    /// Cleanup old completed files.
    fn cleanup_completed(&self) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(&self.completed_dir)?
            .filter_map(|e| e.ok())
            .collect();

        if entries.len() <= self.config.completed_retention_count {
            return Ok(());
        }

        // Sort by modification time (oldest first)
        entries.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            a_time.cmp(&b_time)
        });

        // Remove oldest entries
        let to_remove = entries.len() - self.config.completed_retention_count;
        for entry in entries.into_iter().take(to_remove) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                warn!(
                    path = %entry.path().display(),
                    error = %e,
                    "Failed to cleanup completed payload"
                );
            }
        }

        Ok(())
    }

    /// Recovery: move any uploading files back to pending on startup.
    pub fn recover(&self) -> Result<()> {
        let entries: Vec<_> = std::fs::read_dir(&self.uploading_dir)?
            .filter_map(|e| e.ok())
            .collect();

        for entry in entries {
            let path = entry.path();
            if let Err(e) = self.mark_pending(&path) {
                error!(
                    path = %path.display(),
                    error = %e,
                    "Failed to recover uploading payload"
                );
            }
        }

        Ok(())
    }
}

/// Calculate total size of a directory in bytes.
fn calculate_dir_size(path: &PathBuf) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}
