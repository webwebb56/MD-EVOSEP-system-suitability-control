//! File watcher for detecting completed MS runs.
//!
//! Implements a two-tier watch strategy:
//! 1. Primary: Filesystem events (ReadDirectoryChangesW on Windows)
//! 2. Fallback: Periodic directory scanning
//!
//! Events are treated as hints; all files go through a finalization
//! state machine before processing.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use notify::RecommendedWatcher;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use crate::config::{InstrumentConfig, WatcherConfig};
use crate::types::{FinalizationState, TrackedFile, Vendor};

mod finalizer;

// Finalizer is used internally by the watcher state machine
#[allow(dead_code)]
pub use finalizer::Finalizer;

/// File watcher for a single instrument.
pub struct Watcher {
    instrument: InstrumentConfig,
    config: WatcherConfig,
    ready_tx: mpsc::Sender<TrackedFile>,
    tracked_files: Arc<Mutex<HashMap<PathBuf, TrackedFile>>>,
    notify_watcher: Option<RecommendedWatcher>,
    running: Arc<Mutex<bool>>,
}

impl Watcher {
    /// Create a new watcher for an instrument.
    pub fn new(
        instrument: InstrumentConfig,
        config: WatcherConfig,
        ready_tx: mpsc::Sender<TrackedFile>,
    ) -> Result<Self> {
        Ok(Self {
            instrument,
            config,
            ready_tx,
            tracked_files: Arc::new(Mutex::new(HashMap::new())),
            notify_watcher: None,
            running: Arc::new(Mutex::new(false)),
        })
    }

    /// Start watching for files.
    pub fn start(&self) -> Result<()> {
        let watch_path = PathBuf::from(&self.instrument.watch_path);

        if !watch_path.exists() {
            anyhow::bail!(
                "Watch path does not exist: {}",
                watch_path.display()
            );
        }

        info!(
            instrument = %self.instrument.id,
            path = %watch_path.display(),
            "Starting watcher"
        );

        *self.running.lock().unwrap() = true;

        // Start the finalization loop
        let tracked_files = Arc::clone(&self.tracked_files);
        let ready_tx = self.ready_tx.clone();
        let config = self.config.clone();
        let instrument_id = self.instrument.id.clone();
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            run_finalization_loop(
                tracked_files,
                ready_tx,
                config,
                instrument_id,
                running,
            )
            .await
        });

        // Start the scan loop (fallback or primary depending on config)
        let tracked_files = Arc::clone(&self.tracked_files);
        let watch_path_clone = watch_path.clone();
        let file_pattern = self.instrument.file_pattern.clone();
        let vendor = self.instrument.vendor;
        let scan_interval = self.config.scan_interval_seconds;
        let instrument_id = self.instrument.id.clone();
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            run_scan_loop(
                tracked_files,
                watch_path_clone,
                file_pattern,
                vendor,
                scan_interval,
                instrument_id,
                running,
            )
            .await
        });

        Ok(())
    }

    /// Stop watching.
    pub fn stop(&self) -> Result<()> {
        info!(instrument = %self.instrument.id, "Stopping watcher");
        *self.running.lock().unwrap() = false;
        Ok(())
    }
}

