//! Classify command - preview run classification.

use anyhow::{Context, Result};
use std::path::Path;

use crate::classifier::Classifier;
use crate::config::Config;
use crate::types::{ClassificationConfidence, ClassificationSource, ControlType};

/// Run the classify command.
pub async fn run(path: &str) -> Result<()> {
    let path = Path::new(path);

    if !path.exists() {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    // Load config to find matching instrument
    let config = Config::load().context("Failed to load configuration")?;

    // Find instrument config that matches this path
    let instrument = config
        .instruments
        .iter()
        .find(|i| path.starts_with(&i.watch_path))
        .cloned();

    let classifier = Classifier::new();

    println!();
    println!("Classification Result");
    println!("=====================");
    println!("File: {}", path.display());

    match instrument {
        Some(ref inst) => {
            match classifier.classify(path, inst) {
                Ok(result) => {
                    println!("Control Type: {}", result.control_type);

                    if let Some(ref well) = result.well_position {
                        println!("Well Position: {}", well);
                    } else {
                        println!("Well Position: (not detected)");
                    }

                    println!("Instrument: {}", result.instrument_id);

                    if let Some(ref plate) = result.plate_id {
                        println!("Plate ID: {}", plate);
                    }

                    println!(
                        "Confidence: {}",
                        match result.confidence {
                            ClassificationConfidence::High => "HIGH",
                            ClassificationConfidence::Medium => "MEDIUM",
                            ClassificationConfidence::Low => "LOW",
                        }
                    );

                    println!(
                        "Source: {}",
                        match result.source {
                            ClassificationSource::Filename => "FILENAME",
                            ClassificationSource::Metadata => "METADATA",
                            ClassificationSource::Position => "POSITION",
                            ClassificationSource::Default => "DEFAULT",
                        }
                    );

                    // Show what would happen
                    println!();
                    println!("Processing Decision");
                    println!("-------------------");

                    if result.control_type.is_qc() {
                        println!("Would process: YES");

                        if result.control_type == ControlType::Ssc0 {
                            println!("Action: Register new baseline candidate");
                        } else {
                            println!("Action: Compare against active baseline");
                            // TODO: Look up actual baseline
                            println!("Baseline: (would look up from cloud)");
                        }
                    } else {
                        println!("Would process: NO (SAMPLE runs are skipped by default)");
                    }
                }
                Err(e) => {
                    println!("Classification failed: {}", e);
                    println!();
                    println!("The filename could not be parsed. Expected format:");
                    println!("  {{INSTRUMENT}}_{{CONTROL_TYPE}}_{{WELL}}_{{DATE}}.{{ext}}");
                    println!();
                    println!("Examples:");
                    println!("  TIMSTOF01_SSC0_A1_2026-01-27.d");
                    println!("  EXPLORIS01_QCB_A3_2026-01-27.raw");
                }
            }
        }
        None => {
            // No instrument config, try to classify from filename only
            println!("Instrument: (no matching config found)");
            println!();
            println!("Note: This file's parent directory doesn't match any configured");
            println!("instrument watch_path. Classification from filename only:");
            println!();

            // Extract filename and try to parse
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                let parts: Vec<&str> = filename.split(['_', '-']).collect();

                println!("Filename parts: {:?}", parts);

                // Try to find control type token
                for part in &parts {
                    if let Some(ct) = ControlType::from_token(part) {
                        println!("Detected control type: {} (from token '{}')", ct, part);
                        break;
                    }
                }

                // Try to find well position
                for part in &parts {
                    if let Some(well) = crate::types::WellPosition::from_str(part) {
                        println!("Detected well position: {} (from token '{}')", well, part);
                        break;
                    }
                }
            }
        }
    }

    println!();
    Ok(())
}
