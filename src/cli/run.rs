//! Run command - main agent execution loop.

use anyhow::Result;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::classifier::Classifier;
use crate::config::Config;
use crate::extractor::Extractor;
use crate::failed_files::FailedFiles;
use crate::spool::Spool;
use crate::types::TrackedFile;
use crate::uploader::Uploader;
use crate::watcher::Watcher;

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

/// Generate a hardware-based agent ID.
fn generate_agent_id() -> String {
    // Try to get a machine-specific ID
    #[cfg(windows)]
    {
        // On Windows, use machine GUID from registry
        use winreg::enums::*;
        use winreg::RegKey;

        if let Ok(hklm) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        {
            if let Ok(guid) = hklm.get_value::<String, _>("MachineGuid") {
                return format!("mdqc-{}", &guid[..8]);
            }
        }
    }

    // Fallback: use hostname + random suffix
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let random: u32 = rand::random();
    format!("mdqc-{}-{:08x}", hostname, random)
}

/// Resolve the agent ID from config or generate one.
fn resolve_agent_id(config: &Config) -> String {
    if config.agent.agent_id == "auto" {
        generate_agent_id()
    } else {
        config.agent.agent_id.clone()
    }
}

/// Main agent processing loop.
pub async fn run_agent(config: Config, shutdown_rx: &mut mpsc::Receiver<()>) -> Result<()> {
    // Initialize components
    let spool = Spool::new(&config.spool)?;
    let failed_files = FailedFiles::new();
    let enable_notifications = config.agent.enable_toast_notifications;

    // Set agent ID
    let agent_id = resolve_agent_id(&config);
    spool.set_agent_id(agent_id.clone()).await;
    info!(agent_id = %agent_id, "Agent ID configured");

    let uploader = Uploader::new(&config.cloud, spool.clone())?;
    let extractor = Extractor::new(&config.skyline)?;
    let classifier = Classifier::new();

    // Create channel for files ready for processing
    let (file_tx, mut file_rx) = mpsc::channel::<TrackedFile>(100);

    // Start watcher for each instrument
    let mut watchers = Vec::new();
    for instrument in &config.instruments {
        let watcher = Watcher::new(instrument.clone(), config.watcher.clone(), file_tx.clone())?;
        watchers.push(watcher);
    }

    // Start all watchers
    for watcher in &watchers {
        watcher.start()?;
    }

    // Start uploader background task
    let uploader_handle = tokio::spawn({
        let uploader = uploader.clone();
        async move { uploader.run().await }
    });

    info!(
        instrument_count = config.instruments.len(),
        agent_id = %agent_id,
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
                let vendor = tracked_file.vendor;
                info!(path = ?file_path, vendor = %vendor, "Processing file");

                // Find the instrument config for this file
                let instrument = config.instruments.iter()
                    .find(|i| file_path.starts_with(&i.watch_path))
                    .cloned();

                let Some(instrument) = instrument else {
                    warn!(path = ?file_path, "No instrument config found for file");
                    continue;
                };

                // Find the watcher to mark done/failed
                let watcher = watchers.iter()
                    .find(|_w| file_path.starts_with(PathBuf::from(&instrument.watch_path)));

                // Classify the run
                let classification = match classifier.classify(&file_path, &instrument) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(path = ?file_path, error = %e, "Classification failed");
                        failed_files.record_failure(
                            file_path.clone(),
                            instrument.id.clone(),
                            format!("Classification failed: {}", e),
                        );
                        if let Some(w) = watcher {
                            w.mark_failed(&file_path);
                        }
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
                    if let Some(w) = watcher {
                        w.mark_done(&file_path);
                    }
                    continue;
                }

                info!(
                    path = ?file_path,
                    control_type = %classification.control_type,
                    confidence = ?classification.confidence,
                    "Run classified"
                );

                // Extract metrics
                let file_name = file_path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                match extractor.extract(&file_path, &instrument, &classification).await {
                    Ok(result) => {
                        info!(
                            path = ?file_path,
                            targets_found = result.run_metrics.targets_found,
                            "Extraction complete"
                        );

                        // Show success notification
                        if enable_notifications {
                            crate::notifications::notify_extraction_success(
                                &file_name,
                                result.run_metrics.targets_found,
                                result.run_metrics.targets_expected,
                            );
                        }

                        // Spool for upload (pass vendor from instrument config)
                        if let Err(e) = spool.enqueue(&result, &classification, instrument.vendor).await {
                            error!(path = ?file_path, error = %e, "Failed to spool result");
                            failed_files.record_failure(
                                file_path.clone(),
                                instrument.id.clone(),
                                format!("Failed to spool result: {}", e),
                            );
                            if let Some(w) = watcher {
                                w.mark_failed(&file_path);
                            }
                        } else if let Some(w) = watcher {
                            w.mark_done(&file_path);
                        }
                    }
                    Err(e) => {
                        error!(path = ?file_path, error = %e, "Extraction failed");

                        // Show failure notification
                        if enable_notifications {
                            crate::notifications::notify_extraction_failure(
                                &file_name,
                                &e.to_string(),
                            );
                        }

                        failed_files.record_failure(
                            file_path.clone(),
                            instrument.id.clone(),
                            format!("Skyline extraction failed: {}", e),
                        );
                        if let Some(w) = watcher {
                            w.mark_failed(&file_path);
                        }
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

use std::path::PathBuf;
