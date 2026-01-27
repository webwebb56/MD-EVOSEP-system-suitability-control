//! Path utilities for the MD Local QC Agent.
//!
//! Defines standard locations for configuration, logs, spool, and templates.

use std::path::PathBuf;

/// Base data directory for the agent.
///
/// On Windows: `C:\ProgramData\MassDynamics\QC`
/// On other platforms: `~/.local/share/massdynamics/qc` (for development)
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\ProgramData\MassDynamics\QC")
    }

    #[cfg(not(windows))]
    {
        // For development on macOS/Linux
        directories::ProjectDirs::from("com", "MassDynamics", "QC")
            .map(|p| p.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".local")
                    .join("share")
                    .join("massdynamics")
                    .join("qc")
            })
    }
}

/// Configuration file path.
///
/// On Windows: `C:\ProgramData\MassDynamics\QC\config.toml`
pub fn config_file() -> PathBuf {
    // Check environment variable first
    if let Ok(path) = std::env::var("MDQC_CONFIG") {
        return PathBuf::from(path);
    }

    data_dir().join("config.toml")
}

/// Log directory.
///
/// On Windows: `C:\ProgramData\MassDynamics\QC\logs`
pub fn log_dir() -> std::io::Result<PathBuf> {
    let path = data_dir().join("logs");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Spool directory.
///
/// On Windows: `C:\ProgramData\MassDynamics\QC\spool`
pub fn spool_dir() -> PathBuf {
    data_dir().join("spool")
}

/// Pending spool directory.
pub fn spool_pending_dir() -> PathBuf {
    spool_dir().join("pending")
}

/// Uploading spool directory.
pub fn spool_uploading_dir() -> PathBuf {
    spool_dir().join("uploading")
}

/// Failed spool directory.
pub fn spool_failed_dir() -> PathBuf {
    spool_dir().join("failed")
}

/// Completed spool directory.
pub fn spool_completed_dir() -> PathBuf {
    spool_dir().join("completed")
}

/// Template directory.
///
/// On Windows: `C:\ProgramData\MassDynamics\QC\templates`
pub fn template_dir() -> PathBuf {
    data_dir().join("templates")
}

/// Ensure all required directories exist.
pub fn ensure_directories() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(log_dir()?)?;
    std::fs::create_dir_all(spool_pending_dir())?;
    std::fs::create_dir_all(spool_uploading_dir())?;
    std::fs::create_dir_all(spool_failed_dir())?;
    std::fs::create_dir_all(spool_completed_dir())?;
    std::fs::create_dir_all(template_dir())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_are_valid() {
        // Just ensure these don't panic
        let _ = data_dir();
        let _ = config_file();
        let _ = spool_dir();
        let _ = template_dir();
    }
}
