//! Doctor command - system health checks.

use anyhow::Result;
use std::path::Path;

use crate::config::{self, Config};
use crate::extractor::skyline;

/// ANSI color codes for terminal output.
mod color {
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
}

struct CheckResult {
    status: CheckStatus,
    label: String,
    detail: Option<String>,
}

enum CheckStatus {
    Ok,
    Warning,
    Error,
    NotConfigured,
}

impl CheckResult {
    fn ok(label: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Ok,
            label: label.into(),
            detail: None,
        }
    }

    fn ok_with_detail(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Ok,
            label: label.into(),
            detail: Some(detail.into()),
        }
    }

    fn warning(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Warning,
            label: label.into(),
            detail: Some(detail.into()),
        }
    }

    fn error(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Error,
            label: label.into(),
            detail: Some(detail.into()),
        }
    }

    fn not_configured(label: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::NotConfigured,
            label: label.into(),
            detail: None,
        }
    }

    fn print(&self) {
        let (icon, color) = match self.status {
            CheckStatus::Ok => ("[OK]", color::GREEN),
            CheckStatus::Warning => ("[!!]", color::YELLOW),
            CheckStatus::Error => ("[XX]", color::RED),
            CheckStatus::NotConfigured => ("[--]", color::RESET),
        };

        print!("{}{}{} {}", color, icon, color::RESET, self.label);
        if let Some(ref detail) = self.detail {
            print!(": {}", detail);
        }
        println!();
    }

    fn is_error(&self) -> bool {
        matches!(self.status, CheckStatus::Error)
    }
}

/// Run the doctor command.
pub async fn run() -> Result<()> {
    println!();
    println!(
        "{}MD Local QC Agent - System Health Check{}",
        color::BOLD,
        color::RESET
    );
    println!("{}", "=".repeat(45));
    println!();

    let mut has_errors = false;

    // Agent version
    CheckResult::ok_with_detail("Agent version", env!("CARGO_PKG_VERSION")).print();

    // Configuration
    println!();
    println!("{}Configuration{}", color::BOLD, color::RESET);
    println!("{}", "-".repeat(20));

    let config = match check_config() {
        Ok((result, config)) => {
            result.print();
            Some(config)
        }
        Err(result) => {
            has_errors = result.is_error();
            result.print();
            None
        }
    };

    // Skyline
    println!();
    println!("{}Skyline{}", color::BOLD, color::RESET);
    println!("{}", "-".repeat(20));

    let skyline_checks = check_skyline(config.as_ref());
    for check in &skyline_checks {
        if check.is_error() {
            has_errors = true;
        }
        check.print();
    }

    // Vendor Readers
    println!();
    println!("{}Vendor Readers{}", color::BOLD, color::RESET);
    println!("{}", "-".repeat(20));

    let vendor_checks = check_vendor_readers(config.as_ref());
    for check in &vendor_checks {
        if check.is_error() {
            has_errors = true;
        }
        check.print();
    }

    // Templates
    if let Some(ref config) = config {
        println!();
        println!("{}Templates{}", color::BOLD, color::RESET);
        println!("{}", "-".repeat(20));

        let template_checks = check_templates(config);
        for check in &template_checks {
            if check.is_error() {
                has_errors = true;
            }
            check.print();
        }
    }

    // Instruments
    if let Some(ref config) = config {
        println!();
        println!("{}Instruments{}", color::BOLD, color::RESET);
        println!("{}", "-".repeat(20));

        let instrument_checks = check_instruments(config);
        for check in &instrument_checks {
            if check.is_error() {
                has_errors = true;
            }
            check.print();
        }
    }

    // Certificates
    println!();
    println!("{}Certificates{}", color::BOLD, color::RESET);
    println!("{}", "-".repeat(20));

    let cert_checks = check_certificates(config.as_ref());
    for check in &cert_checks {
        if check.is_error() {
            has_errors = true;
        }
        check.print();
    }

    // Cloud Connectivity
    println!();
    println!("{}Cloud Connectivity{}", color::BOLD, color::RESET);
    println!("{}", "-".repeat(20));

    let cloud_checks = check_cloud_connectivity(config.as_ref()).await;
    for check in &cloud_checks {
        if check.is_error() {
            has_errors = true;
        }
        check.print();
    }

    // Spool
    if let Some(ref config) = config {
        println!();
        println!("{}Spool{}", color::BOLD, color::RESET);
        println!("{}", "-".repeat(20));

        let spool_checks = check_spool(config);
        for check in &spool_checks {
            if check.is_error() {
                has_errors = true;
            }
            check.print();
        }
    }

    // Windows-specific checks
    #[cfg(windows)]
    {
        println!();
        println!("{}Windows Environment{}", color::BOLD, color::RESET);
        println!("{}", "-".repeat(20));

        let windows_checks = check_windows_environment();
        for check in &windows_checks {
            // Windows checks are mostly warnings, not blockers
            check.print();
        }
    }

    // Summary
    println!();
    if has_errors {
        println!(
            "{}Overall: {}UNHEALTHY{} - Some checks failed",
            color::BOLD,
            color::RED,
            color::RESET
        );
    } else {
        println!(
            "{}Overall: {}HEALTHY{}",
            color::BOLD,
            color::GREEN,
            color::RESET
        );
    }
    println!();

    Ok(())
}

