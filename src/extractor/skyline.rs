//! Skyline discovery and utilities.

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::path::PathBuf;

/// Discover SkylineCmd.exe location.
pub fn discover_skyline() -> Option<PathBuf> {
    // 1. Check registry (Windows)
    #[cfg(windows)]
    {
        if let Some(path) = discover_from_registry() {
            return Some(path);
        }
    }

    // 2. Check common installation paths
    let common_paths = [
        r"C:\Program Files\Skyline\SkylineCmd.exe",
        r"C:\Program Files (x86)\Skyline\SkylineCmd.exe",
        r"C:\Skyline\SkylineCmd.exe",
    ];

    for path in &common_paths {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    // 3. Check PATH
    if let Ok(path) = which::which("SkylineCmd") {
        return Some(path);
    }

    None
}

/// Discover Skyline from Windows registry.
#[cfg(windows)]
fn discover_from_registry() -> Option<PathBuf> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    // Try ProteoWizard registry key
    if let Ok(skyline_key) = hklm.open_subkey(r"SOFTWARE\ProteoWizard\Skyline") {
        if let Ok(install_path) = skyline_key.get_value::<String, _>("InstallPath") {
            let cmd_path = PathBuf::from(&install_path).join("SkylineCmd.exe");
            if cmd_path.exists() {
                return Some(cmd_path);
            }
        }
    }

    // Try alternative registry locations
    let alt_keys = [
        r"SOFTWARE\Skyline",
        r"SOFTWARE\WOW6432Node\ProteoWizard\Skyline",
    ];

    for key_path in &alt_keys {
        if let Ok(key) = hklm.open_subkey(key_path) {
            if let Ok(install_path) = key.get_value::<String, _>("InstallPath") {
                let cmd_path = PathBuf::from(&install_path).join("SkylineCmd.exe");
                if cmd_path.exists() {
                    return Some(cmd_path);
                }
            }
        }
    }

    None
}

/// Get Skyline version.
pub fn get_version(skyline_path: &Path) -> Result<String> {
    use std::process::Command;

    let output = Command::new(skyline_path).arg("--version").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Version might be in stdout or stderr depending on Skyline version
    let version_text = if !stdout.trim().is_empty() {
        stdout.to_string()
    } else {
        stderr.to_string()
    };

    // Extract version number (e.g., "Skyline 24.1.0.198")
    let version = version_text
        .lines()
        .find(|line| line.contains("Skyline") || line.chars().any(|c| c.is_ascii_digit()))
        .map(|line| {
            // Try to extract just the version number
            line.split_whitespace()
                .find(|part| {
                    part.chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                })
                .unwrap_or(line.trim())
        })
        .unwrap_or("unknown")
        .to_string();

    Ok(version)
}

/// Calculate SHA-256 hash of a template file.
pub fn hash_template(template_path: &Path) -> Result<String> {
    let content = std::fs::read(template_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hex::encode(hasher.finalize()))
}

/// Check if Thermo raw reader is available.
pub fn check_thermo_reader() -> bool {
    #[cfg(windows)]
    {
        // Check for MSFileReader or RawFileReader DLLs
        let dll_paths = [
            r"C:\Program Files\Thermo\MSFileReader\XRawfile2.dll",
            r"C:\Program Files (x86)\Thermo\MSFileReader\XRawfile2.dll",
        ];

        for path in &dll_paths {
            if std::path::Path::new(path).exists() {
                return true;
            }
        }

        // Check registry for RawFileReader
        use winreg::enums::*;
        use winreg::RegKey;

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if hklm.open_subkey(r"SOFTWARE\Thermo\MSFileReader").is_ok() {
            return true;
        }
    }

    false
}

/// Check if Bruker reader is available.
pub fn check_bruker_reader() -> bool {
    #[cfg(windows)]
    {
        // Check for timsdata.dll
        let dll_paths = [
            r"C:\Program Files\Bruker Daltonics\timsdata.dll",
            r"C:\Program Files (x86)\Bruker Daltonics\timsdata.dll",
        ];

        for path in &dll_paths {
            if std::path::Path::new(path).exists() {
                return true;
            }
        }

        // Also check if Skyline can find it (it bundles its own sometimes)
        if let Some(skyline_path) = discover_skyline() {
            let skyline_dir = skyline_path.parent();
            if let Some(dir) = skyline_dir {
                let bundled_dll = dir.join("timsdata.dll");
                if bundled_dll.exists() {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_template() {
        // Create a temp file for testing
        use std::io::Write;
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        write!(temp, "test content").unwrap();

        let hash = hash_template(temp.path()).unwrap();
        assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex chars
    }
}
