# MD Local QC Agent

A passive, vendor-agnostic telemetry service that extracts EvoSep-aligned system-suitability signals from completed MS runs using Skyline headlessly.

## Overview

The MD Local QC Agent monitors mass spectrometry instruments for completed QC runs, extracts targeted metrics using Skyline, and uploads derived QC data to the Mass Dynamics cloud for longitudinal system-suitability monitoring.

**Key Features:**
- Vendor-agnostic file detection (Thermo, Bruker, Sciex, Waters, Agilent)
- EvoSep kit control alignment (SSC0, QC_A, QC_B)
- Headless Skyline extraction
- Reliable local spooling with offline support
- Secure cloud upload with mTLS

## Installation

### Prerequisites

1. **Windows 10/11 or Windows Server 2016+** (x64)
2. **Skyline** - Download from [skyline.ms](https://skyline.ms)
3. **Vendor raw file readers** - Required for the vendors you want to support

### Install

```powershell
# Download the latest MSI
# Run the installer
msiexec /i MassDynamicsQC.msi

# Or for silent install
msiexec /i MassDynamicsQC.msi /qn
```

### Configuration

1. Copy the example configuration:
   ```powershell
   Copy-Item "C:\Program Files\MassDynamics\QC\config.example.toml" `
             "C:\ProgramData\MassDynamics\QC\config.toml"
   ```

2. Edit the configuration to add your instruments:
   ```toml
   [[instruments]]
   id = "TIMSTOF01"
   vendor = "bruker"
   watch_path = "D:\\Data\\TIMSTOF01"
   file_pattern = "*.d"
   template = "evosep_hela_qc_v1.sky"
   ```

3. Place your Skyline template in:
   ```
   C:\ProgramData\MassDynamics\QC\templates\evosep_hela_qc_v1.sky
   ```

4. Run the doctor command to verify setup:
   ```powershell
   mdqc doctor
   ```

## Usage

### Commands

```
mdqc run              Run the agent (normally as service)
mdqc run --foreground Run in foreground for testing
mdqc doctor           Check system health
mdqc classify <path>  Preview run classification
mdqc status           Show agent status
mdqc baseline list    List baselines
mdqc config validate  Validate configuration
```

### Running as a Service

The installer automatically creates and starts the Windows service. To manage manually:

```powershell
# Start the service
Start-Service MassDynamicsQC

# Stop the service
Stop-Service MassDynamicsQC

# Check status
Get-Service MassDynamicsQC
```

### File Naming Convention

For automatic classification, name your QC files following this pattern:

```
{INSTRUMENT}_{CONTROL_TYPE}_{WELL}_{DATE}.{ext}

Examples:
TIMSTOF01_SSC0_A1_2026-01-27.d
EXPLORIS01_QCB_A3_2026-01-27.raw
```

Control types:
- `SSC0` - System Suitability Control (baseline)
- `QCA` - 500ng lysate full workflow
- `QCB` - 50ng digest LCMS + loading

## Development

### Building

```bash
# Clone the repository
git clone https://github.com/MassDynamics/mdqc-agent.git
cd mdqc-agent

# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run tests
cargo test
```

### Cross-Platform Development

The agent is designed to run on Windows but can be developed on macOS/Linux:

```bash
# Build for Windows from macOS (requires cross-compilation setup)
cargo build --target x86_64-pc-windows-msvc

# Or use GitHub Actions for Windows builds
```

### Project Structure

```
mdqc-agent/
├── src/
│   ├── main.rs           # Entry point
│   ├── cli/              # CLI commands
│   ├── config/           # Configuration management
│   ├── watcher/          # File detection & finalization
│   ├── classifier/       # Run classification
│   ├── extractor/        # Skyline extraction
│   ├── spool/            # Local reliability layer
│   ├── uploader/         # Cloud upload
│   ├── baseline/         # Baseline management
│   ├── metrics/          # Metrics computation
│   └── service/          # Windows service integration
├── Cargo.toml
├── config.example.toml
└── README.md
```

## License

Apache 2.0

## Contributing

Contributions are welcome! Please read our contributing guidelines before submitting PRs.

## Support

- Documentation: https://docs.massdynamics.com/qc-agent
- Issues: https://github.com/MassDynamics/mdqc-agent/issues
- Email: support@massdynamics.com
