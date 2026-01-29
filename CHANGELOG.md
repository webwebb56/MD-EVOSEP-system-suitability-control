# MD QC Agent Changelog

## v0.4.8

### Features

- **Auto-create Start Menu shortcut**: Creates shortcut with AppUserModelID on first run
  - Notifications now show "Mass Dynamics QC Agent" instead of "PowerShell"
  - Shortcut placed in Start Menu Programs folder automatically

---

## v0.4.7

### Features

- **Enhanced notifications**: Added notifications for processing stages
  - "Processing QC File" - when extraction starts
  - "QC Extraction Complete" - when extraction succeeds (with sound)
  - "QC Extraction Failed" - when extraction fails (with sound)
  - "QC Results Queued" - when results are spooled for upload
- Changed notification app ID to "MassDynamics.QCAgent"

### Bug Fixes

- **Fixed: Tray menu actions failing** with "os error 50"
  - Now uses Windows ShellExecuteW API (the correct way)
  - Removed fragile process spawning for opening folders/files
- **Fixed: Files being re-processed continuously**
  - Added `processed_files` set to track completed files
  - Prevents scan loop from re-detecting already processed files
  - Files are only processed once per agent session

---

## v0.4.6

### Features Added

#### Windows Toast Notifications
- Show non-intrusive notifications when extraction completes or fails
- Displays file name and target count (success) or error message (failure)
- Enable/disable via `enable_toast_notifications` in config
- Uses native Windows 10/11 notification system

#### Flexible Skyline Report Parsing
- Header-based column detection supports various Skyline report formats
- Recognizes common column name variations (PeptideSequence, Mz, TotalArea, etc.)
- No longer requires columns in specific order

### Bug Fixes

#### Skyline Command Arguments
- **Issue**: SkylineCmd.exe requires `--name=value` format, not `--name value`
- **Fix**: Changed all argument passing to use `=` format
- **File**: `src/extractor/mod.rs`

#### Skyline Report Export
- **Issue**: Extraction failed with "report does not exist"
- **Solution**: Users must create `MD_QC_Report` in their `QC_Method.sky` template
- Added `--report-invariant` flag for language-independent column names
- Added helpful error message explaining how to create the report

### Documentation

- Added detailed step-by-step guide for creating `QC_Method.sky`
- Documented Full-Scan settings for DIA data extraction
- Documented MD_QC_Report creation with required columns
- Added troubleshooting for common Skyline errors
- Updated all config examples to use `methods/QC_Method.sky` path

---

## v0.4.5

### Features Added

#### Failed Files Tracking System
- **New module**: `src/failed_files.rs` - Persistent JSON store for tracking failed files
- **New CLI commands**: `mdqc failed list|retry|clear`
  - `mdqc failed list` - View all failed files with details
  - `mdqc failed retry <path>` - Retry a specific file (or "all")
  - `mdqc failed clear` - Clear the failed files list
- **Tray menu**: Added "View Failed Files..." menu item showing count
- **Integration**: Failures from Skyline extraction, classification, and spooling are now recorded

#### Test Harness (`test-harness/`)
- `simulate_acquisition.py` - Simulate MS file acquisition for various vendors
  - Supports: Thermo (.raw), Bruker (.d), Agilent (.d), Waters (.raw)
  - Options: `--control-type`, `--well`, `--prefix` for QC naming
  - Simulates gradual file writing and vendor-specific lock files
- `copy_as_qc.py` - Copy real MS files with QC-appropriate naming
  - Useful for testing full Skyline extraction pipeline
  - Options: `--simulate-write` for slow copy simulation
- `monitor_agent.py` - Monitor agent status, logs, and failed files
  - `--follow` for real-time log tailing
  - `--health-check` for CI/automation
- `run_tests.bat` - Quick test runner for Windows

### Bug Fixes

#### Tray Menu Events Not Working
- **Issue**: Menu clicks did nothing after adding failed files feature
- **Root cause**: Event loop using `ControlFlow::Wait` blocked waiting for window events, but menu events come through a separate channel that doesn't wake the event loop
- **Fix**: Changed to `ControlFlow::WaitUntil(100ms)` to poll for menu events periodically
- **File**: `src/tray/windows.rs`

#### Template Path Handling
- **Issue**: Absolute template paths in config not handled correctly
- **Root cause**: Code joined `template_dir` + absolute path, creating invalid path
- **Fix**: Check if path is absolute before joining
- **File**: `src/extractor/mod.rs`

