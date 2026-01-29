//! MD Local QC Agent
//!
//! A passive, vendor-agnostic telemetry service that extracts EvoSep-aligned
//! system-suitability signals from completed MS runs using Skyline headlessly.

use anyhow::Result;
use clap::Parser;
use tracing::info;

mod baseline;
mod classifier;
mod cli;
mod config;
mod crash;
mod error;
mod extractor;
mod failed_files;
#[cfg(windows)]
mod gui;
mod metrics;
mod notifications;
mod service;
mod spool;
mod tray;
mod types;
mod uploader;
mod watcher;

use cli::{Cli, Command};

fn main() {
    // Wrap everything to catch early errors
    if let Err(e) = real_main() {
        // Try to show error - this catches errors before tokio/logging are initialized
        show_startup_error(&format!("{:?}", e));
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn show_startup_error(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let title = "MD QC Agent - Startup Error";
    let full_message = format!(
        "Failed to start MD QC Agent:\n\n{}\n\nPlease run 'mdqc doctor' for diagnostics.",
        message
    );

    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(&full_message)
        .encode_wide()
        .chain(Some(0))
        .collect();

    // MB_ICONERROR = 0x10, MB_SETFOREGROUND = 0x10000, MB_TOPMOST = 0x40000
    let flags: u32 = 0x10 | 0x10000 | 0x40000;

    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            0,
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            flags,
        );
    }
}

#[cfg(not(windows))]
fn show_startup_error(message: &str) {
    eprintln!("MD QC Agent startup error: {}", message);
}

#[tokio::main]
async fn real_main() -> Result<()> {
    // Install crash handler first thing
    crash::install_panic_hook();

    let cli = Cli::parse();

    // Hide console window for tray and GUI commands (they don't need it)
    #[cfg(windows)]
    if matches!(cli.command, Command::Tray | Command::Gui) {
        unsafe {
            windows_sys::Win32::System::Console::FreeConsole();
        }
    }

    // Initialize logging based on command
    // Run and Tray use file logging; GUI has no logging; other commands use console
    let _guard = match &cli.command {
        Command::Run { .. } | Command::Tray => init_file_logging(&cli)?,
        Command::Gui => None, // GUI doesn't need logging
        _ => init_console_logging(&cli)?,
    };

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "MD Local QC Agent starting"
    );

    match cli.command {
        Command::Run { foreground } => {
            if foreground {
                cli::run::run_foreground().await
            } else {
                #[cfg(windows)]
                {
                    service::run_as_service()
                }
                #[cfg(not(windows))]
                {
                    // On non-Windows, just run in foreground
                    cli::run::run_foreground().await
                }
            }
        }
        Command::Doctor => cli::doctor::run().await,
        Command::Classify { path } => cli::classify::run(&path).await,
        Command::Status => cli::status::run().await,
        Command::Baseline { action } => cli::baseline::run(action).await,
        Command::Config { action } => cli::config::run(action).await,
        Command::Failed { action } => cli::failed::run(action).await,
        Command::Tray => tray::run_tray().await,
        Command::Gui => {
            #[cfg(windows)]
            {
                gui::run()
            }
            #[cfg(not(windows))]
            {
                anyhow::bail!("GUI is only supported on Windows")
            }
        }
        Command::Version => {
            println!("mdqc {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

fn init_console_logging(cli: &Cli) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(cli.log_level.as_str()));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();

    Ok(None)
}

fn init_file_logging(cli: &Cli) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let log_dir = config::paths::log_dir()?;
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("mdqc")
        .filename_suffix("log")
        .max_log_files(10)
        .build(&log_dir)?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(cli.log_level.as_str()));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_target(true)
                .with_ansi(false)
                .json()
                .with_writer(non_blocking),
        )
        .init();

    Ok(Some(guard))
}
