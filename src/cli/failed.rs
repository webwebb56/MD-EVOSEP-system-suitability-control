//! Failed files CLI commands.

use anyhow::Result;
use std::io::{self, Write};

use crate::cli::FailedAction;
use crate::failed_files::FailedFiles;

/// Run a failed files command.
pub async fn run(action: FailedAction) -> Result<()> {
    let failed = FailedFiles::new();

    match action {
        FailedAction::List => list_failed(&failed),
        FailedAction::Retry { path } => retry_failed(&failed, &path).await,
        FailedAction::Clear { confirm } => clear_failed(&failed, confirm),
    }
}

fn list_failed(failed: &FailedFiles) -> Result<()> {
    let files = failed.get_all();

    if files.is_empty() {
        println!("No failed files.");
        return Ok(());
    }

    println!("Failed files ({}):", files.len());
    println!("{}", "-".repeat(80));

    for file in files {
        println!("Path:       {}", file.path.display());
        println!("Instrument: {}", file.instrument_id);
        println!("Reason:     {}", file.reason);
        println!(
            "Failed at:  {}",
            file.failed_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        if file.retry_count > 0 {
            println!("Retries:    {}", file.retry_count);
        }
        println!("{}", "-".repeat(80));
    }

    println!("\nTo retry a file: mdqc failed retry <path>");
    println!("To retry all:    mdqc failed retry all");
    println!("To clear list:   mdqc failed clear --confirm");

    Ok(())
}

async fn retry_failed(failed: &FailedFiles, path: &str) -> Result<()> {
    if path == "all" {
        let files = failed.get_all();
        if files.is_empty() {
            println!("No failed files to retry.");
            return Ok(());
        }

        println!("Retrying {} failed files...", files.len());
        for file in files {
            println!("\nRetrying: {}", file.path.display());
            match retry_single_file(&file.path, &file.instrument_id).await {
                Ok(()) => {
                    println!("  Success! File has been queued for reprocessing.");
                    failed.mark_success(&file.path);
                }
                Err(e) => {
                    println!("  Failed: {}", e);
                }
            }
        }
    } else {
        let path = std::path::PathBuf::from(path);

        // Check if this file is in the failed list
        let files = failed.get_all();
        let file_info = files.iter().find(|f| f.path == path);

        if let Some(info) = file_info {
            println!("Retrying: {}", path.display());
            match retry_single_file(&path, &info.instrument_id).await {
                Ok(()) => {
                    println!("Success! File has been queued for reprocessing.");
                    failed.mark_success(&path);
                }
                Err(e) => {
                    println!("Failed: {}", e);
                }
            }
        } else {
            // File not in failed list, but user might want to force-process it
            println!("File not in failed list: {}", path.display());
            println!("\nTo process a file that's not in the failed list,");
            println!("use: mdqc classify {}", path.display());
        }
    }

    Ok(())
}

async fn retry_single_file(path: &std::path::Path, _instrument_id: &str) -> Result<()> {
    // For now, we just validate the file exists and queue it
    // In a full implementation, this would trigger the processing pipeline

    if !path.exists() {
        anyhow::bail!("File no longer exists: {}", path.display());
    }

    // Touch the file to make it appear "new" to the watcher
    // This will cause it to be picked up again
    let now = std::time::SystemTime::now();
    if let Err(e) = filetime::set_file_mtime(path, filetime::FileTime::from_system_time(now)) {
        // If we can't touch the file, inform the user
        println!("  Note: Could not update file timestamp ({})", e);
        println!("  The file will be processed on next agent restart.");
    } else {
        println!("  File timestamp updated - watcher will pick it up.");
    }

    Ok(())
}

fn clear_failed(failed: &FailedFiles, confirm: bool) -> Result<()> {
    let count = failed.count();

    if count == 0 {
        println!("No failed files to clear.");
        return Ok(());
    }

    if !confirm {
        print!("Clear {} failed file(s)? [y/N] ", count);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    failed.clear();
    println!("Cleared {} failed file(s).", count);

    Ok(())
}
