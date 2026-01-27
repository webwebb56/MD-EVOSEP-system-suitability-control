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

Download the latest release and run:

```powershell
# Standard install
.\mdqc-setup.exe

# Or silent install
.\mdqc-setup.exe /S
```

### 3. Configure Your Instrument

Edit the configuration file at `C:\ProgramData\MassDynamics\QC\config.toml`:

```toml
# Add your instrument
[[instruments]]
id = "MY_INSTRUMENT"           # A name for your instrument
vendor = "thermo"              # thermo, bruker, sciex, waters, or agilent
watch_path = "D:\\Data"        # Where your raw files are saved
file_pattern = "*.raw"         # File extension to watch
template = "qc_template.sky"   # Your Skyline template (see below)
```

### 4. Create a Skyline Template

The agent needs a Skyline document with your QC targets (e.g., iRT peptides):

1. Open Skyline
2. Add your QC peptides/targets
3. Configure extraction settings for your acquisition method (DDA/DIA)
4. Save as `C:\ProgramData\MassDynamics\QC\templates\qc_template.sky`

### 5. Verify Setup

```powershell
mdqc doctor
```

You should see all green checkmarks. If not, see [Troubleshooting](#troubleshooting).

### 6. Start Monitoring

The agent runs automatically as a Windows service. To check status:

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
template = "orbitrap_qc.sky"

[[instruments]]
id = "TIMSTOF01"
vendor = "bruker"
watch_path = "D:\\Data\\timsTOF"
file_pattern = "*.d"
template = "tims_qc.sky"
```

### Multiple Instruments

Add multiple `[[instruments]]` sections to monitor several instruments from one PC:

```toml
[[instruments]]
id = "EXPLORIS01"
vendor = "thermo"
watch_path = "\\\\server\\data\\Exploris01"
template = "orbitrap_qc.sky"

[[instruments]]
id = "EXPLORIS02"
vendor = "thermo"
watch_path = "\\\\server\\data\\Exploris02"
template = "orbitrap_qc.sky"
```

## Commands

| Command | Description |
|---------|-------------|
| `mdqc doctor` | Check system health and configuration |
| `mdqc status` | Show current queue and recent activity |
| `mdqc classify <file>` | Preview how a file would be classified |
| `mdqc run --foreground` | Run in foreground (for testing) |
| `mdqc config validate` | Check configuration file for errors |

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

Ensure your Skyline template exists at the path specified. Templates should be in:
```
C:\ProgramData\MassDynamics\QC\templates\
```

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
