//! GUI configuration editor using egui.
//!
//! Windows-only module (requires eframe/egui which are Windows-only deps).

use anyhow::Result;
use eframe::egui;

use crate::config::{self, Config, InstrumentConfig};
use crate::types::Vendor;

/// Editable state for the configuration editor.
struct ConfigEditor {
    /// Path to the config file
    config_path: std::path::PathBuf,

    /// Agent settings
    enable_notifications: bool,

    /// Cloud settings
    endpoint: String,
    api_token: String,

    /// Skyline settings
    skyline_path: String,
    skyline_timeout_secs: u64,

    /// Watcher settings
    scan_interval_secs: u64,
    stability_window_secs: u64,

    /// Instruments
    instruments: Vec<InstrumentEditor>,

    /// Status message
    status_message: Option<(String, bool)>, // (message, is_error)
}

/// Editable state for a single instrument.
#[derive(Clone)]
struct InstrumentEditor {
    id: String,
    vendor: Vendor,
    watch_path: String,
    file_pattern: String,
    template: String,
}

impl Default for InstrumentEditor {
    fn default() -> Self {
        Self {
            id: String::new(),
            vendor: Vendor::Thermo,
            watch_path: String::new(),
            file_pattern: "*.raw".to_string(),
            template: String::new(),
        }
    }
}

impl ConfigEditor {
    fn new() -> Self {
        let config_path = config::paths::config_file();

        // Default values
        let mut enable_notifications = true;
        let mut endpoint = "https://qc-ingest.massdynamics.com/v1/".to_string();
        let mut api_token = String::new();
        let mut skyline_path = String::new();
        let mut skyline_timeout_secs: u64 = 300;
        let mut scan_interval_secs: u64 = 30;
        let mut stability_window_secs: u64 = 60;
        let mut instruments = Vec::new();

        // Try to load existing config
        if config_path.exists() {
            if let Ok(cfg) = Config::load() {
                enable_notifications = cfg.agent.enable_toast_notifications;
                endpoint = cfg.cloud.endpoint.clone();
                api_token = cfg.cloud.api_token.clone().unwrap_or_default();
                skyline_path = cfg.skyline.path.clone().unwrap_or_default();
                skyline_timeout_secs = cfg.skyline.timeout_seconds;
                scan_interval_secs = cfg.watcher.scan_interval_seconds;
                stability_window_secs = cfg.watcher.stability_window_seconds;
                instruments = cfg
                    .instruments
                    .iter()
                    .map(|i| InstrumentEditor {
                        id: i.id.clone(),
                        vendor: i.vendor,
                        watch_path: i.watch_path.clone(),
                        file_pattern: i.file_pattern.clone(),
                        template: i.template.clone(),
                    })
                    .collect();
            }
        }

        Self {
            config_path,
            enable_notifications,
            endpoint,
            api_token,
            skyline_path,
            skyline_timeout_secs,
            scan_interval_secs,
            stability_window_secs,
            instruments,
            status_message: None,
        }
    }

    fn save_config(&mut self) -> Result<()> {
        // Load existing config to preserve other settings, or create default
        let mut config = if self.config_path.exists() {
            Config::load().unwrap_or_default()
        } else {
            Config::default()
        };

        // Update with editor values
        config.path = self.config_path.clone();

        // Agent settings
        config.agent.enable_toast_notifications = self.enable_notifications;

        // Cloud settings
        config.cloud.endpoint = self.endpoint.clone();
        config.cloud.api_token = if self.api_token.is_empty() {
            None
        } else {
            Some(self.api_token.clone())
        };

        // Skyline settings
        config.skyline.path = if self.skyline_path.is_empty() {
            None
        } else {
            Some(self.skyline_path.clone())
        };
        config.skyline.timeout_seconds = self.skyline_timeout_secs;

        // Watcher settings
        config.watcher.scan_interval_seconds = self.scan_interval_secs;
        config.watcher.stability_window_seconds = self.stability_window_secs;

        // Instruments
        config.instruments = self
            .instruments
            .iter()
            .map(|i| InstrumentConfig {
                id: i.id.clone(),
                vendor: i.vendor,
                watch_path: i.watch_path.clone(),
                file_pattern: i.file_pattern.clone(),
                template: i.template.clone(),
                watcher_overrides: None,
            })
            .collect();

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        config.save()?;
        Ok(())
    }
}

