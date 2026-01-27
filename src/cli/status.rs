//! Status command - show agent status and queue.

use anyhow::Result;
use chrono::Utc;

use crate::config::{self, Config};

/// Run the status command.
pub async fn run() -> Result<()> {
    println!();
    println!("Agent Status");
    println!("============");

    // Check if service is running (Windows-specific)
    #[cfg(windows)]
    {
        match check_service_status() {
            ServiceStatus::Running => println!("Service: running"),
            ServiceStatus::Stopped => println!("Service: stopped"),
            ServiceStatus::Unknown => println!("Service: unknown"),
        }
    }

    #[cfg(not(windows))]
    {
        println!("Service: N/A (not on Windows)");
    }

    // Load config
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            println!("Config: error loading - {}", e);
            return Ok(());
        }
    };

    println!("Config: loaded");
    println!("Instruments: {}", config.instruments.len());

    // Show spool status
    println!();
    println!("Queue");
    println!("-----");

    let spool_dir = config::paths::spool_dir();

    let pending_count = count_files(&spool_dir.join("pending"));
    let uploading_count = count_files(&spool_dir.join("uploading"));
    let failed_count = count_files(&spool_dir.join("failed"));

    println!("Pending: {}", pending_count);
    println!("Uploading: {}", uploading_count);
    println!("Failed: {}", failed_count);

    // Show recent activity
    println!();
    println!("Recent Activity");
    println!("---------------");

    let completed_dir = spool_dir.join("completed");
    if completed_dir.exists() {
        let mut entries: Vec<_> = std::fs::read_dir(&completed_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();

        // Sort by modification time, newest first
        entries.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        if entries.is_empty() {
            println!("(no recent activity)");
        } else {
            for entry in entries.into_iter().take(5) {
                let filename = entry.file_name();
                let filename = filename.to_string_lossy();

                let time = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| {
                        let dt: chrono::DateTime<Utc> = t.into();
                        dt.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                // Try to extract original filename from payload
                let display_name = filename.strip_suffix("_payload.json").unwrap_or(&filename);

                println!("{}  {}  uploaded", time, display_name);
            }
        }
    } else {
        println!("(no recent activity)");
    }

    println!();
    Ok(())
}

fn count_files(dir: &std::path::Path) -> usize {
    if dir.exists() {
        std::fs::read_dir(dir)
            .map(|entries| entries.count())
            .unwrap_or(0)
    } else {
        0
    }
}

#[cfg(windows)]
enum ServiceStatus {
    Running,
    Stopped,
    Unknown,
}

#[cfg(windows)]
fn check_service_status() -> ServiceStatus {
    use std::process::Command;

    let output = Command::new("sc")
        .args(["query", "MassDynamicsQC"])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("RUNNING") {
                ServiceStatus::Running
            } else if stdout.contains("STOPPED") {
                ServiceStatus::Stopped
            } else {
                ServiceStatus::Unknown
            }
        }
        Err(_) => ServiceStatus::Unknown,
    }
}