/// Run the periodic directory scan loop.
async fn run_scan_loop(
    tracked_files: Arc<Mutex<HashMap<PathBuf, TrackedFile>>>,
    watch_path: PathBuf,
    file_pattern: String,
    vendor: Vendor,
    scan_interval_secs: u64,
    instrument_id: String,
    running: Arc<Mutex<bool>>,
) {
    let mut interval = tokio::time::interval(
        tokio::time::Duration::from_secs(scan_interval_secs)
    );

    loop {
        interval.tick().await;

        if !*running.lock().unwrap() {
            break;
        }

        trace!(instrument = %instrument_id, "Scanning directory");

        // Scan for files matching the pattern
        let pattern = watch_path.join(&file_pattern);
        let pattern_str = pattern.to_string_lossy();

        let entries = match glob::glob(&pattern_str) {
            Ok(entries) => entries,
            Err(e) => {
                warn!(
                    instrument = %instrument_id,
                    error = %e,
                    "Failed to glob pattern"
                );
                continue;
            }
        };

        for entry in entries.flatten() {
            // Skip if already tracking
            {
                let tracked = tracked_files.lock().unwrap();
                if tracked.contains_key(&entry) {
                    continue;
                }
            }

            // Check if this is a valid raw file for the vendor
            if !is_valid_raw_file(&entry, vendor) {
                continue;
            }

            // Get file metadata
            let metadata = match std::fs::metadata(&entry) {
                Ok(m) => m,
                Err(e) => {
                    trace!(
                        path = %entry.display(),
                        error = %e,
                        "Failed to get metadata"
                    );
                    continue;
                }
            };

            let size = metadata.len();
            let modified: DateTime<Utc> = metadata
                .modified()
                .map(|t| t.into())
                .unwrap_or_else(|_| Utc::now());

            // Start tracking
            let tracked_file = TrackedFile {
                path: entry.clone(),
                state: FinalizationState::Detected,
                first_seen: Utc::now(),
                last_size: size,
                last_modified: modified,
                stable_since: None,
                vendor,
            };

            info!(
                instrument = %instrument_id,
                path = %entry.display(),
                size = size,
                "File detected"
            );

            tracked_files.lock().unwrap().insert(entry, tracked_file);
        }
    }
}

/// Run the finalization state machine loop.
async fn run_finalization_loop(
    tracked_files: Arc<Mutex<HashMap<PathBuf, TrackedFile>>>,
    ready_tx: mpsc::Sender<TrackedFile>,
    config: WatcherConfig,
    instrument_id: String,
    running: Arc<Mutex<bool>>,
) {
    let check_interval = tokio::time::Duration::from_secs(5);
    let mut interval = tokio::time::interval(check_interval);

    let stability_window = Duration::seconds(config.stability_window_seconds as i64);
    let stabilization_timeout = Duration::seconds(config.stabilization_timeout_seconds as i64);

    loop {
        interval.tick().await;

        if !*running.lock().unwrap() {
            break;
        }

        let mut to_remove = Vec::new();
        let mut to_ready = Vec::new();

        {
            let mut tracked = tracked_files.lock().unwrap();

            for (path, file) in tracked.iter_mut() {
                match file.state {
                    FinalizationState::Detected => {
                        // Transition to stabilizing
                        file.state = FinalizationState::Stabilizing;
                        debug!(
                            instrument = %instrument_id,
                            path = %path.display(),
                            "File stabilizing"
                        );
                    }

                    FinalizationState::Stabilizing => {
                        // Check for timeout
                        let elapsed = Utc::now() - file.first_seen;
                        if elapsed > stabilization_timeout {
                            warn!(
                                instrument = %instrument_id,
                                path = %path.display(),
                                "Stabilization timeout"
                            );
                            file.state = FinalizationState::Failed;
                            continue;
                        }

                        // Check current state
                        let metadata = match std::fs::metadata(path) {
                            Ok(m) => m,
                            Err(e) => {
                                warn!(
                                    instrument = %instrument_id,
                                    path = %path.display(),
                                    error = %e,
                                    "Failed to get metadata during stabilization"
                                );
                                continue;
                            }
                        };

                        let current_size = metadata.len();
                        let current_modified: DateTime<Utc> = metadata
                            .modified()
                            .map(|t| t.into())
                            .unwrap_or_else(|_| Utc::now());

                        // Check if stable
                        if current_size == file.last_size
                            && current_modified == file.last_modified
                        {
                            // Still stable
                            if file.stable_since.is_none() {
                                file.stable_since = Some(Utc::now());
                            }

                            let stable_duration =
                                Utc::now() - file.stable_since.unwrap();

                            if stable_duration >= stability_window {
                                // Check vendor-specific finalization
                                if check_vendor_finalization(path, file.vendor) {
                                    file.state = FinalizationState::Ready;
                                    debug!(
                                        instrument = %instrument_id,
                                        path = %path.display(),
                                        "File ready for processing"
                                    );
                                }
                            }
                        } else {
                            // File changed, reset stability
                            file.last_size = current_size;
                            file.last_modified = current_modified;
                            file.stable_since = None;
                            trace!(
                                instrument = %instrument_id,
                                path = %path.display(),
                                size = current_size,
                                "File still changing"
                            );
                        }
                    }

                    FinalizationState::Ready => {
                        // Try non-sharing open test
                        if try_exclusive_open(path) {
                            file.state = FinalizationState::Processing;
                            to_ready.push(file.clone());
                            info!(
                                instrument = %instrument_id,
                                path = %path.display(),
                                "File finalized, queuing for processing"
                            );
                        } else {
                            trace!(
                                instrument = %instrument_id,
                                path = %path.display(),
                                "File still locked"
                            );
                        }
                    }

                    FinalizationState::Processing => {
                        // Will be handled elsewhere
                    }

                    FinalizationState::Done | FinalizationState::Failed => {
                        to_remove.push(path.clone());
                    }
                }
            }
        }

        // Send ready files
        for file in to_ready {
            if let Err(e) = ready_tx.send(file.clone()).await {
                error!(
                    path = %file.path.display(),
                    error = %e,
                    "Failed to send file to processing queue"
                );
            }
        }

        // Remove completed/failed files from tracking
        if !to_remove.is_empty() {
            let mut tracked = tracked_files.lock().unwrap();
            for path in to_remove {
                tracked.remove(&path);
            }
        }
    }
}

