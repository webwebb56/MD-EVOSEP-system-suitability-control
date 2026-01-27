//! System tray icon for Windows.
//!
//! Provides a system tray icon with context menu for quick access to
//! agent status, settings, and controls.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::run_tray;

#[cfg(not(windows))]
pub async fn run_tray() -> anyhow::Result<()> {
    anyhow::bail!("System tray is only supported on Windows")
}
