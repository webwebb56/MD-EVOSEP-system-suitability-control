//! Windows service integration.
//!
//! Provides Windows service scaffolding for running the agent as a service.

#[cfg(windows)]
mod windows_service;

#[cfg(windows)]
pub use windows_service::run_as_service;

#[cfg(not(windows))]
pub fn run_as_service() -> anyhow::Result<()> {
    anyhow::bail!("Windows service is only available on Windows")
}
