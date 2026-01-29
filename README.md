# MD Local QC Agent

**Automated quality control monitoring for mass spectrometry instruments**

Monitor your instrument performance over time by automatically tracking key metrics from QC runs. The agent watches for new QC files, extracts performance data using Skyline, and uploads results to Mass Dynamics for visualization and alerting.

## What Does It Do?

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  Instrument     │    │   QC Agent      │    │    Skyline      │    │ Mass Dynamics   │
│  completes run  │───▶│   detects file  │───▶│   extracts      │───▶│   tracks &      │
│                 │    │                 │    │   metrics       │    │   alerts        │
└─────────────────┘    └─────────────────┘    └─────────────────┘    └─────────────────┘
```

**The agent automatically:**
- Detects when QC runs complete on your instrument
- Identifies the control type from the filename (SSC0, QC_A, QC_B)
- Runs Skyline to extract retention times, peak areas, and mass accuracy
- Uploads metrics to Mass Dynamics for trend monitoring
- Works offline and syncs when connection is restored

## Supported Instruments

| Vendor | File Format | Status |
|--------|-------------|--------|
| Thermo | `.raw` | Supported |
| Bruker | `.d` | Supported |
| Sciex | `.wiff` | Supported |
| Waters | `.raw` (folder) | Supported |
| Agilent | `.d` | Supported |

## Quick Start

### 1. Install Prerequisites

**Skyline** (free, required for data extraction):
- Download from [skyline.ms](https://skyline.ms/project/home/software/Skyline/begin.view)
- Run the installer - no configuration needed

**Vendor Libraries** (for your instrument type):
- Thermo: Included with Skyline
- Bruker: Install timsdata library from Bruker
- Others: See [Skyline documentation](https://skyline.ms/wiki/home/software/Skyline/page.view?name=InstallExternalTools)

### 2. Install the QC Agent

Download `mdqc-setup-vX.X.X.exe` from the [latest release](https://github.com/webwebb56/MD-EVOSEP-system-suitability-control/releases/latest) and run it.

The installer will:
- Install MD QC Agent to Program Files
- Create configuration directories
- Add a system tray icon (look for the MD logo)
- Optionally start automatically when Windows boots

After installation, the MD QC Agent icon will appear in your system tray. Right-click it to access settings and diagnostics.

```powershell
# For silent/unattended install:
.\mdqc-setup-vX.X.X.exe /VERYSILENT
```

### 3. Configure Your Instrument

**Option A: Use the system tray** (recommended)
- Right-click the MD icon in the system tray
- Click "Edit Configuration..."
- Edit the config file that opens in Notepad

**Option B: Edit directly**
- Open `C:\ProgramData\MassDynamics\QC\config.toml`

```toml
# Add your instrument
[[instruments]]
id = "MY_INSTRUMENT"           # A name for your instrument
vendor = "thermo"              # thermo, bruker, sciex, waters, or agilent
watch_path = "D:\\Data"        # Where your raw files are saved
file_pattern = "*.raw"         # File extension to watch
template = "C:\\ProgramData\\MassDynamics\\QC\\methods\\QC_Method.sky"
```

### 4. Create the QC Method File

The agent requires a Skyline document (`QC_Method.sky`) containing:
- Your QC target peptides (e.g., iRT standards)
- Full-Scan settings matching your acquisition method
- A report named **exactly** `MD_QC_Report`

#### Step 4a: Create the Document with QC Targets

1. Open **Skyline**
2. Add your QC peptides:
   - For iRT peptides: **File > Import > Peptide List** and paste sequences
   - Or import from a spectral library
3. Verify precursors are listed in the Targets panel

#### Step 4b: Configure Full-Scan Settings (for DIA data)

1. Go to **Settings > Transition Settings > Full-Scan** tab
2. Configure for your instrument:

   | Setting | Value (Orbitrap example) |
   |---------|--------------------------|
   | **MS1 filtering** | |
   | Isotope peaks included | Count |
   | Precursor mass analyzer | Orbitrap |
   | Resolving power | 60,000 |
   | **MS/MS filtering** | |
   | Acquisition method | DIA |
   | Product mass analyzer | Orbitrap |
   | Resolving power | 30,000 |

3. Click **OK**

> **Note**: For DDA data, set Acquisition method to "Targeted" instead of "DIA"

#### Step 4c: Create the MD_QC_Report (CRITICAL)

The agent exports metrics using a report named **exactly** `MD_QC_Report`. You must create this report:

1. Go to **View > Document Grid**
2. Click the **Reports** dropdown (top-left of grid)
3. Click **Edit Reports...**
4. Click **Add** to create a new report
5. Name it exactly: `MD_QC_Report`
6. Add these columns from the left panel:

   | Column | Location in Skyline |
   |--------|---------------------|
   | Peptide Sequence | Proteins > Peptides > Peptide Sequence |
   | Precursor Mz | Proteins > Peptides > Precursors > Precursor Mz |
   | Peptide Retention Time | Proteins > Peptides > Precursors > Peptide Retention Time |
   | Total Area | Proteins > Peptides > Precursors > Total Area |
   | Max Height | Proteins > Peptides > Precursors > Max Height |
   | Average Mass Error PPM | Proteins > Peptides > Precursors > Average Mass Error PPM |
   | Max Fwhm | Proteins > Peptides > Precursors > Max Fwhm |

7. Click **OK** to save the report

#### Step 4d: Save the QC Method

1. **File > Save As**
2. Save to: `C:\ProgramData\MassDynamics\QC\methods\QC_Method.sky`

> **Tip**: You can access ProgramData by typing `%ProgramData%` in the File Explorer address bar

#### Step 4e: Update Configuration

In your `config.toml`, set the template path:

```toml
[[instruments]]
id = "MY_INSTRUMENT"
vendor = "thermo"
watch_path = "D:\\Data"
file_pattern = "*.raw"
template = "C:\\ProgramData\\MassDynamics\\QC\\methods\\QC_Method.sky"
```

### 5. Verify Setup

```powershell
mdqc doctor
```

You should see all green checkmarks. If not, see [Troubleshooting](#troubleshooting).

### 6. Start Monitoring

The agent runs automatically from the system tray. Look for the MD logo icon.

**System tray menu options:**
- View current status
- Edit configuration
- Open Skyline template
- Open watch folder
- View logs
- Run diagnostics

To check status from command line:
```powershell
mdqc status
```

## QC Control Types

The agent recognizes EvoSep kit controls by filename:

| Control | Filename Contains | Purpose |
|---------|-------------------|---------|
| **SSC0** | `SSC0` or `SSC` | System Suitability Control - establishes baseline performance |
| **QC_A** | `QCA` or `QC_A` | 500ng HeLa lysate - full workflow check |
| **QC_B** | `QCB` or `QC_B` | 50ng HeLa digest - sensitivity check |
| **Blank** | `BLANK` or `BLK` | Carryover monitoring |

**Recommended naming:**
```
INSTRUMENT_CONTROLTYPE_WELL_DATE.raw