fn check_config() -> Result<(CheckResult, Config), CheckResult> {
    let config_path = config::paths::config_file();

    if !config_path.exists() {
        return Err(CheckResult::error(
            "Config file",
            format!("not found at {}", config_path.display()),
        ));
    }

    match Config::load() {
        Ok(config) => Ok((
            CheckResult::ok_with_detail("Config file", config_path.display().to_string()),
            config,
        )),
        Err(e) => Err(CheckResult::error("Config file", format!("invalid: {}", e))),
    }
}

fn check_skyline(config: Option<&Config>) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // Find Skyline
    // Handle "auto" path - treat it as None to trigger auto-discovery
    let skyline_path = if let Some(config) = config {
        config
            .skyline
            .path
            .as_ref()
            .filter(|p| !p.eq_ignore_ascii_case("auto") && !p.is_empty())
            .map(std::path::PathBuf::from)
    } else {
        None
    };

    let skyline_path = skyline_path.or_else(skyline::discover_skyline);

    match skyline_path {
        Some(path) if path.exists() => {
            results.push(CheckResult::ok_with_detail(
                "SkylineCmd.exe",
                path.display().to_string(),
            ));

            // Try to get version
            match skyline::get_version(&path) {
                Ok(version) => {
                    results.push(CheckResult::ok_with_detail("Skyline version", version));
                }
                Err(_) => {
                    results.push(CheckResult::warning(
                        "Skyline version",
                        "could not determine",
                    ));
                }
            }
        }
        Some(path) => {
            results.push(CheckResult::error(
                "SkylineCmd.exe",
                format!("configured path not found: {}", path.display()),
            ));
        }
        None => {
            results.push(CheckResult::error(
                "SkylineCmd.exe",
                "not found (checked registry and common paths)",
            ));
        }
    }

    results
}

fn check_vendor_readers(_config: Option<&Config>) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // Check Thermo reader
    if skyline::check_thermo_reader() {
        results.push(CheckResult::ok("Thermo RawFileReader"));
    } else {
        results.push(CheckResult::warning("Thermo RawFileReader", "not detected"));
    }

    // Check Bruker reader
    if skyline::check_bruker_reader() {
        results.push(CheckResult::ok("Bruker timsdata.dll"));
    } else {
        results.push(CheckResult::warning("Bruker timsdata.dll", "not detected"));
    }

    // Sciex and Waters - not configured by default
    results.push(CheckResult::not_configured("Sciex"));
    results.push(CheckResult::not_configured("Waters"));

    results
}

fn check_templates(config: &Config) -> Vec<CheckResult> {
    let mut results = Vec::new();
    let template_dir = config::paths::template_dir();

    for instrument in &config.instruments {
        let template_path = template_dir.join(&instrument.template);

        if template_path.exists() {
            // Calculate hash
            let hash = match crate::extractor::skyline::hash_template(&template_path) {
                Ok(h) => format!("sha256:{}...", &h[..16]),
                Err(_) => "hash error".to_string(),
            };

            results.push(CheckResult::ok_with_detail(
                &instrument.template,
                format!("found, {}", hash),
            ));
        } else {
            results.push(CheckResult::error(
                &instrument.template,
                format!("not found at {}", template_path.display()),
            ));
        }
    }

    if results.is_empty() {
        results.push(CheckResult::not_configured("No templates configured"));
    }

    results
}

