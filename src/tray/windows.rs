//! Windows system tray implementation.

use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use crate::config;
use crate::extractor::skyline;

/// Mutex name for single instance check (per-user to avoid cross-privilege conflicts)
const SINGLE_INSTANCE_MUTEX: &str = "Local\\MassDynamicsQCAgent";

/// GitHub releases URL for update checks
const RELEASES_URL: &str =
    "https://github.com/webwebb56/MD-EVOSEP-system-suitability-control/releases";

/// Menu item IDs
mod menu_ids {
    pub const STATUS: &str = "status";
    pub const HEALTH_STATUS: &str = "health_status";
    pub const INSTRUMENT_COUNT: &str = "instrument_count";
    pub const OPEN_CONFIG: &str = "open_config";
    pub const OPEN_LOGS: &str = "open_logs";
    pub const OPEN_TEMPLATE: &str = "open_template";
    pub const OPEN_DATA_FOLDER: &str = "open_data_folder";
    pub const DOCTOR: &str = "doctor";
    pub const CHECK_UPDATES: &str = "check_updates";
    pub const EXIT: &str = "exit";
}

/// Result of a startup health check
#[derive(Debug)]
struct HealthCheckResult {
    is_healthy: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl HealthCheckResult {
    fn new() -> Self {
        Self {
            is_healthy: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn add_error(&mut self, msg: impl Into<String>) {
        self.is_healthy = false;
        self.errors.push(msg.into());
    }

    fn add_warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    fn summary(&self) -> String {
        if self.is_healthy && self.warnings.is_empty() {
            "All systems operational".to_string()
        } else if self.is_healthy {
            format!("{} warning(s)", self.warnings.len())
        } else {
            format!(
                "{} error(s), {} warning(s)",
                self.errors.len(),
                self.warnings.len()
            )
        }
    }

    #[allow(dead_code)]
    fn details(&self) -> String {
        let mut lines = Vec::new();
        for err in &self.errors {
            lines.push(format!("ERROR: {}", err));
        }
        for warn in &self.warnings {
            lines.push(format!("Warning: {}", warn));
        }
        lines.join("\n")
    }
}

/// Application state for the tray icon
struct TrayApp {
    tray_icon: Option<TrayIcon>,
    running: Arc<AtomicBool>,
    health_status: Option<HealthCheckResult>,
}

impl TrayApp {
    fn new() -> Self {
        Self {
            tray_icon: None,
            running: Arc::new(AtomicBool::new(true)),
            health_status: None,
        }
    }

    /// Run startup health checks
    fn run_health_check(&mut self) -> &HealthCheckResult {
        let mut result = HealthCheckResult::new();

        // Check 1: Configuration file exists and is valid
        let config_path = config::paths::config_file();

        if !config_path.exists() {
            result.add_error(
                "Configuration file not found. Right-click tray icon to edit configuration.",
            );
            self.health_status = Some(result);
            return self.health_status.as_ref().unwrap();
        }

        let config = match config::Config::load() {
            Ok(c) => c,
            Err(e) => {
                result.add_error(format!("Invalid configuration: {}", e));
                self.health_status = Some(result);
                return self.health_status.as_ref().unwrap();
            }
        };

        // Check 2: Skyline is installed
        let skyline_path = config
            .skyline
            .path
            .as_ref()
            .map(std::path::PathBuf::from)
            .or_else(skyline::discover_skyline);

        match skyline_path {
            Some(path) if path.exists() => {
                // Skyline found - good
            }
            Some(path) => {
                result.add_error(format!(
                    "Skyline not found at configured path: {}",
                    path.display()
                ));
            }
            None => {
                result.add_error("Skyline not found. Install from skyline.ms");
            }
        }

        // Check 3: At least one instrument configured
        if config.instruments.is_empty() {
            result.add_warning("No instruments configured");
        }

        // Check 4: Watch paths exist
        for instrument in &config.instruments {
            let watch_path = Path::new(&instrument.watch_path);
            if !watch_path.exists() {
                result.add_error(format!(
                    "{}: Watch path does not exist: {}",
                    instrument.id, instrument.watch_path
                ));
            } else if !watch_path.is_dir() {
                result.add_error(format!("{}: Watch path is not a directory", instrument.id));
            }
        }

        // Check 5: Templates exist
        for instrument in &config.instruments {
            let template_path = Path::new(&instrument.template);
            // Check if it's an absolute path or relative to template dir
            let full_path = if template_path.is_absolute() {
                template_path.to_path_buf()
            } else {
                config::paths::data_dir()
                    .join("templates")
                    .join(&instrument.template)
            };

            if !full_path.exists() {
                result.add_warning(format!(
                    "{}: Template not found: {}",
                    instrument.id, instrument.template
                ));
            }
        }

        // Check 6: API token configured (warning only)
        if config.cloud.api_token.is_none()
            || config
                .cloud
                .api_token
                .as_ref()
                .map(|t| t.is_empty())
                .unwrap_or(true)
        {
            result.add_warning("API token not configured - uploads will fail");
        }

        self.health_status = Some(result);
        self.health_status.as_ref().unwrap()
    }

    /// Show a Windows notification/balloon tip
    fn show_notification(&self, title: &str, message: &str) {
        if let Some(ref _tray) = self.tray_icon {
            // tray-icon doesn't have built-in notification support
            // We'll use a simple message box for errors, or just print to console
            // For a proper implementation, we'd use win32 toast notifications
            println!("[{}] {}", title, message);
        }
    }

    fn create_menu(&self) -> Result<Menu> {
        let menu = Menu::new();

        // Status item (disabled, just shows info)
        let version = env!("CARGO_PKG_VERSION");
        let status_item = MenuItem::with_id(
            menu_ids::STATUS,
            format!("MD QC Agent v{}", version),
            false,
            None,
        );
        menu.append(&status_item)?;

        // Health status from startup check
        let health_text = match &self.health_status {
            Some(health) if health.is_healthy && health.warnings.is_empty() => {
                "Status: Ready".to_string()
            }
            Some(health) if health.is_healthy => {
                format!("Status: {} warning(s)", health.warnings.len())
            }
            Some(health) => {
                format!("Status: {} error(s)", health.errors.len())
            }
            None => "Status: Checking...".to_string(),
        };
        let health_item = MenuItem::with_id(menu_ids::HEALTH_STATUS, &health_text, false, None);
        menu.append(&health_item)?;

        // Try to load config and show instrument count
        let instrument_text = self.get_instrument_status();
        let instrument_item =
            MenuItem::with_id(menu_ids::INSTRUMENT_COUNT, &instrument_text, false, None);
        menu.append(&instrument_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Settings section
        let config_item =
            MenuItem::with_id(menu_ids::OPEN_CONFIG, "Edit Configuration...", true, None);
        menu.append(&config_item)?;

        let template_item = MenuItem::with_id(
            menu_ids::OPEN_TEMPLATE,
            "Open Skyline Template...",
            true,
            None,
        );
        menu.append(&template_item)?;

        let data_folder_item = MenuItem::with_id(
            menu_ids::OPEN_DATA_FOLDER,
            "Open Watch Folder...",
            true,
            None,
        );
        menu.append(&data_folder_item)?;

        let logs_item = MenuItem::with_id(menu_ids::OPEN_LOGS, "View Logs...", true, None);
        menu.append(&logs_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Diagnostics
        let doctor_item = MenuItem::with_id(menu_ids::DOCTOR, "Run Diagnostics...", true, None);
        menu.append(&doctor_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Check for Updates
        let updates_item =
            MenuItem::with_id(menu_ids::CHECK_UPDATES, "Check for Updates...", true, None);
        menu.append(&updates_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Exit
        let exit_item = MenuItem::with_id(menu_ids::EXIT, "Exit", true, None);
        menu.append(&exit_item)?;

        Ok(menu)
    }

    fn get_instrument_status(&self) -> String {
        let config_path = config::paths::config_file();

        if !config_path.exists() {
            return "No configuration found".to_string();
        }

        match config::Config::load() {
            Ok(cfg) => {
                let count = cfg.instruments.len();
                if count == 0 {
                    "No instruments configured".to_string()
                } else if count == 1 {
                    format!("Watching: {}", cfg.instruments[0].id)
                } else {
                    format!("Watching {} instruments", count)
                }
            }
            Err(_) => "Configuration error".to_string(),
        }
    }

    fn create_icon(&self) -> Result<tray_icon::Icon> {
        // Load the embedded PNG icon (Mass Dynamics logo)
        const ICON_PNG: &[u8] = include_bytes!("../../assets/icon.png");

        // Decode the PNG
        let img = image::load_from_memory(ICON_PNG)
            .map_err(|e| anyhow::anyhow!("Failed to decode icon: {}", e))?;

        // Resize to 32x32 for tray icon (standard size)
        let img = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);

        // Convert to RGBA
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let raw_data = rgba.into_raw();

        let icon = tray_icon::Icon::from_rgba(raw_data, width, height)
            .map_err(|e| anyhow::anyhow!("Failed to create icon: {}", e))?;

        Ok(icon)
    }

    fn handle_menu_event(&self, event: MenuEvent) {
        match event.id.0.as_str() {
            menu_ids::OPEN_CONFIG => {
                if let Err(e) = self.open_config() {
                    eprintln!("Failed to open config: {}", e);
                }
            }
            menu_ids::OPEN_LOGS => {
                if let Err(e) = self.open_logs() {
                    eprintln!("Failed to open logs: {}", e);
                }
            }
            menu_ids::OPEN_TEMPLATE => {
                if let Err(e) = self.open_template() {
                    eprintln!("Failed to open template: {}", e);
                }
            }
            menu_ids::OPEN_DATA_FOLDER => {
                if let Err(e) = self.open_data_folder() {
                    eprintln!("Failed to open data folder: {}", e);
                }
            }
            menu_ids::DOCTOR => {
                if let Err(e) = self.run_doctor() {
                    eprintln!("Failed to run doctor: {}", e);
                }
            }
            menu_ids::CHECK_UPDATES => {
                open_url(RELEASES_URL);
            }
            menu_ids::EXIT => {
                self.running.store(false, Ordering::SeqCst);
            }
            _ => {}
        }
    }

    fn open_config(&self) -> Result<()> {
        // Launch the GUI configuration editor
        let exe_path = std::env::current_exe()?;
        std::process::Command::new(&exe_path).arg("gui").spawn()?;
        Ok(())
    }

    fn open_logs(&self) -> Result<()> {
        let log_dir = config::paths::log_dir()?;
        std::fs::create_dir_all(&log_dir)?;
        std::process::Command::new("explorer")
            .arg(&log_dir)
            .spawn()?;
        Ok(())
    }

    fn open_template(&self) -> Result<()> {
        // Try to load config and find template path
        let config_path = config::paths::config_file();
        if config_path.exists() {
            if let Ok(cfg) = config::Config::load() {
                // Get the first instrument's template
                if let Some(instrument) = cfg.instruments.first() {
                    let template_path = std::path::Path::new(&instrument.template);
                    if template_path.exists() {
                        // Open with Skyline if available
                        if let Some(skyline_path) = find_skyline() {
                            let skyline_exe = skyline_path.with_file_name("Skyline.exe");
                            if skyline_exe.exists() {
                                std::process::Command::new(&skyline_exe)
                                    .arg(template_path)
                                    .spawn()?;
                                return Ok(());
                            }
                        }
                        // Fallback: open with default handler
                        std::process::Command::new("cmd")
                            .args(["/c", "start", "", template_path.to_str().unwrap_or("")])
                            .spawn()?;
                        return Ok(());
                    }
                }
            }
        }

        // Open templates directory instead
        let templates_dir = config::paths::data_dir().join("templates");
        std::fs::create_dir_all(&templates_dir)?;
        std::process::Command::new("explorer")
            .arg(&templates_dir)
            .spawn()?;
        Ok(())
    }

    fn open_data_folder(&self) -> Result<()> {
        // Try to load config and find watch path
        let config_path = config::paths::config_file();
        if config_path.exists() {
            if let Ok(cfg) = config::Config::load() {
                // Get the first instrument's watch path
                if let Some(instrument) = cfg.instruments.first() {
                    let watch_path = std::path::Path::new(&instrument.watch_path);
                    if watch_path.exists() {
                        std::process::Command::new("explorer")
                            .arg(watch_path)
                            .spawn()?;
                        return Ok(());
                    }
                }
            }
        }

        // Fallback: open user's documents
        if let Ok(docs) = std::env::var("USERPROFILE") {
            let docs_path = std::path::Path::new(&docs).join("Documents");
            std::process::Command::new("explorer")
                .arg(&docs_path)
                .spawn()?;
        }
        Ok(())
    }

    fn run_doctor(&self) -> Result<()> {
        // Open a command prompt and run mdqc doctor
        let exe_path = std::env::current_exe()?;
        // Use cmd /c start to open a new console window that stays open
        std::process::Command::new("cmd")
            .args([
                "/c",
                "start",
                "MD QC Diagnostics",
                "cmd",
                "/k",
                exe_path.to_str().unwrap_or("mdqc"),
                "doctor",
            ])
            .spawn()?;
        Ok(())
    }
}

impl ApplicationHandler for TrayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        // Create tray icon on first resume
        if self.tray_icon.is_none() {
            // Run health check first
            println!("Running startup health check...");
            let health = self.run_health_check();

            // Print health check results to console and show message box for errors
            if health.is_healthy {
                if health.warnings.is_empty() {
                    println!("Health check: PASSED");
                } else {
                    println!("Health check: PASSED with warnings");
                    for warn in &health.warnings {
                        println!("  Warning: {}", warn);
                    }
                }
            } else {
                println!("Health check: FAILED");
                for err in &health.errors {
                    println!("  Error: {}", err);
                }
                for warn in &health.warnings {
                    println!("  Warning: {}", warn);
                }

                // Show message box for critical errors
                let error_msg = format!(
                    "MD QC Agent detected configuration issues:\n\n{}\n\nRight-click the tray icon and select 'Edit Configuration...' to fix.",
                    health.errors.join("\n")
                );
                show_message_box("MD QC Agent - Setup Required", &error_msg, true);
            }

            let menu = match self.create_menu() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Failed to create menu: {}", e);
                    return;
                }
            };

            let icon = match self.create_icon() {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Failed to create icon: {}", e);
                    return;
                }
            };

            // Set tooltip based on health status
            let tooltip = match &self.health_status {
                Some(h) if h.is_healthy => "MD QC Agent - Ready",
                Some(_) => "MD QC Agent - Issues detected (right-click for details)",
                None => "MD QC Agent",
            };

            let tray_icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(tooltip)
                .with_icon(icon)
                .build();

            match tray_icon {
                Ok(ti) => {
                    self.tray_icon = Some(ti);
                    println!("System tray icon created successfully");

                    // Show notification if there are issues
                    if let Some(ref health) = self.health_status {
                        if !health.is_healthy {
                            self.show_notification(
                                "MD QC Agent",
                                &format!("Setup incomplete: {}", health.summary()),
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to create tray icon: {}", e);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
        // We don't have any windows, just the tray icon
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Process menu events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            self.handle_menu_event(event);
        }

        // Check if we should exit
        if !self.running.load(Ordering::SeqCst) {
            event_loop.exit();
        }

        // Keep running
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// Find Skyline installation
fn find_skyline() -> Option<std::path::PathBuf> {
    // Check common locations
    let common_paths = [
        r"C:\Program Files\Skyline\SkylineCmd.exe",
        r"C:\Program Files (x86)\Skyline\SkylineCmd.exe",
    ];

    for path in &common_paths {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }

    // Check ClickOnce location
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let apps_dir = std::path::Path::new(&local_app_data)
            .join("Apps")
            .join("2.0");
        if apps_dir.exists() {
            // Search for SkylineCmd.exe in the ClickOnce deployment
            if let Ok(entries) = glob::glob(&format!("{}/**/SkylineCmd.exe", apps_dir.display())) {
                if let Some(entry) = entries.flatten().next() {
                    return Some(entry);
                }
            }
        }
    }

    None
}

/// Show a Windows message box
fn show_message_box(title: &str, message: &str, is_error: bool) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Convert strings to wide strings for Windows API
    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();

    // MB_OK = 0, MB_ICONERROR = 0x10, MB_ICONINFORMATION = 0x40
    let flags: u32 = if is_error { 0x10 } else { 0x40 };

    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            0, // HWND is isize, 0 = no parent window
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            flags,
        );
    }
}

/// Check if another instance is already running
fn check_single_instance() -> Option<SingleInstanceGuard> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;

    let mutex_name: Vec<u16> = OsStr::new(SINGLE_INSTANCE_MUTEX)
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe {
        let handle = windows_sys::Win32::System::Threading::CreateMutexW(
            null(), // SECURITY_ATTRIBUTES pointer
            1,      // bInitialOwner = TRUE
            mutex_name.as_ptr(),
        );

        // HANDLE is isize, 0 means failure
        if handle == 0 {
            return None;
        }

        let last_error = windows_sys::Win32::Foundation::GetLastError();

        // ERROR_ALREADY_EXISTS = 183
        if last_error == 183 {
            // Another instance is running
            windows_sys::Win32::Foundation::CloseHandle(handle);
            return None;
        }

        Some(SingleInstanceGuard { handle })
    }
}

/// Guard that releases the mutex when dropped
struct SingleInstanceGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Create default config file if it doesn't exist
fn ensure_config_exists() -> bool {
    let config_path = config::paths::config_file();

    if config_path.exists() {
        return true;
    }

    // Create parent directory
    if let Some(parent) = config_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Failed to create config directory: {}", e);
            return false;
        }
    }

    // Default config template
    let default_config = r#"# MD QC Agent Configuration
# Edit this file to configure your instruments.
# After editing, the changes will take effect on next restart.

[agent]
agent_id = "auto"
log_level = "info"

[cloud]
endpoint = "https://qc.massdynamics.com/api/"
# Get your API token from Mass Dynamics account settings
api_token = ""

[skyline]
path = "auto"
timeout_seconds = 300

[watcher]
scan_interval_seconds = 30
stability_window_seconds = 60

# Configure your instrument(s) below:
# Uncomment and edit the following section:

# [[instruments]]
# id = "MY_INSTRUMENT"
# vendor = "thermo"           # thermo, bruker, sciex, waters, or agilent
# watch_path = "D:\\Data"     # Folder where raw files are saved
# file_pattern = "*.raw"      # File pattern to watch
# template = "C:\\ProgramData\\MassDynamics\\QC\\templates\\my_template.sky"
"#;

    match std::fs::write(&config_path, default_config) {
        Ok(_) => {
            println!("Created default config at: {}", config_path.display());
            true
        }
        Err(e) => {
            eprintln!("Failed to create default config: {}", e);
            false
        }
    }
}

/// Open URL in default browser
fn open_url(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn();
}

/// Run the system tray application
pub async fn run_tray() -> Result<()> {
    // Wrap in inner function to catch errors and show message box
    match run_tray_inner().await {
        Ok(()) => Ok(()),
        Err(e) => {
            show_message_box(
                "MD QC Agent - Startup Error",
                &format!("Failed to start MD QC Agent:\n\n{}\n\nPlease check the logs or run 'mdqc doctor' for diagnostics.", e),
                true,
            );
            Err(e)
        }
    }
}

async fn run_tray_inner() -> Result<()> {
    // Check for single instance
    let _guard = match check_single_instance() {
        Some(guard) => guard,
        None => {
            show_message_box(
                "MD QC Agent",
                "MD QC Agent is already running.\n\nLook for the icon in your system tray.",
                false,
            );
            return Ok(());
        }
    };

    // Ensure config exists (create default if needed)
    ensure_config_exists();

    // Ensure directories exist
    let _ = config::paths::ensure_directories();

    println!("Starting MD QC Agent system tray...");
    println!("Right-click the tray icon for options.");

    let event_loop = EventLoop::new()?;
    let mut app = TrayApp::new();

    event_loop.run_app(&mut app)?;

    Ok(())
}