impl eframe::App for ConfigEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("MD QC Agent Configuration");
                ui.add_space(10.0);

                // General Settings Section
                ui.group(|ui| {
                    ui.heading("General");
                    ui.add_space(5.0);
                    ui.checkbox(&mut self.enable_notifications, "Enable notifications")
                        .on_hover_text("Show Windows notifications for file detection, processing, and completion");
                });

                ui.add_space(10.0);

                // Watcher Settings Section
                ui.group(|ui| {
                    ui.heading("File Watcher");
                    ui.add_space(5.0);

                    egui::Grid::new("watcher_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .show(ui, |ui| {
                            ui.label("Scan interval (seconds):")
                                .on_hover_text("How often to check for new files");
                            ui.add(egui::DragValue::new(&mut self.scan_interval_secs).range(5..=300));
                            ui.end_row();

                            ui.label("Stability window (seconds):")
                                .on_hover_text("Wait for file to stop changing before processing");
                            ui.add(egui::DragValue::new(&mut self.stability_window_secs).range(10..=600));
                            ui.end_row();
                        });
                });

                ui.add_space(10.0);

                // Cloud Settings Section
                ui.group(|ui| {
                    ui.heading("Cloud Settings");
                    ui.add_space(5.0);

                    egui::Grid::new("cloud_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .show(ui, |ui| {
                            ui.label("Endpoint:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.endpoint).desired_width(400.0),
                            );
                            ui.end_row();

                            ui.label("API Token:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.api_token)
                                    .password(true)
                                    .desired_width(400.0),
                            );
                            ui.end_row();
                        });
                });

                ui.add_space(10.0);

                // Skyline Section
                ui.group(|ui| {
                    ui.heading("Skyline");
                    ui.add_space(5.0);

                    egui::Grid::new("skyline_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .show(ui, |ui| {
                            ui.label("Path:");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.skyline_path)
                                        .desired_width(300.0)
                                        .hint_text("Leave empty for auto-discovery"),
                                );
                                if ui.button("Browse...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("Skyline", &["exe"])
                                        .set_title("Select SkylineCmd.exe")
                                        .pick_file()
                                    {
                                        self.skyline_path = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Timeout (seconds):")
                                .on_hover_text("Maximum time to wait for Skyline extraction");
                            ui.add(egui::DragValue::new(&mut self.skyline_timeout_secs).range(60..=1800));
                            ui.end_row();
                        });
                });

                ui.add_space(10.0);

                // Instruments Section
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Instruments");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("+ Add Instrument").clicked() {
                                self.instruments.push(InstrumentEditor::default());
                            }
                        });
                    });
                    ui.add_space(5.0);

                    let mut to_remove: Option<usize> = None;

                    for (idx, instrument) in self.instruments.iter_mut().enumerate() {
                        ui.push_id(idx, |ui| {
                            ui.group(|ui| {
                                egui::Grid::new(format!("instrument_grid_{}", idx))
                                    .num_columns(2)
                                    .spacing([10.0, 5.0])
                                    .show(ui, |ui| {
                                        ui.label("ID:");
                                        ui.add(
                                            egui::TextEdit::singleline(&mut instrument.id)
                                                .desired_width(200.0)
                                                .hint_text("e.g., EXPLORIS_01"),
                                        );
                                        ui.end_row();

                                        ui.label("Vendor:");
                                        egui::ComboBox::from_id_salt(format!("vendor_{}", idx))
                                            .selected_text(format!("{}", instrument.vendor))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut instrument.vendor,
                                                    Vendor::Thermo,
                                                    "thermo",
                                                );
                                                ui.selectable_value(
                                                    &mut instrument.vendor,
                                                    Vendor::Bruker,
                                                    "bruker",
                                                );
                                                ui.selectable_value(
                                                    &mut instrument.vendor,
                                                    Vendor::Sciex,
                                                    "sciex",
                                                );
                                                ui.selectable_value(
                                                    &mut instrument.vendor,
                                                    Vendor::Waters,
                                                    "waters",
                                                );
                                                ui.selectable_value(
                                                    &mut instrument.vendor,
                                                    Vendor::Agilent,
                                                    "agilent",
                                                );
                                            });
                                        ui.end_row();

                                        ui.label("Watch Path:");
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut instrument.watch_path,
                                                )
                                                .desired_width(300.0)
                                                .hint_text("e.g., D:\\Data"),
                                            );
                                            if ui.button("Browse...").clicked() {
                                                if let Some(path) = rfd::FileDialog::new()
                                                    .set_title("Select Watch Folder")
                                                    .pick_folder()
                                                {
                                                    instrument.watch_path =
                                                        path.display().to_string();
                                                }
                                            }
                                        });
                                        ui.end_row();

                                        ui.label("File Pattern:");
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut instrument.file_pattern,
                                            )
                                            .desired_width(150.0)
                                            .hint_text("e.g., *.raw"),
                                        );
                                        ui.end_row();

                                        ui.label("Template:");
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut instrument.template,
                                                )
                                                .desired_width(300.0)
                                                .hint_text("Path to .sky file"),
                                            );
                                            if ui.button("Browse...").clicked() {
                                                if let Some(path) = rfd::FileDialog::new()
                                                    .add_filter("Skyline Document", &["sky"])
                                                    .set_title("Select Skyline Template")
                                                    .pick_file()
                                                {
                                                    instrument.template =
                                                        path.display().to_string();
                                                }
                                            }
                                        });
                                        ui.end_row();
                                    });

                                ui.horizontal(|ui| {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Remove").clicked() {
                                                to_remove = Some(idx);
                                            }
                                        },
                                    );
                                });
                            });
                            ui.add_space(5.0);
                        });
                    }

                    if let Some(idx) = to_remove {
                        self.instruments.remove(idx);
                    }

                    if self.instruments.is_empty() {
                        ui.label("No instruments configured. Click '+ Add Instrument' to add one.");
                    }
                });

                ui.add_space(15.0);

                // Status message
                if let Some((msg, is_error)) = &self.status_message {
                    let color = if *is_error {
                        egui::Color32::RED
                    } else {
                        egui::Color32::GREEN
                    };
                    ui.colored_label(color, msg);
                    ui.add_space(5.0);
                }

                // Buttons
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Save").clicked() {
                            match self.save_config() {
                                Ok(()) => {
                                    self.status_message = Some((
                                        "Configuration saved successfully!".to_string(),
                                        false,
                                    ));
                                }
                                Err(e) => {
                                    self.status_message =
                                        Some((format!("Failed to save: {}", e), true));
                                }
                            }
                        }

                        if ui.button("Cancel").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
        });
    }
}

/// Run the configuration editor GUI.
pub fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([550.0, 600.0])
            .with_min_inner_size([500.0, 400.0])
            .with_title("MD QC Agent Configuration"),
        ..Default::default()
    };

    eframe::run_native(
        "MD QC Agent Configuration",
        options,
        Box::new(|_cc| Ok(Box::new(ConfigEditor::new()))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run GUI: {}", e))?;

    Ok(())
}
