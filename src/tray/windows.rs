//! Windows system tray implementation.

use anyhow::Result;
use std::os::windows::process::CommandExt;
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
    pub const FAILED_FILES: &str = "failed_files";
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
        // Handle "auto" path - treat it as None to trigger auto-discovery
        let skyline_path = config
            .skyline
            .path
            .as_ref()
            .filter(|p| !p.eq_ignore_ascii_case("auto") && !p.is_empty())
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

        // Failed files (show count if any)
        let failed_count = crate::failed_files::FailedFiles::new().count();
        let failed_text = if failed_count > 0 {
            format!("View Failed Files ({})...", failed_count)
        } else {
            "View Failed Files...".to_string()
        };
        let failed_item = MenuItem::with_id(menu_ids::FAILED_FILES, &failed_text, true, None);
        menu.append(&failed_item)?;

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
        let id = event.id.0.as_str();

        let result: Result<()> = match id {
            menu_ids::OPEN_CONFIG => self.open_config(),
            menu_ids::OPEN_LOGS => self.open_logs(),
            menu_ids::OPEN_TEMPLATE => self.open_template(),
            menu_ids::OPEN_DATA_FOLDER => self.open_data_folder(),
            menu_ids::DOCTOR => self.run_doctor(),
            menu_ids::FAILED_FILES => self.view_failed_files(),
            menu_ids::CHECK_UPDATES => {
                open_url(RELEASES_URL);
                Ok(())
            }
            menu_ids::EXIT => {
                self.running.store(false, Ordering::SeqCst);
                Ok(())
            }
            _ => Ok(()),
        };

        if let Err(e) = result {
            show_message_box(
                "MD QC Agent - Error",
                &format!("Failed to handle menu action '{}':\n\n{}", id, e),
                true,
            );
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
        shell_open(&log_dir.to_string_lossy())
    }

    fn open_template(&self) -> Result<()> {
        // Try to load config and find template path
        if let Ok(cfg) = config::Config::load() {
            if let Some(instrument) = cfg.instruments.first() {
                if !instrument.template.is_empty() {
                    let template_path = std::path::Path::new(&instrument.template);
                    if template_path.exists() {
                        return shell_open(&template_path.to_string_lossy());
                    }
                }
            }
        }

        // Fallback: open methods directory (where QC_Method.sky should be)
        let methods_dir = config::paths::data_dir().join("methods");
        std::fs::create_dir_all(&methods_dir)?;
        shell_open(&methods_dir.to_string_lossy())
    }

    fn open_data_folder(&self) -> Result<()> {
        // Try to load config and find watch path
        if let Ok(cfg) = config::Config::load() {
            if let Some(instrument) = cfg.instruments.first() {
                let watch_path = std::path::Path::new(&instrument.watch_path);
                if watch_path.exists() {
                    return shell_open(&watch_path.to_string_lossy());
                }
            }
        }

        // Fallback: open user's documents
        let docs_path = dirs::document_dir().unwrap_or_else(|| std::path::PathBuf::from("C:\\"));
        shell_open(&docs_path.to_string_lossy())
    }

    fn run_doctor(&self) -> Result<()> {
        // Run mdqc doctor in a visible console that stays open
        let exe_path = std::env::current_exe()?;
        std::process::Command::new("cmd")
            .args(["/k", &format!("\"{}\" doctor", exe_path.display())])
            .spawn()?;
        Ok(())
    }

    fn view_failed_files(&self) -> Result<()> {
        // Run mdqc failed list in a visible console that stays open
        let exe_path = std::env::current_exe()?;
        std::process::Command::new("cmd")
            .args(["/k", &format!("\"{}\" failed list", exe_path.display())])
            .spawn()?;
        Ok(())
    }
}

impl ApplicationHandler for TrayApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
                    let msg = format!("Failed to create tray menu:\n\n{}", e);
                    eprintln!("{}", msg);
                    show_message_box("MD QC Agent - Fatal Error", &msg, true);
                    self.running.store(false, Ordering::SeqCst);
                    event_loop.exit();
                    return;
                }
            };

            let icon = match self.create_icon() {
                Ok(i) => i,
                Err(e) => {
                    let msg = format!("Failed to load tray icon:\n\n{}", e);
                    eprintln!("{}", msg);
                    show_message_box("MD QC Agent - Fatal Error", &msg, true);
                    self.running.store(false, Ordering::SeqCst);
                    event_loop.exit();
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
                    let msg = format!("Failed to create system tray icon:\n\n{}", e);
                    eprintln!("{}", msg);
                    show_message_box("MD QC Agent - Fatal Error", &msg, true);
                    self.running.store(false, Ordering::SeqCst);
                    event_loop.exit();
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
        // Process all pending menu events
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            self.handle_menu_event(event);
        }

        // Check if we should exit
        if !self.running.load(Ordering::SeqCst) {
            event_loop.exit();
            return;
        }

        // Use a short timeout to poll for menu events periodically
        // Menu events come through a separate channel and don't wake the event loop
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        ));
    }
}