/// Check if a path is a valid raw file for the given vendor.
fn is_valid_raw_file(path: &Path, vendor: Vendor) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match vendor {
        Vendor::Thermo => extension.as_deref() == Some("raw") && path.is_file(),
        Vendor::Bruker => extension.as_deref() == Some("d") && path.is_dir(),
        Vendor::Sciex => {
            matches!(extension.as_deref(), Some("wiff") | Some("wiff2"))
                && path.is_file()
        }
        Vendor::Waters => extension.as_deref() == Some("raw") && path.is_dir(),
        Vendor::Agilent => extension.as_deref() == Some("d") && path.is_dir(),
    }
}

/// Check vendor-specific finalization requirements.
fn check_vendor_finalization(path: &Path, vendor: Vendor) -> bool {
    match vendor {
        Vendor::Thermo => {
            // Thermo .raw: just needs to be a stable file
            true
        }
        Vendor::Bruker => {
            // Bruker .d: check for analysis.tdf and no lock file
            let analysis_tdf = path.join("analysis.tdf");
            let lock_file = path.join("analysis.tdf-journal");

            analysis_tdf.exists() && !lock_file.exists()
        }
        Vendor::Sciex => {
            // Sciex .wiff: check for .wiff.scan companion
            let scan_file = path.with_extension("wiff.scan");
            // .scan file is optional in newer versions
            !scan_file.exists()
                || (scan_file.exists() && try_exclusive_open(&scan_file))
        }
        Vendor::Waters => {
            // Waters .raw directory: check for _FUNC001.DAT
            let func_file = path.join("_FUNC001.DAT");
            func_file.exists()
        }
        Vendor::Agilent => {
            // Agilent .d: check for AcqData subdirectory
            let acq_data = path.join("AcqData");
            acq_data.exists() && acq_data.is_dir()
        }
    }
}

/// Try to open a file exclusively to verify it's not in use.
fn try_exclusive_open(path: &Path) -> bool {
    if path.is_dir() {
        // For directories, we can't do exclusive open
        // Just return true if vendor-specific checks passed
        return true;
    }

    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        // FILE_SHARE_NONE = 0
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .is_ok()
    }

    #[cfg(not(windows))]
    {
        // On non-Windows, just check if we can open for reading
        std::fs::File::open(path).is_ok()
    }
}
