//! Toast notifications for Windows.
//!
//! Provides lightweight, non-intrusive notifications for extraction events.

use tracing::{debug, warn};

/// Show a toast notification for successful extraction.
pub fn notify_extraction_success(file_name: &str, targets_found: u32, targets_expected: u32) {
    #[cfg(windows)]
    {
        use winrt_notification::{Duration, Sound, Toast};

        let title = "QC Extraction Complete";
        let body = format!(
            "{}\n{}/{} targets detected",
            file_name, targets_found, targets_expected
        );

        debug!(file = file_name, "Showing success notification");

        let result = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(title)
            .text1(&body)
            .sound(Some(Sound::Default))
            .duration(Duration::Short)
            .show();

        if let Err(e) = result {
            warn!(error = %e, "Failed to show toast notification");
        }
    }

    #[cfg(not(windows))]
    {
        debug!(
            file = file_name,
            targets_found, targets_expected, "Extraction success (notifications not supported)"
        );
    }
}

/// Show a toast notification for failed extraction.
pub fn notify_extraction_failure(file_name: &str, error: &str) {
    #[cfg(windows)]
    {
        use winrt_notification::{Duration, Sound, Toast};

        let title = "QC Extraction Failed";
        // Truncate error message if too long
        let error_short = if error.len() > 100 {
            format!("{}...", &error[..100])
        } else {
            error.to_string()
        };
        let body = format!("{}\n{}", file_name, error_short);

        debug!(file = file_name, "Showing failure notification");

        let result = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(title)
            .text1(&body)
            .sound(Some(Sound::Default))
            .duration(Duration::Short)
            .show();

        if let Err(e) = result {
            warn!(error = %e, "Failed to show toast notification");
        }
    }

    #[cfg(not(windows))]
    {
        debug!(
            file = file_name,
            error, "Extraction failure (notifications not supported)"
        );
    }
}

/// Show a toast notification for a processing event.
pub fn notify_processing_started(file_name: &str) {
    #[cfg(windows)]
    {
        use winrt_notification::{Duration, Toast};

        let title = "Processing QC File";
        let body = file_name;

        debug!(file = file_name, "Showing processing notification");

        // Use silent notification for processing start (less intrusive)
        let result = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(title)
            .text1(body)
            .duration(Duration::Short)
            .show();

        if let Err(e) = result {
            warn!(error = %e, "Failed to show toast notification");
        }
    }

    #[cfg(not(windows))]
    {
        debug!(
            file = file_name,
            "Processing started (notifications not supported)"
        );
    }
}