/// Show a Windows message box (ensures it appears in foreground)
fn show_message_box(title: &str, message: &str, is_error: bool) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Convert strings to wide strings for Windows API
    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();

    // MB_OK = 0x00000000
    // MB_ICONERROR = 0x00000010
    // MB_ICONINFORMATION = 0x00000040
    // MB_SETFOREGROUND = 0x00010000 (bring to foreground)
    // MB_TOPMOST = 0x00040000 (stay on top)
    let base_flags: u32 = 0x00010000 | 0x00040000; // SETFOREGROUND | TOPMOST
    let flags: u32 = base_flags | if is_error { 0x10 } else { 0x40 };

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

/// Open a file, folder, or URL using the Windows Shell API.
/// This is the correct, robust way to open things on Windows.
fn shell_open(path: &str) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;

    let path_wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();

    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            0,                  // hwnd
            operation.as_ptr(), // lpOperation ("open")
            path_wide.as_ptr(), // lpFile
            null(),             // lpParameters
            null(),             // lpDirectory
            1,                  // nShowCmd (SW_SHOWNORMAL = 1)
        )
    };

    // ShellExecuteW returns > 32 on success
    if result as usize > 32 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "ShellExecute failed with code {}",
            result as usize
        ))
    }
}

/// Open URL in default browser
fn open_url(url: &str) {
    let _ = shell_open(url);
}

/// Ensure a Start Menu shortcut exists with the correct AppUserModelID.
/// This is required for Windows toast notifications to show the correct app name.
fn ensure_start_menu_shortcut() {
    use crate::notifications::APP_USER_MODEL_ID;

    // Get the Start Menu Programs folder
    let start_menu = match std::env::var("APPDATA") {
        Ok(appdata) => std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs"),
        Err(_) => return,
    };

    let shortcut_path = start_menu.join("MD QC Agent.lnk");

    // Skip if shortcut already exists
    if shortcut_path.exists() {
        return;
    }

    // Get the current executable path
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    println!("Creating Start Menu shortcut for notifications...");

    // Use PowerShell with .NET to create shortcut with AppUserModelID
    // This approach uses Windows.Storage which can properly set the property
    let ps_script = format!(
        r#"
$shortcutPath = '{shortcut}'
$targetPath = '{exe}'
$appId = '{app_id}'

# Create shortcut using WScript.Shell
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $targetPath
$shortcut.Arguments = 'tray'
$shortcut.Description = 'Mass Dynamics QC Agent'
$shortcut.Save()

# Set AppUserModelID using PropertyStore
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public class ShortcutHelper {{
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern int SHGetPropertyStoreFromParsingName(
        string pszPath,
        IntPtr pbc,
        int flags,
        ref Guid riid,
        out IPropertyStore ppv);

    [ComImport]
    [Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IPropertyStore {{
        int GetCount(out uint cProps);
        int GetAt(uint iProp, out PROPERTYKEY pkey);
        int GetValue(ref PROPERTYKEY key, out PROPVARIANT pv);
        int SetValue(ref PROPERTYKEY key, ref PROPVARIANT pv);
        int Commit();
    }}

    [StructLayout(LayoutKind.Sequential)]
    struct PROPERTYKEY {{
        public Guid fmtid;
        public uint pid;
    }}

    [StructLayout(LayoutKind.Sequential)]
    struct PROPVARIANT {{
        public ushort vt;
        public ushort wReserved1;
        public ushort wReserved2;
        public ushort wReserved3;
        public IntPtr pwszVal;
        public IntPtr dummy;
    }}

    public static void SetAppUserModelId(string shortcutPath, string appId) {{
        Guid IID_IPropertyStore = new Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99");
        IPropertyStore store;
        int hr = SHGetPropertyStoreFromParsingName(shortcutPath, IntPtr.Zero, 2, ref IID_IPropertyStore, out store);
        if (hr != 0) return;

        PROPERTYKEY key = new PROPERTYKEY();
        key.fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3");
        key.pid = 5;

        PROPVARIANT pv = new PROPVARIANT();
        pv.vt = 31; // VT_LPWSTR
        pv.pwszVal = Marshal.StringToCoTaskMemUni(appId);

        store.SetValue(ref key, ref pv);
        store.Commit();
        Marshal.FreeCoTaskMem(pv.pwszVal);
    }}
}}
'@

[ShortcutHelper]::SetAppUserModelId($shortcutPath, $appId)
Write-Host 'Shortcut created with AppUserModelID'
"#,
        shortcut = shortcut_path
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('\'', "''"),
        exe = exe_path
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('\'', "''"),
        app_id = APP_USER_MODEL_ID
    );

    // Execute PowerShell script
    let result = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_script,
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                println!("Start Menu shortcut created successfully");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stderr.is_empty() {
                    eprintln!("Shortcut creation warning: {}", stderr);
                }
                if stdout.contains("Shortcut created") {
                    println!("Start Menu shortcut created");
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to run PowerShell: {}", e);
        }
    }
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

    // Ensure Start Menu shortcut exists (for notification app name)
    ensure_start_menu_shortcut();

    println!("Starting MD QC Agent system tray...");
    println!("Right-click the tray icon for options.");

    let event_loop = EventLoop::new()?;
    let mut app = TrayApp::new();

    event_loop.run_app(&mut app)?;

    Ok(())
}
