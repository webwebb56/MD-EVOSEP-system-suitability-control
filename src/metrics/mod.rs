//! Metrics computation utilities.
//!
//! Helper functions for computing QC metrics from raw data.

#![allow(dead_code)]

use crate::types::TargetMetrics;

/// Calculate a chromatography quality score from target metrics.
///
/// The score is based on:
/// - Peak detection rate
/// - Peak width consistency
/// - Peak symmetry
/// - Mass accuracy
pub fn calculate_chromatography_score(targets: &[TargetMetrics]) -> f64 {
    if targets.is_empty() {
        return 0.0;
    }

    let mut scores = Vec::new();

    // Peak detection component (0-1)
    let detected_count = targets.iter().filter(|t| t.detected).count();
    let detection_score = detected_count as f64 / targets.len() as f64;
    scores.push(detection_score);

    // Peak width consistency (0-1)
    let fwhm_values: Vec<f64> = targets.iter().filter_map(|t| t.peak_width_fwhm).collect();

    if fwhm_values.len() >= 2 {
        let mean_fwhm = fwhm_values.iter().sum::<f64>() / fwhm_values.len() as f64;
        let cv = if mean_fwhm > 0.0 {
            let variance = fwhm_values
                .iter()
                .map(|v| (v - mean_fwhm).powi(2))
                .sum::<f64>()
                / fwhm_values.len() as f64;
            variance.sqrt() / mean_fwhm
        } else {
            1.0
        };
        // CV of 0 -> score 1, CV of 1 -> score 0
        let width_score = (1.0 - cv).clamp(0.0, 1.0);
        scores.push(width_score);
    }

    // Peak symmetry component (0-1)
    let symmetry_values: Vec<f64> = targets.iter().filter_map(|t| t.peak_symmetry).collect();

    if !symmetry_values.is_empty() {
        // Ideal symmetry is 1.0; score decreases as symmetry deviates
        let mean_symmetry = symmetry_values.iter().sum::<f64>() / symmetry_values.len() as f64;
        let symmetry_score = (1.0 - (mean_symmetry - 1.0).abs()).clamp(0.0, 1.0);
        scores.push(symmetry_score);
    }

    // Mass accuracy component (0-1)
    let mass_errors: Vec<f64> = targets
        .iter()
        .filter_map(|t| t.mass_error_ppm)
        .map(|e| e.abs())
        .collect();

    if !mass_errors.is_empty() {
        let mean_error = mass_errors.iter().sum::<f64>() / mass_errors.len() as f64;
        // 0 ppm -> score 1, 10 ppm -> score 0
        let mass_score = (1.0 - mean_error / 10.0).clamp(0.0, 1.0);
        scores.push(mass_score);
    }

    // Weighted average of all components
    if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    }
}

/// Identify outlier targets based on deviation from expected values.
pub fn identify_outliers(
    targets: &[TargetMetrics],
    rt_threshold_minutes: f64,
    _area_fold_change_threshold: f64,
) -> Vec<String> {
    let mut outliers = Vec::new();

    for target in targets {
        let mut is_outlier = false;

        // Check RT deviation
        if let Some(rt_delta) = target.rt_delta {
            if rt_delta.abs() > rt_threshold_minutes {
                is_outlier = true;
            }
        }

        // Check area (would need baseline for fold change)
        // For now, flag if area is zero when peak is expected
        if target.detected && target.peak_area == 0.0 {
            is_outlier = true;
        }

        if is_outlier {
            outliers.push(target.target_id.clone());
        }
    }

    outliers
}

/// Calculate summary statistics for a metric across targets.
pub struct MetricSummary {
    pub count: usize,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub median: f64,
}

impl MetricSummary {
    pub fn from_values(mut values: Vec<f64>) -> Option<Self> {
        if values.is_empty() {
            return None;
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;

        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        let min = values[0];
        let max = values[count - 1];

        let median = if count.is_multiple_of(2) {
            (values[count / 2 - 1] + values[count / 2]) / 2.0
        } else {
            values[count / 2]
        };

        Some(Self {
            count,
            mean,
            std_dev,
            min,
            max,
            median,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_summary() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let summary = MetricSummary::from_values(values).unwrap();

        assert_eq!(summary.count, 5);
        assert_eq!(summary.mean, 3.0);
        assert_eq!(summary.min, 1.0);
        assert_eq!(summary.max, 5.0);
        assert_eq!(summary.median, 3.0);
    }

    #[test]
    fn test_chromatography_score_empty() {
        assert_eq!(calculate_chromatography_score(&[]), 0.0);
    }
}
