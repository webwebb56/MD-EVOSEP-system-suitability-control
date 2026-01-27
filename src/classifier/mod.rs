//! Run classification based on filename and metadata.
//!
//! Classifies MS runs into control types (SSC0, QC_A, QC_B, SAMPLE, BLANK)
//! based on filename tokens and well positions.

use std::path::Path;
use tracing::{debug, trace};

use crate::config::InstrumentConfig;
use crate::error::ClassificationError;
use crate::types::{
    ClassificationConfidence, ClassificationSource, ControlType, RunClassification,
    WellPosition,
};

/// Classifier for MS runs.
pub struct Classifier {
    // Could hold configuration or pre-compiled patterns in the future
}

impl Classifier {
    pub fn new() -> Self {
        Self {}
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

        // Parse filename into tokens
        let tokens = tokenize_filename(filename);
        debug!(tokens = ?tokens, "Filename tokens");

        // Try to extract control type
        let (control_type, ct_source) = self.extract_control_type(&tokens);

        // Try to extract well position
        let well_position = self.extract_well_position(&tokens);

        // Determine confidence based on how we found the control type
        let confidence = match (&control_type, &well_position, &ct_source) {
            (ct, Some(_), ClassificationSource::Filename) if ct.is_qc() => {
                ClassificationConfidence::High
            }
            (ct, None, ClassificationSource::Filename) if ct.is_qc() => {
                ClassificationConfidence::Medium
            }
            (_, Some(well), ClassificationSource::Position) => {
                // Inferred from well position only
                ClassificationConfidence::Medium
            }
            _ => ClassificationConfidence::Low,
        };

        Ok(RunClassification {
            control_type,
            well_position,
            instrument_id: instrument.id.clone(),
            plate_id: self.extract_plate_id(&tokens),
            confidence,
            source: ct_source,
        })
    }

    /// Extract control type from tokens.
    fn extract_control_type(
        &self,
        tokens: &[String],
    ) -> (ControlType, ClassificationSource) {
        // First, try explicit control type tokens
        for token in tokens {
            if let Some(ct) = ControlType::from_token(token) {
                return (ct, ClassificationSource::Filename);
            }
        }

        // Try to infer from well position
        if let Some(well) = self.extract_well_position(tokens) {
            let inferred = self.infer_control_type_from_well(&well);
            if inferred != ControlType::Sample {
                return (inferred, ClassificationSource::Position);
            }
        }

        // Default to SAMPLE
        (ControlType::Sample, ClassificationSource::Default)
    }

    /// Extract well position from tokens.
    fn extract_well_position(&self, tokens: &[String]) -> Option<WellPosition> {
        for token in tokens {
            if let Some(well) = WellPosition::from_str(token) {
                return Some(well);
            }
        }
        None
    }

    /// Extract plate ID from tokens (if present).
    fn extract_plate_id(&self, tokens: &[String]) -> Option<String> {
        // Look for tokens that look like plate IDs
        // Common patterns: "plate1", "P001", "PLATE_A"
        for token in tokens {
            let lower = token.to_lowercase();
            if lower.starts_with("plate") || lower.starts_with("plt") {
                return Some(token.clone());
            }
        }
        None
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

/// Tokenize a filename into parts.
fn tokenize_filename(filename: &str) -> Vec<String> {
    // Remove extension
    let stem = if let Some(pos) = filename.rfind('.') {
        &filename[..pos]
    } else {
        filename
    };

    // Split on common separators
    stem.split(|c| c == '_' || c == '-' || c == '.')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_filename() {
        let tokens = tokenize_filename("TIMSTOF01_QCB_A3_2026-01-27.d");
        assert_eq!(tokens, vec!["TIMSTOF01", "QCB", "A3", "2026", "01", "27"]);
    }

    #[test]
    fn test_control_type_from_token() {
        assert_eq!(ControlType::from_token("SSC0"), Some(ControlType::Ssc0));
        assert_eq!(ControlType::from_token("QCA"), Some(ControlType::QcA));
        assert_eq!(ControlType::from_token("QC_B"), Some(ControlType::QcB));
        assert_eq!(ControlType::from_token("qcb"), Some(ControlType::QcB));
        assert_eq!(ControlType::from_token("BLANK"), Some(ControlType::Blank));
        assert_eq!(ControlType::from_token("random"), None);
    }

    #[test]
    fn test_well_position_parsing() {
        assert!(WellPosition::from_str("A1").is_some());
        assert!(WellPosition::from_str("A3").is_some());
        assert!(WellPosition::from_str("H12").is_some());
        assert!(WellPosition::from_str("I1").is_none()); // Invalid row
        assert!(WellPosition::from_str("A13").is_none()); // Invalid column
    }
}
