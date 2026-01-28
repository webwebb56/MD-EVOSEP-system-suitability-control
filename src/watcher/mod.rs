//! File watcher for detecting completed MS runs.
//!
//! Implements a two-tier watch strategy per spec:
//! 1. Primary: Filesystem events (ReadDirectoryChangesW on Windows) for local paths
//! 2. Fallback: Periodic directory scanning for network shares or when events fail
//!
//! Events are treated as hints; all files go through a finalization
//! state machine before processing.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use notify::{
    Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use crate::config::{InstrumentConfig, WatcherConfig};
use crate::failed_files::FailedFiles;
use crate::types::{FinalizationState, TrackedFile, Vendor};

mod finalizer;

/// File watcher for a single instrument.
pub struct Watcher {
    instrument: InstrumentConfig,
    config: WatcherConfig,
    ready_tx: mpsc::Sender<TrackedFile>,
    tracked_files: Arc<Mutex<HashMap<PathBuf, TrackedFile>>>,
    running: Arc<Mutex<bool>>,
    is_network_path: bool,
}

impl Watcher {
    /// Create a new watcher for an instrument.
    pub fn new(
        instrument: InstrumentConfig,
        config: WatcherConfig,
        ready_tx: mpsc::Sender<TrackedFile>,
    ) -> Result<Self> {
        let watch_path = PathBuf::from(&instrument.watch_path);
        let is_network_path = Self::detect_network_path(&watch_path);

        if is_network_path {
            warn!(
                instrument = %instrument.id,
                path = %watch_path.display(),
                "Network path detected - using polling-only mode (filesystem events unreliable on SMB/CIFS)"
            );
        }

        Ok(Self {
            instrument,
            config,
            ready_tx,
            tracked_files: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(false)),
            is_network_path,
        })
    }

    /// Detect if a path is a network share.
    fn detect_network_path(path: &Path) -> bool {
        // Check for UNC path (\\server\share)
        let path_str = path.to_string_lossy();
        if path_str.starts_with(r"\\") {
            return true;
        }

        // On Windows, check drive type
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;

            if let Some(prefix) = path.components().next() {
                let prefix_str = prefix.as_os_str().to_string_lossy();
                if prefix_str.len() >= 2 && prefix_str.chars().nth(1) == Some(':') {
                    let drive = format!("{}\\", &prefix_str[..2]);
                    let drive_wide: Vec<u16> = std::ffi::OsStr::new(&drive)
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();

                    // GetDriveTypeW returns DRIVE_REMOTE (4) for network drives
                    let drive_type = unsafe {
                        windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(drive_wide.as_ptr())
                    };

                    // DRIVE_REMOTE = 4
                    if drive_type == 4 {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Start watching for files.
    pub fn start(&self) -> Result<()> {
        let watch_path = PathBuf::from(&self.instrument.watch_path);

        if !watch_path.exists() {
            anyhow::bail!("Watch path does not exist: {}", watch_path.display());
        }

        info!(
            instrument = %self.instrument.id,
            path = %watch_path.display(),
            use_events = !self.is_network_path && self.config.use_filesystem_events,
            "Starting watcher"
        );

        *self.running.lock().unwrap() = true;

        // Start filesystem event watcher if enabled and not a network path
        if self.config.use_filesystem_events && !self.is_network_path {
            let tracked_files = Arc::clone(&self.tracked_files);
            let watch_path_clone = watch_path.clone();
            let vendor = self.instrument.vendor;
            let instrument_id = self.instrument.id.clone();
            let running = Arc::clone(&self.running);

            std::thread::spawn(move || {
                if let Err(e) = run_event_watcher(
                    tracked_files,
                    watch_path_clone,
                    vendor,
                    instrument_id.clone(),
                    running,
                ) {
                    error!(
                        instrument = %instrument_id,
                        error = %e,
                        "Event watcher failed, falling back to polling only"
                    );
                }
            });
        }

        // Start the finalization loop
        let tracked_files = Arc::clone(&self.tracked_files);
        let ready_tx = self.ready_tx.clone();
        let config = self.config.clone();
        let instrument_id = self.instrument.id.clone();
        let running = Arc::clone(&self.running);
        let failed_files = FailedFiles::new();

        tokio::spawn(async move {
            run_finalization_loop(
                tracked_files,
                ready_tx,
                config,
                instrument_id,
                running,
                failed_files,
            )
            .await
        });

        // Start the scan loop (always runs as fallback/supplement)
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

    /// Mark a file as done (called after successful processing).
    pub fn mark_done(&self, path: &Path) {
        let mut tracked = self.tracked_files.lock().unwrap();
        if let Some(file) = tracked.get_mut(path) {
            file.state = FinalizationState::Done;
            debug!(path = %path.display(), "File marked as done");
        }
    }

    /// Mark a file as failed (called after processing error).
    pub fn mark_failed(&self, path: &Path) {
        let mut tracked = self.tracked_files.lock().unwrap();
        if let Some(file) = tracked.get_mut(path) {
            file.state = FinalizationState::Failed;
            warn!(path = %path.display(), "File marked as failed");
        }
    }
}

/// Run filesystem event watcher using notify crate.
fn run_event_watcher(
    tracked_files: Arc<Mutex<HashMap<PathBuf, TrackedFile>>>,
    watch_path: PathBuf,
    vendor: Vendor,
    instrument_id: String,
    running: Arc<Mutex<bool>>,
) -> Result<()> {
    let tracked_files_clone = Arc::clone(&tracked_files);
    let instrument_id_clone = instrument_id.clone();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Only care about create and modify events
                    let dominated = matches!(
                        event.kind,
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                    );

                    if !dominated {
                        return;
                    }

                    for path in event.paths {
                        // Check if it's a valid raw file
                        if !is_valid_raw_file(&path, vendor) {
                            continue;
                        }

                        // Check if already tracking
                        {
                            let tracked = tracked_files_clone.lock().unwrap();
                            if tracked.contains_key(&path) {
                                // Already tracking, event will update stability
                                continue;
                            }
                        }

                        // Get file metadata
                        let metadata = match std::fs::metadata(&path) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };

                        let size = metadata.len();
                        let modified: DateTime<Utc> = metadata
                            .modified()
                            .map(|t| t.into())
                            .unwrap_or_else(|_| Utc::now());

                        // Start tracking
                        let tracked_file = TrackedFile {
                            path: path.clone(),
                            state: FinalizationState::Detected,
                            first_seen: Utc::now(),
                            last_size: size,
                            last_modified: modified,
                            stable_since: None,
                            vendor,
                        };

                        info!(
                            instrument = %instrument_id_clone,
                            path = %path.display(),
                            size = size,
                            source = "event",
                            "File detected via filesystem event"
                        );

                        tracked_files_clone
                            .lock()
                            .unwrap()
                            .insert(path, tracked_file);
                    }
                }
                Err(e) => {
                    warn!(
                        instrument = %instrument_id_clone,
                        error = %e,
                        "Filesystem event error"
                    );
                }
            }
        },
        NotifyConfig::default(),
    )?;

    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    info!(
        instrument = %instrument_id,
        path = %watch_path.display(),
        "Filesystem event watcher started"
    );

    // Keep the watcher alive until stopped
    while *running.lock().unwrap() {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    Ok(())
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
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(scan_interval_secs));

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
                source = "scan",
                "File detected via directory scan"
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
    failed_files: FailedFiles,
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
        let mut to_record_failed: Vec<(PathBuf, String)> = Vec::new();

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
                            to_record_failed.push((
                                path.clone(),
                                format!(
                                    "Stabilization timeout after {} seconds",
                                    config.stabilization_timeout_seconds
                                ),
                            ));
                            continue;
                        }

                        // Check current state based on vendor type
                        let (current_size, current_modified, is_complete) =
                            check_file_state(path, file.vendor);

                        // Check if stable
                        if current_size == file.last_size && current_modified == file.last_modified
                        {
                            // Still stable
                            if file.stable_since.is_none() {
                                file.stable_since = Some(Utc::now());
                            }

                            let stable_duration = Utc::now() - file.stable_since.unwrap();

                            if stable_duration >= stability_window && is_complete {
                                file.state = FinalizationState::Ready;
                                debug!(
                                    instrument = %instrument_id,
                                    path = %path.display(),
                                    "File ready for processing"
                                );
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
                        if try_exclusive_open(path, file.vendor) {
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
                        // Processing state - waiting for mark_done/mark_failed
                        // Check for processing timeout (e.g., 30 minutes)
                        let processing_timeout = Duration::minutes(30);
                        if let Some(stable_since) = file.stable_since {
                            if Utc::now() - stable_since > processing_timeout {
                                warn!(
                                    instrument = %instrument_id,
                                    path = %path.display(),
                                    "Processing timeout - marking as failed"
                                );
                                file.state = FinalizationState::Failed;
                                to_record_failed.push((
                                    path.clone(),
                                    "Processing timeout after 30 minutes".to_string(),
                                ));
                            }
                        }
                    }

                    FinalizationState::Done => {
                        debug!(
                            instrument = %instrument_id,
                            path = %path.display(),
                            "Removing completed file from tracking"
                        );
                        to_remove.push(path.clone());
                    }

                    FinalizationState::Failed => {
                        warn!(
                            instrument = %instrument_id,
                            path = %path.display(),
                            "Removing failed file from tracking"
                        );
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

        // Record failed files
        for (path, reason) in to_record_failed {
            failed_files.record_failure(path, instrument_id.clone(), reason);
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

/// Check file state including vendor-specific internal file checks.
/// Returns (size, modified_time, is_complete).
fn check_file_state(path: &Path, vendor: Vendor) -> (u64, DateTime<Utc>, bool) {
    let default_time = Utc::now();

    match vendor {
        Vendor::Thermo => {
            // Thermo .raw: single file
            let metadata = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => return (0, default_time, false),
            };
            let modified: DateTime<Utc> = metadata
                .modified()
                .map(|t| t.into())
                .unwrap_or(default_time);
            (metadata.len(), modified, true)
        }

        Vendor::Bruker => {
            // Bruker .d: check analysis.tdf stability and lock file absence
            let analysis_tdf = path.join("analysis.tdf");
            let lock_file = path.join("analysis.tdf-journal");
            let lock_file2 = path.join("analysis.tdf-lock");

            if lock_file.exists() || lock_file2.exists() {
                // Lock file present - acquisition in progress
                return (0, default_time, false);
            }

            if !analysis_tdf.exists() {
                return (0, default_time, false);
            }

            let metadata = match std::fs::metadata(&analysis_tdf) {
                Ok(m) => m,
                Err(_) => return (0, default_time, false),
            };
            let modified: DateTime<Utc> = metadata
                .modified()
                .map(|t| t.into())
                .unwrap_or(default_time);
            (metadata.len(), modified, true)
        }

        Vendor::Sciex => {
            // Sciex .wiff: check both .wiff and .wiff.scan files
            let scan_file = path.with_extension("wiff.scan");

            let wiff_metadata = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => return (0, default_time, false),
            };

            // .wiff.scan might not exist in newer versions
            let (total_size, latest_modified) = if scan_file.exists() {
                let scan_metadata = match std::fs::metadata(&scan_file) {
                    Ok(m) => m,
                    Err(_) => return (0, default_time, false),
                };

                let wiff_modified: DateTime<Utc> = wiff_metadata
                    .modified()
                    .map(|t| t.into())
                    .unwrap_or(default_time);
                let scan_modified: DateTime<Utc> = scan_metadata
                    .modified()
                    .map(|t| t.into())
                    .unwrap_or(default_time);

                let latest = if scan_modified > wiff_modified {
                    scan_modified
                } else {
                    wiff_modified
                };

                (wiff_metadata.len() + scan_metadata.len(), latest)
            } else {
                let modified: DateTime<Utc> = wiff_metadata
                    .modified()
                    .map(|t| t.into())
                    .unwrap_or(default_time);
                (wiff_metadata.len(), modified)
            };

            (total_size, latest_modified, true)
        }

        Vendor::Waters => {
            // Waters .raw directory: check _FUNC001.DAT and _extern.inf
            let func_file = path.join("_FUNC001.DAT");
            let extern_inf = path.join("_extern.inf");
            let lock_file = path.join("_LOCK_");

            if lock_file.exists() {
                return (0, default_time, false);
            }

            if !func_file.exists() {
                return (0, default_time, false);
            }

            let func_metadata = match std::fs::metadata(&func_file) {
                Ok(m) => m,
                Err(_) => return (0, default_time, false),
            };

            let modified: DateTime<Utc> = func_metadata
                .modified()
                .map(|t| t.into())
                .unwrap_or(default_time);

            // Also check _extern.inf if it exists (indicates acquisition complete)
            let is_complete = extern_inf.exists();

            (func_metadata.len(), modified, is_complete)
        }

        Vendor::Agilent => {
            // Agilent .d: check AcqData subdirectory and MSScan.bin
            let acq_data = path.join("AcqData");
            let ms_scan = acq_data.join("MSScan.bin");

            if !acq_data.exists() || !acq_data.is_dir() {
                return (0, default_time, false);
            }

            let check_file = if ms_scan.exists() {
                ms_scan
            } else {
                // Fall back to checking the directory itself
                acq_data
            };

            let metadata = match std::fs::metadata(&check_file) {
                Ok(m) => m,
                Err(_) => return (0, default_time, false),
            };

            let modified: DateTime<Utc> = metadata
                .modified()
                .map(|t| t.into())
                .unwrap_or(default_time);

            (metadata.len(), modified, true)
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
            matches!(extension.as_deref(), Some("wiff") | Some("wiff2")) && path.is_file()
        }
        Vendor::Waters => extension.as_deref() == Some("raw") && path.is_dir(),
        Vendor::Agilent => extension.as_deref() == Some("d") && path.is_dir(),
    }
}

/// Try to open a file exclusively to verify it's not in use.
fn try_exclusive_open(path: &Path, vendor: Vendor) -> bool {
    // For directory-based formats, check the key internal file
    let file_to_check = match vendor {
        Vendor::Thermo => path.to_path_buf(),
        Vendor::Bruker => path.join("analysis.tdf"),
        Vendor::Sciex => path.to_path_buf(),
        Vendor::Waters => path.join("_FUNC001.DAT"),
        Vendor::Agilent => path.join("AcqData").join("MSScan.bin"),
    };

    if !file_to_check.exists() || file_to_check.is_dir() {
        return true; // Can't check directories, assume OK if vendor checks passed
    }

    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        // FILE_SHARE_NONE = 0
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&file_to_check)
            .is_ok()
    }

    #[cfg(not(windows))]
    {
        // On non-Windows, just check if we can open for reading
        std::fs::File::open(&file_to_check).is_ok()
    }
}
