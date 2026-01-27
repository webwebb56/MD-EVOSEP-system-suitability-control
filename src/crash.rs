//! Crash reporting and panic handling.

use std::backtrace::Backtrace;
use std::fs;
use std::panic::PanicHookInfo;

use crate::config::paths;

/// GitHub repository for issue reporting
const GITHUB_REPO: &str = "webwebb56/MD-EVOSEP-system-suitability-control";

/// Install the panic hook for crash reporting.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        handle_panic(panic_info);
    }));
}

fn handle_panic(panic_info: &PanicHookInfo) {
    let backtrace = Backtrace::force_capture();

    // Build crash report
    let report = build_crash_report(panic_info, &backtrace);

    // Try to write crash report to file
    let crash_file = write_crash_report(&report);

    // Show dialog and offer to report
    show_crash_dialog(&report, crash_file.as_deref());
}

fn build_crash_report(panic_info: &PanicHookInfo, backtrace: &Backtrace) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

    // Get panic message
    let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic".to_string()
    };

    // Get location
    let location = panic_info
        .location()
        .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
        .unwrap_or_else(|| "unknown location".to_string());

    // Get OS info
    let os_info = format!("Windows {}", std::env::var("OS").unwrap_or_default());

    format!(
        r#"MD QC Agent Crash Report
========================

Version: {version}
Timestamp: {timestamp}
OS: {os_info}

Panic Message:
{message}

Location:
{location}

Backtrace:
{backtrace}
"#
    )
}

fn write_crash_report(report: &str) -> Option<String> {
    let log_dir = paths::log_dir().ok()?;
    fs::create_dir_all(&log_dir).ok()?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("crash_{}.txt", timestamp);
    let path = log_dir.join(&filename);

    fs::write(&path, report).ok()?;
    Some(path.display().to_string())
}

#[cfg(windows)]
fn show_crash_dialog(report: &str, crash_file: Option<&str>) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Build message
    let file_info = crash_file
        .map(|f| format!("\n\nCrash report saved to:\n{}", f))
        .unwrap_or_default();

    let message = format!(
        "MD QC Agent has crashed unexpectedly.{}\n\nWould you like to report this issue on GitHub?",
        file_info
    );

    let title = "MD QC Agent - Crash";

    // Convert to wide strings
    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(&message).encode_wide().chain(Some(0)).collect();

    // MB_YESNO = 4, MB_ICONERROR = 0x10
    let flags: u32 = 4 | 0x10;

    let result = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            0,
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            flags,
        )
    };

    // IDYES = 6
    if result == 6 {
        open_github_issue(report);
    }
}

#[cfg(not(windows))]
fn show_crash_dialog(report: &str, crash_file: Option<&str>) {
    eprintln!("MD QC Agent crashed!");
    if let Some(f) = crash_file {
        eprintln!("Crash report saved to: {}", f);
    }
    eprintln!(
        "\nReport this issue at: https://github.com/{}/issues",
        GITHUB_REPO
    );
}

fn open_github_issue(report: &str) {
    let version = env!("CARGO_PKG_VERSION");

    // Extract just the panic message and location for the title
    let title = format!("Crash in v{}", version);

    // Build issue body - truncate if too long for URL
    let body = build_issue_body(report);

    // URL encode
    let title_encoded = urlencoding::encode(&title);
    let body_encoded = urlencoding::encode(&body);

    let url = format!(
        "https://github.com/{}/issues/new?title={}&body={}",
        GITHUB_REPO, title_encoded, body_encoded
    );

    // Open in browser
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", &url])
        .spawn();
}

fn build_issue_body(report: &str) -> String {
    // Truncate backtrace to keep URL reasonable (max ~2000 chars for body)
    let truncated_report = if report.len() > 1500 {
        let mut truncated = report.chars().take(1500).collect::<String>();
        truncated.push_str("\n\n... (truncated, see attached crash report for full details)");
        truncated
    } else {
        report.to_string()
    };

    format!(
        r#"## Crash Report

The application crashed unexpectedly.

<details>
<summary>Crash Details</summary>

```
{}
```

</details>

## Steps to Reproduce

1.

## Additional Context

<!-- Please attach the full crash report file if available -->
"#,
        truncated_report
    )
}

/// Simple URL encoding module
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for c in input.chars() {
            match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                    result.push(c);
                }
                ' ' => result.push_str("%20"),
                '\n' => result.push_str("%0A"),
                '\r' => result.push_str("%0D"),
                _ => {
                    for byte in c.to_string().bytes() {
                        result.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
        }
        result
    }
}