Examples:
EXPLORIS01_SSC0_A1_2026-01-27.raw
TIMSTOF01_QCA_B3_2026-01-27.d
```

## Configuration Reference

### Full Configuration Example

```toml
[agent]
agent_id = "auto"                    # Unique ID (auto-generates from hardware)
log_level = "info"                   # error, warn, info, debug
enable_toast_notifications = true    # Show Windows notifications for extractions

[cloud]
endpoint = "https://qc.massdynamics.com/api/"
api_token = "your-token-here"        # From Mass Dynamics account

[skyline]
path = "auto"                        # Auto-detect Skyline installation
timeout_seconds = 300                # Max time for extraction
process_priority = "below_normal"    # Don't interfere with acquisition

[watcher]
scan_interval_seconds = 30           # How often to check for new files
stability_window_seconds = 60        # Wait for file to stop changing

[[instruments]]
id = "EXPLORIS01"
vendor = "thermo"
watch_path = "D:\\Data\\Exploris"
file_pattern = "*.raw"
template = "C:\\ProgramData\\MassDynamics\\QC\\methods\\QC_Method.sky"

[[instruments]]
id = "TIMSTOF01"
vendor = "bruker"
watch_path = "D:\\Data\\timsTOF"
file_pattern = "*.d"
template = "C:\\ProgramData\\MassDynamics\\QC\\methods\\QC_Method.sky"
```

### Multiple Instruments

Add multiple `[[instruments]]` sections to monitor several instruments from one PC:

```toml
[[instruments]]
id = "EXPLORIS01"
vendor = "thermo"
watch_path = "\\\\server\\data\\Exploris01"
file_pattern = "*.raw"
template = "C:\\ProgramData\\MassDynamics\\QC\\methods\\QC_Method.sky"

