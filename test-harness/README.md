# MD QC Agent Test Harness

A test suite for validating the MD QC Agent's file watching and processing capabilities.

## Overview

This test harness simulates mass spectrometry file acquisitions from various vendors to verify that the MD QC Agent correctly:

1. **Detects new files** in watch folders
2. **Waits for stability** (files stop changing)
3. **Respects lock files** (vendor-specific acquisition indicators)
4. **Handles timeouts** gracefully
5. **Records failures** for later retry

## Requirements

- Python 3.7+
- MD QC Agent installed and configured
- A watch folder configured in the agent's config.toml

## Quick Start

### 1. Simulate a Thermo .raw file acquisition

```powershell
python simulate_acquisition.py --watch-folder "D:\TestData" --vendor thermo
```

### 2. Monitor the agent's response

```powershell
python monitor_agent.py --follow
```

### 3. Check agent health

```powershell
python monitor_agent.py --health-check
```

## Scripts

### simulate_acquisition.py

Simulates file acquisition for different MS vendors.

**Supported Vendors:**
- `thermo` - Creates `.raw` files (single binary file)
- `bruker` - Creates `.d` folders with `analysis.tdf` and lock files
- `agilent` - Creates `.d` folders with `AcqData/MSScan.bin`
- `waters` - Creates `.raw` folders with `_FUNC001.DAT` and lock files

**Options:**

| Option | Description |
|--------|-------------|
| `--watch-folder`, `-w` | Path to the watch folder (required) |
| `--vendor`, `-v` | Vendor(s) to simulate (default: thermo) |
| `--duration`, `-d` | Acquisition duration in seconds (default: 10) |
| `--cleanup`, `-c` | Clean up test files after completion |
| `--test-timeout` | Test timeout handling (file never finishes) |
| `--run-name` | Custom run name (default: auto-generated) |

**Examples:**

```powershell
# Single vendor test
python simulate_acquisition.py -w "D:\TestData" -v thermo

# Multiple vendors
python simulate_acquisition.py -w "D:\TestData" -v thermo bruker agilent

# Longer acquisition (30 seconds)
python simulate_acquisition.py -w "D:\TestData" -v thermo -d 30

# Clean up after test
python simulate_acquisition.py -w "D:\TestData" -v thermo --cleanup

# Test timeout handling
python simulate_acquisition.py -w "D:\TestData" -v thermo --test-timeout
```

### monitor_agent.py

Monitors the MD QC Agent's status, logs, and failed files.

**Options:**

| Option | Description |
|--------|-------------|
| `--follow`, `-f` | Continuously follow log output |
| `--health-check` | Run health check and exit |
| `--log-dir` | Custom log directory path |
| `--data-dir` | Custom data directory path |
| `--verbose`, `-v` | Show verbose output |

**Examples:**

```powershell
# Show current status
python monitor_agent.py

# Follow logs in real-time
python monitor_agent.py --follow

# Health check (for CI/automation)
python monitor_agent.py --health-check
```

## Test Scenarios

### Scenario 1: Normal Acquisition

Tests that files are detected and processed after the stability window.

```powershell
# Start monitoring in one terminal
python monitor_agent.py --follow

# In another terminal, simulate acquisition
python simulate_acquisition.py -w "D:\TestData" -v thermo -d 10

# Expected: File detected, stabilizes after ~60s, then processed
```

### Scenario 2: Lock File Handling (Bruker/Waters)

Tests that the agent waits for lock files to be removed.

```powershell
python simulate_acquisition.py -w "D:\TestData" -v bruker -d 20

# Expected: Agent waits while lock file exists, processes after removal
```

### Scenario 3: Timeout Handling

Tests that files that never stabilize are marked as failed.

```powershell
python simulate_acquisition.py -w "D:\TestData" -v thermo --test-timeout

# Let it run for a few minutes, then Ctrl+C
# Expected: File appears in "mdqc failed list"
```

### Scenario 4: Multiple Vendors

Tests simultaneous handling of different file formats.

```powershell
python simulate_acquisition.py -w "D:\TestData" -v thermo bruker agilent -d 15 --cleanup
```

## Integration with CI/CD

The monitor script can be used for automated health checks:

```powershell
python monitor_agent.py --health-check
if ($LASTEXITCODE -ne 0) {
    Write-Error "Agent health check failed"
    exit 1
}
```

## File Structures Created

### Thermo (.raw)
```
QC_20240115_123456_ABCD.raw  (single binary file)
```

### Bruker (.d)
```
QC_20240115_123456_ABCD.d/
├── analysis.tdf
└── analysis.tdf-journal  (lock file, removed when complete)
```

### Agilent (.d)
```
QC_20240115_123456_ABCD.d/
└── AcqData/
    └── MSScan.bin
```

### Waters (.raw)
```
QC_20240115_123456_ABCD.raw/
├── _FUNC001.DAT
├── _extern.inf  (created when complete)
└── _LOCK_       (lock file, removed when complete)
```

## Troubleshooting

### Agent not detecting files

1. Check the watch folder path in config.toml matches your test folder
2. Check the file_pattern matches (e.g., `*.raw` for Thermo)
3. Run `mdqc doctor` to verify configuration

### Files stuck in "stabilizing" state

1. The stability window is 60 seconds by default
2. Check that the simulation has finished writing
3. For Bruker/Waters, ensure lock files were removed

### Files going to "failed" state

1. Check `mdqc failed list` for the reason
2. Common causes: timeout, Skyline errors, template issues
3. Use `mdqc failed retry <path>` to retry

## Contributing

When adding support for new vendors:

1. Add a new simulator class in `simulate_acquisition.py`
2. Implement the correct file structure and lock file behavior
3. Add test scenarios to this README
4. Test with the actual MD QC Agent