fn check_instruments(config: &Config) -> Vec<CheckResult> {
    let mut results = Vec::new();

    for instrument in &config.instruments {
        let watch_path = Path::new(&instrument.watch_path);

        if watch_path.exists() {
            if watch_path.is_dir() {
                // Check if readable
                match std::fs::read_dir(watch_path) {
                    Ok(_) => {
                        results.push(CheckResult::ok_with_detail(
                            &instrument.id,
                            format!("{} (accessible)", instrument.watch_path),
                        ));
                    }
                    Err(e) => {
                        results.push(CheckResult::error(
                            &instrument.id,
                            format!("{} (not readable: {})", instrument.watch_path, e),
                        ));
                    }
                }
            } else {
                results.push(CheckResult::error(
                    &instrument.id,
                    format!("{} (not a directory)", instrument.watch_path),
                ));
            }
        } else {
            results.push(CheckResult::error(
                &instrument.id,
                format!("{} (path does not exist)", instrument.watch_path),
            ));
        }
    }

    if results.is_empty() {
        results.push(CheckResult::warning(
            "No instruments configured",
            "add [[instruments]] to config",
        ));
    }

    results
}

fn check_certificates(config: Option<&Config>) -> Vec<CheckResult> {
    let mut results = Vec::new();

    let thumbprint = config.and_then(|c| c.cloud.certificate_thumbprint.as_ref());

    match thumbprint {
        Some(thumbprint) => {
            // On Windows, we would check the cert store
            // For now, just validate the thumbprint format
            if thumbprint.len() == 40 && thumbprint.chars().all(|c| c.is_ascii_hexdigit()) {
                results.push(CheckResult::ok_with_detail(
                    "Client certificate",
                    format!("thumbprint {}...", &thumbprint[..8]),
                ));
                // TODO: Actually check cert store and expiry on Windows
            } else {
                results.push(CheckResult::error(
                    "Client certificate",
                    "invalid thumbprint format",
                ));
            }
        }
        None => {
            results.push(CheckResult::warning(
                "Client certificate",
                "not configured (enrollment required)",
            ));
        }
    }

    results
}

async fn check_cloud_connectivity(config: Option<&Config>) -> Vec<CheckResult> {
    let mut results = Vec::new();

    let endpoint = config
        .map(|c| c.cloud.endpoint.as_str())
        .unwrap_or("https://qc-ingest.massdynamics.com/v1/");

    results.push(CheckResult::ok_with_detail("Endpoint", endpoint));

    // Try to reach the endpoint
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();

    match client {
        Ok(client) => {
            let health_url = format!("{}health", endpoint);
            match client.get(&health_url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        results.push(CheckResult::ok("Connectivity"));
                    } else {
                        results.push(CheckResult::warning(
                            "Connectivity",
                            format!("status {}", response.status()),
                        ));
                    }
                }
                Err(e) => {
                    results.push(CheckResult::warning(
                        "Connectivity",
                        format!("unreachable: {}", e),
                    ));
                }
            }
        }
        Err(e) => {
            results.push(CheckResult::error(
                "HTTP client",
                format!("failed to create: {}", e),
            ));
        }
    }

    results
}