[[instruments]]
id = "EXPLORIS02"
vendor = "thermo"
watch_path = "\\\\server\\data\\Exploris02"
file_pattern = "*.raw"
template = "C:\\ProgramData\\MassDynamics\\QC\\methods\\QC_Method.sky"
```

## Commands

| Command | Description |
|---------|-------------|
| `mdqc doctor` | Check system health and configuration |
| `mdqc status` | Show current queue and recent activity |
| `mdqc classify <file>` | Preview how a file would be classified |
| `mdqc run --foreground` | Run in foreground (for testing) |
| `mdqc config validate` | Check configuration file for errors |
| `mdqc failed list` | Show files that failed extraction |
| `mdqc failed retry <path>` | Retry a specific failed file (or "all") |
| `mdqc failed clear` | Clear the failed files list |
| `mdqc gui` | Open the configuration editor GUI |

## Troubleshooting

### "Skyline not found"

The agent couldn't locate SkylineCmd.exe. Either:
- Install Skyline from [skyline.ms](https://skyline.ms)
- Or specify the path manually in config:
  ```toml
  [skyline]
  path = "C:\\Program Files\\Skyline\\SkylineCmd.exe"
  ```

### "Template not found"

Ensure your `QC_Method.sky` file exists at the path specified in your config. Default location:
```
C:\ProgramData\MassDynamics\QC\methods\QC_Method.sky
```

### "Report does not exist" or "MD_QC_Report not found"

The Skyline template is missing the required report. You must create a report named **exactly** `MD_QC_Report`:

1. Open `QC_Method.sky` in Skyline
2. Go to **View > Document Grid**
3. Click **Reports > Edit Reports...**
4. Create a report named `MD_QC_Report` with these columns:
   - Peptide Sequence, Precursor Mz, Peptide Retention Time
   - Total Area, Max Height, Average Mass Error PPM, Max Fwhm
5. **Save the document** (File > Save)

### "Does not contain SRM/MRM chromatograms"

Your Skyline template's Full-Scan settings don't match your data type:

**For DIA data:**
1. Open `QC_Method.sky` in Skyline
2. Go to **Settings > Transition Settings > Full-Scan**
3. Set MS/MS filtering Acquisition method = **DIA**
4. Set appropriate mass analyzers (Orbitrap, TOF, etc.)
5. Save the document

### "Vendor reader not detected"

Skyline needs vendor-specific libraries to read raw files:
- **Thermo**: Usually included with Skyline
- **Bruker**: Download timsdata.dll from Bruker
- **Waters**: Install MassLynx or waters_connect

Run `mdqc doctor` to see which readers are available.

### Files Not Being Detected

1. Check the watch path exists and is accessible
2. Verify the file pattern matches your files (e.g., `*.raw` vs `*.d`)
3. Ensure files are fully written before the agent checks (increase `stability_window_seconds`)
4. Check logs at `C:\ProgramData\MassDynamics\QC\logs\`

### Upload Failures

The agent queues data locally when offline. Check:
- Network connectivity to Mass Dynamics
- API token is valid
- Run `mdqc status` to see pending uploads

## File Locations

After setup, your installation should look like this:

```
C:\ProgramData\MassDynamics\QC\
├── config.toml              # Your configuration
├── methods\
│   └── QC_Method.sky        # Skyline method with targets + MD_QC_Report
├── logs\
│   └── mdqc.YYYY-MM-DD.log  # Daily log files
├── spool\
│   ├── pending\             # Results waiting to upload
│   └── completed\           # Successfully uploaded results
└── failed_files.json        # Tracking of failed extractions
```

## Logs

Logs are stored at:
```
C:\ProgramData\MassDynamics\QC\logs\mdqc.log
```

Increase verbosity in config:
```toml
[agent]
log_level = "debug"
```

## Service Management

The agent runs as a Windows service:

```powershell
# Check status
Get-Service MassDynamicsQC

# Restart
Restart-Service MassDynamicsQC

# Stop
Stop-Service MassDynamicsQC

# View logs
Get-Content C:\ProgramData\MassDynamics\QC\logs\mdqc.log -Tail 50
```

## For Developers

### Building from Source

```bash
# Clone
git clone https://github.com/webwebb56/MD-EVOSEP-system-suitability-control.git
cd mdqc-agent

# Build
cargo build --release

# Test
cargo test
```

### Project Structure

```
src/
├── cli/          # Command-line interface
├── config/       # Configuration loading
├── watcher/      # File detection
├── classifier/   # Control type identification
├── extractor/    # Skyline integration
├── spool/        # Offline queue
├── uploader/     # Cloud sync
└── baseline/     # Baseline management
```

## Support

- **Documentation**: [docs.massdynamics.com](https://docs.massdynamics.com)
- **Issues**: [GitHub Issues](https://github.com/webwebb56/MD-EVOSEP-system-suitability-control/issues)
- **Email**: support@massdynamics.com

## License

Apache 2.0 - See [LICENSE](LICENSE) for details.
