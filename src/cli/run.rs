//! Run command - main agent execution loop.

use anyhow::Result;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{info, error, warn};

use crate::config::Config;
use crate::watcher::Watcher;
use crate::classifier::Classifier;
use crate::extractor::Extractor;
use crate::spool::Spool;
use crate::uploader::Uploader;
use crate::types::TrackedFile;

/// Run the agent in foreground mode.
pub async fn run_foreground() -> Result<()> {
    info!("Running agent in foreground mode");

    // Load configuration
    let config = Config::load()?;
    info!(config_path = ?config.path, "Configuration loaded");

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Spawn shutdown signal handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = signal::ctrl_c().await {
            error!("Failed to listen for ctrl-c: {}", e);
        }
        info!("Received shutdown signal");
        let _ = shutdown_tx_clone.send(()).await;
    });

    // Run the main agent loop
    run_agent(config, &mut shutdown_rx).await
}

/// Main agent processing loop.
pub async fn run_agent(config: Config, shutdown_rx: &mut mpsc::Receiver<()>) -> Result<()> {
    // Initialize components
    let spool = Spool::new(&config.spool)?;
    let uploader = Uploader::new(&config.cloud, spool.clone())?;
    let extractor = Extractor::new(&config.skyline)?;
    let classifier = Classifier::new();

    // Create channel for files ready for processing
    let (file_tx, mut file_rx) = mpsc::channel::<TrackedFile>(100);

    // Start watcher for each instrument
    let mut watchers = Vec::new();
    for instrument in &config.instruments {
        let watcher = Watcher::new(
            instrument.clone(),
            config.watcher.clone(),
            file_tx.clone(),
        )?;
        watchers.push(watcher);
    }

    // Start all watchers
    for watcher in &watchers {
        watcher.start()?;
    }

    // Start uploader background task
    let uploader_handle = tokio::spawn({
        let uploader = uploader.clone();
        async move {
            uploader.run().await
        }
    });

    info!(
        instrument_count = config.instruments.len(),
        "Agent started, watching for QC runs"
    );

    // Main processing loop
    loop {
        tokio::select! {
            // Check for shutdown
            _ = shutdown_rx.recv() => {
                info!("Shutdown requested, stopping agent");
                break;
            }

            // Process incoming files
            Some(tracked_file) = file_rx.recv() => {
                let file_path = tracked_file.path.clone();
                info!(path = ?file_path, "Processing file");

                // Find the instrument config for this file
                let instrument = config.instruments.iter()
                    .find(|i| file_path.starts_with(&i.watch_path))
                    .cloned();

                let Some(instrument) = instrument else {
                    warn!(path = ?file_path, "No instrument config found for file");
                    continue;
                };

                // Classify the run
                let classification = match classifier.classify(&file_path, &instrument) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(path = ?file_path, error = %e, "Classification failed");
                        continue;
                    }
                };

                // Skip SAMPLE runs unless configured otherwise
                if !classification.control_type.is_qc() {
                    info!(
                        path = ?file_path,
                        control_type = %classification.control_type,
                        "Skipping non-QC run"
                    );
                    continue;
                }

                info!(
                    path = ?file_path,
                    control_type = %classification.control_type,
                    confidence = ?classification.confidence,
                    "Run classified"
                );

                // Extract metrics
                match extractor.extract(&file_path, &instrument, &classification).await {
                    Ok(result) => {
                        info!(
                            path = ?file_path,
                            targets_found = result.run_metrics.targets_found,
                            "Extraction complete"
                        );

                        // Spool for upload
                        if let Err(e) = spool.enqueue(&result, &classification).await {
                            error!(path = ?file_path, error = %e, "Failed to spool result");
                        }
                    }
                    Err(e) => {
                        error!(path = ?file_path, error = %e, "Extraction failed");
                        // TODO: Spool error record for cloud notification
                    }
                }
            }
        }
    }

    // Cleanup
    info!("Stopping watchers");
    for watcher in watchers {
        watcher.stop()?;
    }

    info!("Stopping uploader");
    uploader_handle.abort();

    info!("Agent stopped");
    Ok(())
}
