//! Run classification based on filename and metadata.
//!
//! Classifies MS runs into control types (SSC0, QC_A, QC_B, SAMPLE, BLANK)
//! based on filename tokens and well positions.

use regex::Regex;
use std::path::Path;
use tracing::{debug, trace};

use crate::config::InstrumentConfig;
use crate::error::ClassificationError;
use crate::types::{
    ClassificationConfidence, ClassificationSource, ControlType, RunClassification, WellPosition,
};

/// Classifier for MS runs.
pub struct Classifier {
    // Pre-compiled regex patterns for control type detection
    ssc0_pattern: Regex,
    qca_pattern: Regex,
    qcb_pattern: Regex,
    blank_pattern: Regex,
    well_pattern: Regex,
}

impl Classifier {
    pub fn new() -> Self {
        // Patterns that match the spec-compliant forms:
        // SSC0, SSC_0, SSC-0, ssc0
        // QCA, QC_A, QC-A, qc_a
        // QCB, QC_B, QC-B, qc_b
        //
        // Note: Rust regex treats _ as a word character, so \b doesn't work
        // at underscore boundaries. We use explicit delimiters instead:
        // (?:^|[_\-\s.]) = start of string OR delimiter before
        // (?:$|[_\-\s.]) = end of string OR delimiter after
        Self {
            ssc0_pattern: Regex::new(r"(?i)(?:^|[_\-\s.])(SSC[_-]?0|SSC)(?:$|[_\-\s.])").unwrap(),
            qca_pattern: Regex::new(r"(?i)(?:^|[_\-\s.])(QC[_-]?A|QCA)(?:$|[_\-\s.])").unwrap(),
            qcb_pattern: Regex::new(r"(?i)(?:^|[_\-\s.])(QC[_-]?B|QCB)(?:$|[_\-\s.])").unwrap(),
            blank_pattern: Regex::new(r"(?i)(?:^|[_\-\s.])(BLANK|BLK)(?:$|[_\-\s.])").unwrap(),
            // Well pattern: letter A-H followed by 1-12, with delimiters
            well_pattern: Regex::new(r"(?i)(?:^|[_\-\s.])([A-H])(1[0-2]|[1-9])(?:$|[_\-\s.])")
                .unwrap(),
        }
    }

    /// Classify a run based on its file path and instrument config.
    pub fn classify(
        &self,
        path: &Path,
        instrument: &InstrumentConfig,
    ) -> Result<RunClassification, ClassificationError> {
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .ok_or_else(|| ClassificationError::FilenameParse(path.display().to_string()))?;

        trace!(filename = %filename, "Classifying run");

        // Extract control type using regex (preserves QC_A, QC_B, etc.)
        let (control_type, ct_source) = self.extract_control_type(filename);

        // Extract well position
        let well_position = self.extract_well_position(filename);

        // Extract plate ID
        let plate_id = self.extract_plate_id(filename);

        // Determine confidence based on how we found the control type
        let confidence = match (&control_type, &well_position, &ct_source) {
            (ct, Some(_), ClassificationSource::Filename) if ct.is_qc() => {
                ClassificationConfidence::High
            }
            (ct, None, ClassificationSource::Filename) if ct.is_qc() => {
                ClassificationConfidence::Medium
            }
            (_, Some(_), ClassificationSource::Position) => {
                // Inferred from well position only
                ClassificationConfidence::Medium
            }
            _ => ClassificationConfidence::Low,
        };

        debug!(
            filename = %filename,
            control_type = %control_type,
            well = ?well_position,
            confidence = ?confidence,
            source = ?ct_source,
            "Classification result"
        );

        Ok(RunClassification {
            control_type,
            well_position,
            instrument_id: instrument.id.clone(),
            plate_id,
            confidence,
            source: ct_source,
        })
    }

    /// Extract control type from filename using regex patterns.
    fn extract_control_type(&self, filename: &str) -> (ControlType, ClassificationSource) {
        // Check patterns in priority order
        if self.ssc0_pattern.is_match(filename) {
            return (ControlType::Ssc0, ClassificationSource::Filename);
        }

        if self.qca_pattern.is_match(filename) {
            return (ControlType::QcA, ClassificationSource::Filename);
        }

        if self.qcb_pattern.is_match(filename) {
            return (ControlType::QcB, ClassificationSource::Filename);
        }

        if self.blank_pattern.is_match(filename) {
            return (ControlType::Blank, ClassificationSource::Filename);
        }

        // Try to infer from well position
        if let Some(well) = self.extract_well_position(filename) {
            let inferred = self.infer_control_type_from_well(&well);
            if inferred != ControlType::Sample {
                return (inferred, ClassificationSource::Position);
            }
        }

        // Default to SAMPLE
        (ControlType::Sample, ClassificationSource::Default)
    }