fn check_spool(_config: &Config) -> Vec<CheckResult> {
    let mut results = Vec::new();

    let spool_dir = config::paths::spool_dir();

    if spool_dir.exists() {
        // Check if writable
        let test_file = spool_dir.join(".write_test");
        match std::fs::write(&test_file, "test") {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
                results.push(CheckResult::ok("Spool directory writable"));
            }
            Err(e) => {
                results.push(CheckResult::error(
                    "Spool directory",
                    format!("not writable: {}", e),
                ));
            }
        }
    } else {
        // Try to create it
        match std::fs::create_dir_all(&spool_dir) {
            Ok(_) => {
                results.push(CheckResult::ok_with_detail("Spool directory", "created"));
            }
            Err(e) => {
                results.push(CheckResult::error(
                    "Spool directory",
                    format!("cannot create: {}", e),
                ));
            }
        }
    }

    // Count pending items
    let pending_dir = spool_dir.join("pending");
    if pending_dir.exists() {
        let count = std::fs::read_dir(&pending_dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        results.push(CheckResult::ok_with_detail(
            "Pending items",
            count.to_string(),
        ));
    } else {
        results.push(CheckResult::ok_with_detail("Pending items", "0"));
    }

    // Count failed items
    let failed_dir = spool_dir.join("failed");
    if failed_dir.exists() {
        let count = std::fs::read_dir(&failed_dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        if count > 0 {
            results.push(CheckResult::warning(
                "Failed items",
                format!("{} (review required)", count),
            ));
        } else {
            results.push(CheckResult::ok_with_detail("Failed items", "0"));
        }
    } else {
        results.push(CheckResult::ok_with_detail("Failed items", "0"));
    }

    results
}

/// Check Windows-specific environment settings that could cause issues.
#[cfg(windows)]
fn check_windows_environment() -> Vec<CheckResult> {
    let mut results = Vec::new();

    // Check Windows version
    let version_info = get_windows_version();
    results.push(CheckResult::ok_with_detail("Windows version", version_info));

    // Check if Start Menu shortcut exists (needed for notifications)
    let shortcut_path = std::env::var("APPDATA")
        .map(|appdata| {
            std::path::PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("MD QC Agent.lnk")
        })
        .ok();

    if let Some(ref path) = shortcut_path {
        if path.exists() {
            results.push(CheckResult::ok("Start Menu shortcut"));
        } else {
            results.push(CheckResult::warning(
                "Start Menu shortcut",
                "missing (notifications may show as 'PowerShell')",
            ));
        }
    }

    // Check if running with admin rights (usually not needed, but good to know)
    let is_admin = is_running_as_admin();
    if is_admin {
        results.push(CheckResult::ok_with_detail("Running as", "Administrator"));
    } else {
        results.push(CheckResult::ok_with_detail("Running as", "Standard user"));
    }

    // Check long path support
    if long_paths_enabled() {
        results.push(CheckResult::ok("Long path support"));
    } else {
        results.push(CheckResult::warning(
            "Long path support",
            "disabled (paths >260 chars may fail)",
        ));
    }

    // Check if running from Program Files (recommended) or elsewhere
    if let Ok(exe_path) = std::env::current_exe() {
        let exe_str = exe_path.display().to_string().to_lowercase();
        if exe_str.contains("program files") {
            results.push(CheckResult::ok_with_detail(
                "Install location",
                "Program Files (recommended)",
            ));
        } else if exe_str.contains("temp") || exe_str.contains("downloads") {
            results.push(CheckResult::warning(
                "Install location",
                "temporary folder (may cause issues)",
            ));
        } else {
            results.push(CheckResult::ok_with_detail(
                "Install location",
                exe_path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ));
        }
    }

    results
}

/// Get Windows version info.
#[cfg(windows)]
fn get_windows_version() -> String {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") {
        let product_name: String = key.get_value("ProductName").unwrap_or_default();
        let build: String = key.get_value("CurrentBuildNumber").unwrap_or_default();
        let display_version: String = key.get_value("DisplayVersion").unwrap_or_default();

        if !product_name.is_empty() {
            if !display_version.is_empty() {
                format!("{} {} (Build {})", product_name, display_version, build)
            } else {
                format!("{} (Build {})", product_name, build)
            }
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    }
}

/// Check if running with admin privileges (simplified check).
#[cfg(windows)]
fn is_running_as_admin() -> bool {
    // Simple check: try to read a protected registry key
    use winreg::enums::*;
    use winreg::RegKey;

    // This key requires admin to write to (we just read, but it's a hint)
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control")
        .map(|key| {
            // If we can read CurrentUser, we have some privileges
            // Try to check if we have write access by checking key metadata
            key.enum_keys().next().is_some()
        })
        .unwrap_or(false)
}

/// Check if Windows long path support is enabled.
#[cfg(windows)]
fn long_paths_enabled() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey(r"SYSTEM\CurrentControlSet\Control\FileSystem") {
        let value: u32 = key.get_value("LongPathsEnabled").unwrap_or(0);
        value == 1
    } else {
        false
    }
}
