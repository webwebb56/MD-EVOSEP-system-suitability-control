//! Baseline management.
//!
//! Baselines are primarily managed by the MD cloud, but the agent
//! needs to track active baselines for comparison metrics.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::types::{Baseline, RunMetrics, TargetMetrics};

/// Baseline manager that caches baseline information from the cloud.
pub struct BaselineManager {
    /// Cached baselines by instrument ID
    baselines: Arc<RwLock<HashMap<String, Baseline>>>,
}

impl BaselineManager {
    pub fn new() -> Self {
        Self {
            baselines: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the active baseline for an instrument.
    pub async fn get_active(&self, instrument_id: &str) -> Option<Baseline> {
        let baselines = self.baselines.read().await;
        baselines.get(instrument_id).cloned()
    }

    /// Update the cached baseline for an instrument.
    pub async fn update(&self, baseline: Baseline) {
        let mut baselines = self.baselines.write().await;
        baselines.insert(baseline.instrument_id.clone(), baseline);
    }

    /// Clear the cached baseline for an instrument.
    pub async fn clear(&self, instrument_id: &str) {
        let mut baselines = self.baselines.write().await;
        baselines.remove(instrument_id);
    }

    /// Refresh baselines from the cloud.
    pub async fn refresh_from_cloud(&self, _cloud_endpoint: &str) -> anyhow::Result<()> {
        // TODO: Implement cloud API call to fetch active baselines
        // For now, this is a no-op
        Ok(())
    }
}

impl Default for BaselineManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Compare run metrics against a baseline.
pub fn compare_to_baseline(
    _run_metrics: &RunMetrics,
    target_metrics: &[TargetMetrics],
    baseline: &Baseline,
) -> ComparisonResult {
    // Calculate RT shift statistics
    let mut rt_shifts = Vec::new();
    let mut area_ratios = Vec::new();
    let mut outliers = Vec::new();

    for target in target_metrics {
        // Find corresponding baseline target
        let baseline_target = baseline
            .target_metrics
            .iter()
            .find(|bt| bt.target_id == target.target_id);

        if let Some(bt) = baseline_target {
            // RT shift
            let rt_shift = target.retention_time - bt.retention_time;
            rt_shifts.push(rt_shift);

            // Area ratio
            if bt.peak_area > 0.0 {
                let ratio = target.peak_area / bt.peak_area;
                area_ratios.push(ratio);

                // Check for outliers (>3 sigma from 1.0)
                if (ratio - 1.0).abs() > 0.5 {
                    outliers.push(target.target_id.clone());
                }
            }
        }
    }

    // Calculate statistics
    let rt_shift_mean = mean(&rt_shifts);
    let rt_shift_std = std_dev(&rt_shifts);
    let area_ratio_mean = mean(&area_ratios);
    let area_ratio_std = std_dev(&area_ratios);

    let within_tolerance = outliers.is_empty() && rt_shift_std < 0.5;

    ComparisonResult {
        rt_shift_mean,
        rt_shift_std,
        area_ratio_mean,
        area_ratio_std,
        outlier_targets: outliers,
        within_tolerance,
    }
}

/// Result of comparing a run to a baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    pub rt_shift_mean: f64,
    pub rt_shift_std: f64,
    pub area_ratio_mean: f64,
    pub area_ratio_std: f64,
    pub outlier_targets: Vec<String>,
    pub within_tolerance: bool,
}

/// Calculate mean of a slice.
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Calculate standard deviation of a slice.
fn std_dev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }

    let m = mean(values);
    let variance = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (values.len() - 1) as f64;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mean() {
        assert_eq!(mean(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn test_std_dev() {
        let values = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let sd = std_dev(&values);
        // Sample std dev = sqrt(32/7) ≈ 2.138
        assert!((sd - 2.138).abs() < 0.01, "Expected ~2.138, got {}", sd);
    }
}