#### Silent Skyline Failures
- **Issue**: Skyline errors showed empty stderr
- **Fix**: Also capture stdout (Skyline often writes errors there) and show exit code
- **File**: `src/extractor/mod.rs`

---

## v0.4.4

### Bug Fixes
- Hide console window for tray and GUI commands using `FreeConsole()`

---

## v0.4.3

### Bug Fixes
- Fix tray startup issues: handle "auto" skyline path correctly
- Show visible error dialogs for startup failures (MB_SETFOREGROUND | MB_TOPMOST)

---

## v0.4.2

### Bug Fixes
- Add early startup error handling with message boxes
- Kill running instance before upgrade (PrepareToInstall hook in installer)

---

## v0.3.0

### Features Added
- GUI configuration editor using egui/eframe
- `mdqc gui` command to launch editor
- Tray menu "Edit Configuration..." now opens GUI instead of Notepad
- Crash reporting with GitHub issue integration

---

## Architecture Notes

### File Processing Pipeline
1. **Watcher** detects new files via filesystem events or directory scan
2. **Stabilization** waits for file to stop changing (default 60s window)
3. **Lock check** verifies file not in use (vendor-specific)
4. **Classification** determines control type from filename (SSC0, QCA, QCB, BLANK, SAMPLE)
5. **Extraction** runs Skyline to extract metrics (if QC file)
6. **Spooling** saves results for upload
7. **Upload** sends to cloud API

### QC Classification Patterns
| Pattern | Control Type | Confidence |
|---------|-------------|------------|
| `SSC0`, `SSC_0`, `SSC-0` | System Suitability | High |
| `QCA`, `QC_A`, `QC-A` | QC Type A | High |
| `QCB`, `QC_B`, `QC-B` | QC Type B | High |
| `BLANK`, `BLK` | Blank | High |
| Well `A1`, `A2` | QC_A (inferred) | Medium |
| Well `A3`, `A4` | QC_B (inferred) | Medium |
| Everything else | SAMPLE | Low |

### Key File Locations
- Config: `C:\ProgramData\MassDynamics\QC\config.toml`
- Logs: `C:\ProgramData\MassDynamics\QC\logs\`
- Spool: `C:\ProgramData\MassDynamics\QC\spool\`
- Failed files: `C:\ProgramData\MassDynamics\QC\failed_files.json`
- Templates: `C:\ProgramData\MassDynamics\QC\templates\`

### Vendor File Formats
| Vendor | Extension | Type | Lock Files |
|--------|-----------|------|------------|
| Thermo | `.raw` | Single file | None |
| Bruker | `.d` | Directory | `analysis.tdf-journal`, `analysis.tdf-lock` |
| Agilent | `.d` | Directory | None |
| Waters | `.raw` | Directory | `_LOCK_` |
| Sciex | `.wiff` | Single file | None |

---

## Files Modified This Session

### New Files
- `src/failed_files.rs` - Failed files tracking module
- `src/cli/failed.rs` - Failed files CLI commands
- `test-harness/simulate_acquisition.py` - Acquisition simulator
- `test-harness/copy_as_qc.py` - Copy files with QC naming
- `test-harness/monitor_agent.py` - Agent monitor
- `test-harness/run_tests.bat` - Quick test runner
- `test-harness/README.md` - Test harness documentation
- `assets/MD_QC_Report.skyr` - Skyline report definition (needs fixing)

### Modified Files
- `Cargo.toml` - Added filetime dependency
- `src/main.rs` - Added failed_files module, FreeConsole for tray/GUI
- `src/cli/mod.rs` - Added FailedAction enum and Failed command
- `src/cli/run.rs` - Record failures to FailedFiles store
- `src/tray/windows.rs` - Fixed event loop, added failed files menu, visible errors
- `src/watcher/mod.rs` - Record timeout failures to FailedFiles
- `src/extractor/mod.rs` - Fixed template path, better error capture

---

## Next Steps

1. **Fix Skyline report issue** - Either correct .skyr format or use alternative approach
2. **Add cooldown for failed files** - Prevent immediate re-processing
3. **Tray notifications** - Show Windows toast when files fail
4. **Version bump to 0.4.6** - After Skyline fix is complete
5. **Create GitHub release** - With installer and changelog