    /// Extract well position from filename.
    fn extract_well_position(&self, filename: &str) -> Option<WellPosition> {
        if let Some(caps) = self.well_pattern.captures(filename) {
            let row = caps.get(1)?.as_str().chars().next()?.to_ascii_uppercase();
            let col: u8 = caps.get(2)?.as_str().parse().ok()?;
            WellPosition::new(row, col)
        } else {
            None
        }
    }

    /// Extract plate ID from filename (if present).
    fn extract_plate_id(&self, filename: &str) -> Option<String> {
        // Look for patterns like "plate1", "P001", "PLATE_A"
        let plate_pattern = Regex::new(r"(?i)\b(plate[_-]?\w+|plt[_-]?\w+|P\d{2,})\b").ok()?;
        plate_pattern.find(filename).map(|m| m.as_str().to_string())
    }

    /// Infer control type from well position based on EvoSep defaults.
    fn infer_control_type_from_well(&self, well: &WellPosition) -> ControlType {
        // EvoSep defaults:
        // A1, A2 -> QC_A
        // A3, A4 -> QC_B
        if well.row == 'A' {
            match well.column {
                1 | 2 => return ControlType::QcA,
                3 | 4 => return ControlType::QcB,
                _ => {}
            }
        }
        ControlType::Sample
    }
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_classifier() -> Classifier {
        Classifier::new()
    }

    #[test]
    fn test_ssc0_variants() {
        let c = make_classifier();

        // All these should match SSC0
        let variants = [
            "TIMSTOF01_SSC0_A1_2026-01-27.d",
            "TIMSTOF01_SSC_0_A1_2026-01-27.d",
            "TIMSTOF01_SSC-0_A1_2026-01-27.d",
            "TIMSTOF01_ssc0_A1_2026-01-27.d",
            "SSC0_run.raw",
        ];

        for filename in variants {
            let (ct, source) = c.extract_control_type(filename);
            assert_eq!(ct, ControlType::Ssc0, "Failed for: {}", filename);
            assert_eq!(source, ClassificationSource::Filename);
        }
    }

    #[test]
    fn test_qca_variants() {
        let c = make_classifier();

        let variants = [
            "TIMSTOF01_QCA_A1_2026-01-27.d",
            "TIMSTOF01_QC_A_A1_2026-01-27.d",
            "TIMSTOF01_QC-A_A1_2026-01-27.d",
            "TIMSTOF01_qc_a_A1_2026-01-27.d",
            "QCA_run.raw",
        ];

        for filename in variants {
            let (ct, source) = c.extract_control_type(filename);
            assert_eq!(ct, ControlType::QcA, "Failed for: {}", filename);
            assert_eq!(source, ClassificationSource::Filename);
        }
    }

    #[test]
    fn test_qcb_variants() {
        let c = make_classifier();

        let variants = [
            "TIMSTOF01_QCB_A3_2026-01-27.d",
            "TIMSTOF01_QC_B_A3_2026-01-27.d",
            "TIMSTOF01_QC-B_A3_2026-01-27.d",
            "TIMSTOF01_qc_b_A3_2026-01-27.d",
            "QCB_run.raw",
        ];

        for filename in variants {
            let (ct, source) = c.extract_control_type(filename);
            assert_eq!(ct, ControlType::QcB, "Failed for: {}", filename);
            assert_eq!(source, ClassificationSource::Filename);
        }
    }

    #[test]
    fn test_well_position_extraction() {
        let c = make_classifier();

        assert_eq!(
            c.extract_well_position("TIMSTOF01_QCB_A3_2026-01-27.d"),
            Some(WellPosition::new('A', 3).unwrap())
        );
        assert_eq!(
            c.extract_well_position("run_H12_sample.raw"),
            Some(WellPosition::new('H', 12).unwrap())
        );
        assert_eq!(c.extract_well_position("no_well_here.raw"), None);
    }

    #[test]
    fn test_inference_from_well() {
        let c = make_classifier();

        // A1, A2 -> QC_A
        let (ct, source) = c.extract_control_type("TIMSTOF01_A1_2026-01-27.d");
        assert_eq!(ct, ControlType::QcA);
        assert_eq!(source, ClassificationSource::Position);

        // A3, A4 -> QC_B
        let (ct, source) = c.extract_control_type("TIMSTOF01_A3_2026-01-27.d");
        assert_eq!(ct, ControlType::QcB);
        assert_eq!(source, ClassificationSource::Position);

        // Other wells -> SAMPLE (default)
        let (ct, source) = c.extract_control_type("TIMSTOF01_B5_2026-01-27.d");
        assert_eq!(ct, ControlType::Sample);
        assert_eq!(source, ClassificationSource::Default);
    }

    #[test]
    fn test_default_to_sample() {
        let c = make_classifier();

        let (ct, source) = c.extract_control_type("random_file_name.d");
        assert_eq!(ct, ControlType::Sample);
        assert_eq!(source, ClassificationSource::Default);
    }
}
