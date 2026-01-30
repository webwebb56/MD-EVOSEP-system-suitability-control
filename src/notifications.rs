//! Toast notifications for Windows.
//!
//! Provides lightweight, non-intrusive notifications for QC processing events.

use tracing::{debug, warn};

/// App User Model ID for notifications.
/// This must match the ID set on the Start Menu shortcut created by ensure_start_menu_shortcut().
#[cfg(windows)]
pub const APP_USER_MODEL_ID: &str = "MassDynamics.QCAgent";

/// Helper to show a toast notification with consistent styling.
#[cfg(windows)]
fn show_toast(title: &str, body: &str, silent: bool) {
    use winrt_notification::{Duration, Sound, Toast};

    let mut toast = Toast::new(APP_USER_MODEL_ID);
    toast = toast.title(title).text1(body).duration(Duration::Short);

    if !silent {
        toast = toast.sound(Some(Sound::Default));
    }

    if let Err(e) = toast.show() {
        warn!(error = %e, "Failed to show toast notification");
    }
}

/// Notify when a QC file is detected and queued for processing.
pub fn notify_file_detected(file_name: &str, instrument: &str, stability_window_secs: u64) {
    debug!(file = file_name, instrument, "File detected notification");

    #[cfg(windows)]
    {
        let title = "QC File Detected";
        let body = format!(
            "{}\nWaiting {}s for file to stabilize...",
            file_name, stability_window_secs
        );
        show_toast(title, &body, true); // Silent - don't beep for detection
    }

    #[cfg(not(windows))]
    {
        let _ = (file_name, instrument, stability_window_secs);
    }
}

/// Notify when extraction/processing starts.
pub fn notify_processing_started(file_name: &str) {
    debug!(file = file_name, "Processing started notification");

    #[cfg(windows)]
    {
        let title = "Processing QC File";
        let body = format!("{}\nExtracting with Skyline...", file_name);
        show_toast(title, &body, true); // Silent
    }

    #[cfg(not(windows))]
    {
        let _ = file_name;
    }
}

/// Notify when extraction completes successfully.
pub fn notify_extraction_success(file_name: &str, targets_found: u32, targets_expected: u32) {
    debug!(
        file = file_name,
        targets_found, targets_expected, "Extraction success notification"
    );

    #[cfg(windows)]
    {
        let title = "QC Extraction Complete";
        let body = format!(
            "{}\n{}/{} targets detected",
            file_name, targets_found, targets_expected
        );
        show_toast(title, &body, false); // Play sound for completion
    }

    #[cfg(not(windows))]
    {
        let _ = (file_name, targets_found, targets_expected);
    }
}

/// Notify when extraction fails.
pub fn notify_extraction_failure(file_name: &str, error: &str) {
    debug!(file = file_name, error, "Extraction failure notification");

    #[cfg(windows)]
    {
        let title = "QC Extraction Failed";
        // Truncate error message if too long
        let error_short = if error.len() > 80 {
            format!("{}...", &error[..80])
        } else {
            error.to_string()
        };
        let body = format!("{}\n{}", file_name, error_short);
        show_toast(title, &body, false); // Play sound for errors
    }

    #[cfg(not(windows))]
    {
        let _ = (file_name, error);
    }
}

/// Notify when results are queued for upload.
pub fn notify_upload_queued(file_name: &str) {
    debug!(file = file_name, "Upload queued notification");

    #[cfg(windows)]
    {
        let title = "QC Results Queued";
        let body = format!("{}\nReady for upload", file_name);
        show_toast(title, &body, true); // Silent
    }

    #[cfg(not(windows))]
    {
        let _ = file_name;
    }
}

/// Notify when results are successfully uploaded.
#[allow(dead_code)] // Will be used when upload destination is configured
pub fn notify_upload_success(file_name: &str) {
    debug!(file = file_name, "Upload success notification");

    #[cfg(windows)]
    {
        let title = "QC Results Uploaded";
        let body = format!("{}\nSuccessfully sent to Mass Dynamics", file_name);
        show_toast(title, &body, true); // Silent - completion was the important one
    }

    #[cfg(not(windows))]
    {
        let _ = file_name;
    }
}

/// Notify when upload fails.
#[allow(dead_code)] // Will be used when upload destination is configured
pub fn notify_upload_failure(file_name: &str, error: &str) {
    debug!(file = file_name, error, "Upload failure notification");

    #[cfg(windows)]
    {
        let title = "QC Upload Failed";
        let error_short = if error.len() > 80 {
            format!("{}...", &error[..80])
        } else {
            error.to_string()
        };
        let body = format!("{}\n{}", file_name, error_short);
        show_toast(title, &body, false); // Play sound for errors
    }

    #[cfg(not(windows))]
    {
        let _ = (file_name, error);
    }
}
