//! Failed files tracking and management.
//!
//! Tracks files that failed to process (timeout, errors, etc.) and allows
//! users to view and retry them.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::paths;

/// Maximum number of failed files to keep in history
const MAX_FAILED_FILES: usize = 100;

/// A file that failed to process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedFile {
    /// Path to the file
    pub path: PathBuf,
    /// Instrument ID
    pub instrument_id: String,
    /// Reason for failure
    pub reason: String,
    /// When the failure occurred
    pub failed_at: DateTime<Utc>,
    /// Number of retry attempts
    pub retry_count: u32,
}

/// Store for tracking failed files
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailedFilesStore {
    /// Map of file path to failure info
    pub files: HashMap<PathBuf, FailedFile>,
}

impl FailedFilesStore {
    /// Load the failed files store from disk
    pub fn load() -> Result<Self> {
        let store_path = Self::store_path();

        if !store_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&store_path)?;
        let store: Self = serde_json::from_str(&content)?;
        Ok(store)
    }

    /// Save the store to disk
    pub fn save(&self) -> Result<()> {
        let store_path = Self::store_path();

        if let Some(parent) = store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&store_path, content)?;
        Ok(())
    }

    /// Get the path to the store file
    fn store_path() -> PathBuf {
        paths::data_dir().join("failed_files.json")
    }

    /// Add a failed file
    pub fn add(&mut self, path: PathBuf, instrument_id: String, reason: String) {
        let failed = FailedFile {
            path: path.clone(),
            instrument_id,
            reason,
            failed_at: Utc::now(),
            retry_count: 0,
        };

        self.files.insert(path, failed);

        // Trim to max size, removing oldest entries
        self.trim_to_max();

        // Save to disk (ignore errors)
        let _ = self.save();
    }

    /// Remove a file from the failed list (e.g., after successful retry)
    pub fn remove(&mut self, path: &Path) {
        self.files.remove(path);
        let _ = self.save();
    }

    /// Increment retry count for a file
    #[allow(dead_code)]
    pub fn increment_retry(&mut self, path: &Path) {
        if let Some(file) = self.files.get_mut(path) {
            file.retry_count += 1;
            let _ = self.save();
        }
    }

    /// Get all failed files, sorted by most recent first
    pub fn get_all(&self) -> Vec<&FailedFile> {
        let mut files: Vec<_> = self.files.values().collect();
        files.sort_by(|a, b| b.failed_at.cmp(&a.failed_at));
        files
    }

    /// Get count of failed files
    pub fn count(&self) -> usize {
        self.files.len()
    }

    /// Clear all failed files
    pub fn clear(&mut self) {
        self.files.clear();
        let _ = self.save();
    }

    /// Trim store to maximum size
    fn trim_to_max(&mut self) {
        if self.files.len() <= MAX_FAILED_FILES {
            return;
        }

        // Get paths sorted by oldest first
        let mut entries: Vec<_> = self.files.iter().collect();
        entries.sort_by(|a, b| a.1.failed_at.cmp(&b.1.failed_at));

        // Collect paths to remove
        let to_remove_count = self.files.len() - MAX_FAILED_FILES;
        let paths_to_remove: Vec<_> = entries
            .into_iter()
            .take(to_remove_count)
            .map(|(path, _)| path.clone())
            .collect();

        // Remove oldest entries
        for path in paths_to_remove {
            self.files.remove(&path);
        }
    }
}

/// Thread-safe wrapper for the failed files store
#[derive(Clone)]
pub struct FailedFiles {
    inner: Arc<Mutex<FailedFilesStore>>,
}

impl FailedFiles {
    /// Create a new failed files tracker, loading from disk
    pub fn new() -> Self {
        let store = FailedFilesStore::load().unwrap_or_default();
        Self {
            inner: Arc::new(Mutex::new(store)),
        }
    }

    /// Record a file failure
    pub fn record_failure(&self, path: PathBuf, instrument_id: String, reason: String) {
        let mut store = self.inner.lock().unwrap();
        store.add(path, instrument_id, reason);
    }

    /// Remove a file from failures (after successful processing)
    pub fn mark_success(&self, path: &Path) {
        let mut store = self.inner.lock().unwrap();
        store.remove(path);
    }

    /// Get retry info and increment counter
    #[allow(dead_code)]
    pub fn get_for_retry(&self, path: &Path) -> Option<FailedFile> {
        let mut store = self.inner.lock().unwrap();
        store.increment_retry(path);
        store.files.get(path).cloned()
    }

    /// Get all failed files
    pub fn get_all(&self) -> Vec<FailedFile> {
        let store = self.inner.lock().unwrap();
        store.get_all().into_iter().cloned().collect()
    }

    /// Get count
    pub fn count(&self) -> usize {
        let store = self.inner.lock().unwrap();
        store.count()
    }

    /// Clear all
    pub fn clear(&self) {
        let mut store = self.inner.lock().unwrap();
        store.clear();
    }
}

impl Default for FailedFiles {
    fn default() -> Self {
        Self::new()
    }
}
