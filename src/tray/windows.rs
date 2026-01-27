//! Windows system tray implementation.

use anyhow::Result;
use image::GenericImageView;
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

/// Menu item IDs
mod menu_ids {
    pub const STATUS: &str = "status";
    pub const INSTRUMENT_COUNT: &str = "instrument_count";
    pub const OPEN_CONFIG: &str = "open_config";
    pub const OPEN_LOGS: &str = "open_logs";
    pub const OPEN_TEMPLATE: &str = "open_template";
    pub const OPEN_DATA_FOLDER: &str = "open_data_folder";
    pub const DOCTOR: &str = "doctor";
    pub const EXIT: &str = "exit";
}

/// Application state for the tray icon
struct TrayApp {
    tray_icon: Option<TrayIcon>,
    running: Arc<AtomicBool>,
}

impl TrayApp {
    fn new() -> Self {
        Self {
            tray_icon: None,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    fn create_menu(&self) -> Result<Menu> {
        let menu = Menu::new();

        // Status item (disabled, just shows info)
        let status_item = MenuItem::with_id(menu_ids::STATUS, "MD QC Agent v0.1.0", false, None);
        menu.append(&status_item)?;

        // Try to load config and show instrument count
        let instrument_text = self.get_instrument_status();
        let instrument_item = MenuItem::with_id(menu_ids::INSTRUMENT_COUNT, &instrument_text, false, None);
        menu.append(&instrument_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Settings section
        let config_item = MenuItem::with_id(menu_ids::OPEN_CONFIG, "Edit Configuration...", true, None);
        menu.append(&config_item)?;

        let template_item = MenuItem::with_id(menu_ids::OPEN_TEMPLATE, "Open Skyline Template...", true, None);
        menu.append(&template_item)?;

        let data_folder_item = MenuItem::with_id(menu_ids::OPEN_DATA_FOLDER, "Open Watch Folder...", true, None);
        menu.append(&data_folder_item)?;

        let logs_item = MenuItem::with_id(menu_ids::OPEN_LOGS, "View Logs...", true, None);
        menu.append(&logs_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Diagnostics
        let doctor_item = MenuItem::with_id(menu_ids::DOCTOR, "Run Diagnostics...", true, None);
        menu.append(&doctor_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Exit
        let exit_item = MenuItem::with_id(menu_ids::EXIT, "Exit", true, None);
        menu.append(&exit_item)?;

        Ok(menu)
    }

    fn get_instrument_status(&self) -> String {
        let config_path = match config::paths::config_file() {
            Ok(p) => p,
            Err(_) => return "No configuration found".to_string(),
        };

        if !config_path.exists() {
            return "No configuration found".to_string();
        }

        match config::Config::load(Some(&config_path)) {
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
            menu_ids::EXIT => {
                self.running.store(false, Ordering::SeqCst);
            }
            _ => {}
        }
    }

    fn open_config(&self) -> Result<()> {
        let config_path = config::paths::config_file()?;
        if config_path.exists() {
            // Open with default text editor
            std::process::Command::new("notepad")
                .arg(&config_path)
                .spawn()?;
        } else {
            // Open the config directory
            let config_dir = config::paths::config_dir()?;
            std::process::Command::new("explorer")
                .arg(&config_dir)
                .spawn()?;
        }
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
        let config_path = config::paths::config_file()?;
        if config_path.exists() {
            if let Ok(cfg) = config::Config::load(Some(&config_path)) {
                // Get the first instrument's template
                if let Some(instrument) = cfg.instruments.first() {
                    let template_path = std::path::Path::new(&instrument.template);
                    if template_path.exists() {
                        // Open with Skyline if available
                        if let Some(skyline_path) = find_skyline() {
                            let skyline_exe = skyline_path.with_file_name("Skyline.exe");
                            if skyline_exe.exists() {
                                std::process::Command::new(&skyline_exe)
                                    .arg(&template_path)
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
        let config_path = config::paths::config_file()?;
        if config_path.exists() {
            if let Ok(cfg) = config::Config::load(Some(&config_path)) {
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
        std::process::Command::new("cmd")
            .args(["/k", exe_path.to_str().unwrap_or("mdqc"), "doctor"])
            .spawn()?;
        Ok(())
    }
}

impl ApplicationHandler for TrayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        // Create tray icon on first resume
        if self.tray_icon.is_none() {
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

            let tray_icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("MD QC Agent")
                .with_icon(icon)
                .build();

            match tray_icon {
                Ok(ti) => {
                    self.tray_icon = Some(ti);
                    println!("System tray icon created successfully");
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
        let apps_dir = std::path::Path::new(&local_app_data).join("Apps").join("2.0");
        if apps_dir.exists() {
            // Search for SkylineCmd.exe in the ClickOnce deployment
            if let Ok(entries) = glob::glob(&format!("{}/**/SkylineCmd.exe", apps_dir.display())) {
                for entry in entries.flatten() {
                    return Some(entry);
                }
            }
        }
    }

    None
}

/// Run the system tray application
pub async fn run_tray() -> Result<()> {
    println!("Starting MD QC Agent system tray...");
    println!("Right-click the tray icon for options.");

    let event_loop = EventLoop::new()?;
    let mut app = TrayApp::new();

    event_loop.run_app(&mut app)?;

    Ok(())
}
