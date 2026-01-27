//! Windows service implementation.

#[cfg(windows)]
use std::ffi::OsString;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::cli::run::run_agent;
use crate::config::Config;

const SERVICE_NAME: &str = "MassDynamicsQC";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

/// Run the agent as a Windows service.
#[cfg(windows)]
pub fn run_as_service() -> anyhow::Result<()> {
    // Register the service entry point
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

// Generate the Windows service boilerplate
#[cfg(windows)]
define_windows_service!(ffi_service_main, service_main);

/// Service main entry point.
#[cfg(windows)]
fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service(arguments) {
        error!(error = %e, "Service failed");
    }
}

/// Run the service.
#[cfg(windows)]
fn run_service(_arguments: Vec<OsString>) -> anyhow::Result<()> {
    // Create a channel for shutdown signaling
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Register the service control handler
    let shutdown_tx_clone = shutdown_tx.clone();
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                // Signal shutdown
                let _ = shutdown_tx_clone.blocking_send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    // Report that we're starting
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    })?;

    // Initialize the async runtime
    let runtime = tokio::runtime::Runtime::new()?;

    // Load configuration
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to load configuration");

            status_handle.set_service_status(ServiceStatus {
                service_type: SERVICE_TYPE,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::Win32(1),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })?;

            return Err(e);
        }
    };

    // Report that we're running
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    info!("Service started");

    // Run the agent
    let result = runtime.block_on(async { run_agent(config, &mut shutdown_rx).await });

    // Report that we're stopping
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::StopPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    })?;

    info!("Service stopping");

    // Report that we've stopped
    let exit_code = match result {
        Ok(()) => ServiceExitCode::Win32(0),
        Err(e) => {
            error!(error = %e, "Service error");
            ServiceExitCode::Win32(1)
        }
    };

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code,
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    info!("Service stopped");

    Ok(())
}
