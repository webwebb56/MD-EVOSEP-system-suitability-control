# MD Local QC Agent — Technical Specification v2

**Status:** Ready for Engineering
**Last Updated:** 2026-01-27
**Authors:** Mass Dynamics + EvoSep Collaboration

---

## Executive Summary

MD Local QC Agent is a passive, vendor-agnostic telemetry service that extracts EvoSep-aligned system-suitability signals from completed MS runs using Skyline headlessly, and securely trends instrument and workflow health over time—without touching raw data or acquisition software.

---

## Table of Contents

1. [Purpose & Scope](#1-purpose--scope)
2. [Core Design Principles](#2-core-design-principles)
3. [EvoSep Control Model](#3-evosep-control-model)
4. [System Architecture](#4-system-architecture)
5. [File Detection & Finalization](#5-file-detection--finalization)
6. [Run Classification](#6-run-classification)
7. [Extraction Backend (Skyline)](#7-extraction-backend-skyline)
8. [Metrics Computation](#8-metrics-computation)
9. [Baseline Management](#9-baseline-management)
10. [Local Reliability & Spooling](#10-local-reliability--spooling)
11. [Cloud Upload & Security](#11-cloud-upload--security)
12. [Failure Handling & Alerting](#12-failure-handling--alerting)
13. [Configuration](#13-configuration)
14. [Logging & Diagnostics](#14-logging--diagnostics)
15. [Installation & Deployment](#15-installation--deployment)
16. [CLI Commands](#16-cli-commands)
17. [Security Model](#17-security-model)
18. [Payload Schema](#18-payload-schema)
19. [Build & Release](#19-build--release)
20. [Operational Guidance](#20-operational-guidance)
21. [Future Considerations](#21-future-considerations)

---

## 1. Purpose & Scope

### 1.1 What the Agent Does

- Passively detects completed MS runs (PRM or DIA) across multiple vendors
- Extracts targeted QC features using Skyline in headless mode
- Computes system suitability metrics aligned with EvoSep kit controls
- Uploads derived QC metrics only (not raw data) to the MD cloud
- Enables longitudinal system-suitability monitoring across:
  - Time
  - Instruments
  - Kit installations
  - Plates and well positions

### 1.2 What the Agent Is NOT

- ❌ An instrument controller
- ❌ A data analysis platform
- ❌ A UI application
- ❌ A raw-data transfer tool
- ❌ A vendor SDK wrapper

### 1.3 Supported Vendors (v1)

| Vendor | File Format | Priority |
|--------|-------------|----------|
| Thermo | `.raw` | P0 (launch) |
| Bruker | `.d` (directory) | P0 (launch) |
| Sciex | `.wiff` / `.wiff2` | P1 (post-launch) |
| Waters | `.raw` (directory) | P1 (post-launch) |
| Agilent | `.d` (directory) | P2 |

---

## 2. Core Design Principles

These are **non-negotiable** constraints:

| Principle | Rationale |
|-----------|-----------|
| Vendor-agnostic invocation | No control-software APIs; trigger on file finalization only |
| No raw data leaves the site | Only derived metrics are uploaded |
| No vendor SDK installation | Skyline handles vendor formats via its own readers |
| Outbound-only networking | No inbound ports; firewall-friendly |
| Install once → invisible operation | Zero ongoing user interaction required |
| Fail loudly, safely, deterministically | Predictable failure modes; no silent data loss |
| Boring, predictable behavior | Telemetry agent mindset; no surprises |

---

## 3. EvoSep Control Model

### 3.1 Control Types

| Control Type | Description | Role |
|--------------|-------------|------|
| `SSC0` | System Suitability Control | Golden reference baseline |
| `QC_A` | 500 ng lysate (full kit workflow) | End-to-end workflow sentinel |
| `QC_B` | 50 ng digest | LCMS + loading sentinel |
| `SAMPLE` | Normal sample | Ignored by default |
| `BLANK` | Blank injection | Optional monitoring |

### 3.2 Baseline Semantics

```
SSC0 establishes baseline per:
  (instrument_id, method_id, template_hash, kit_install_event)

QC_B → compared against active SSC0 baseline
QC_A → compared against active SSC0 baseline

After initial validation:
  QC_B in positions A1-A4 passing → QC_B can serve as steady-state SSC
```

### 3.3 Default Plate Positions

| Well | Control Type | Purpose |
|------|--------------|---------|
| A1, A2 | QC_A | Full workflow validation |
| A3, A4 | QC_B | LCMS + loading validation |
| Additional wells | QC_B (optional) | Intra-plate drift, inter-plate batch monitoring |

### 3.4 Comparison Logic

| Run Type | Compared Against | Purpose |
|----------|------------------|---------|
| SSC0 | (registers new baseline) | Establish reference |
| QC_B | Active SSC0 | Ongoing system suitability |
| QC_A | Active SSC0 | Full workflow integrity |

---

## 4. System Architecture

### 4.1 Deployment Model

**Required:** Windows Service
**Fallback:** Scheduled Task (only if customer IT policy prohibits services)

```
┌─────────────────────────────────────────────────────────────────┐
│                     MD Local QC Agent                            │
│                    (Windows Service)                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────┐   ┌─────────────┐   ┌───────────┐   ┌──────────┐ │
│  │ Watcher  │──▶│ Finalizer   │──▶│ Classifier│──▶│Extractor │ │
│  └──────────┘   └─────────────┘   └───────────┘   └──────────┘ │
│       │                                                  │       │
│       │ (events)                                         │       │
│       ▼                                                  ▼       │
│  ┌──────────┐                                     ┌──────────┐  │
│  │  Poller  │                                     │Normalizer│  │
│  │(fallback)│                                     └──────────┘  │
│  └──────────┘                                           │       │
│                                                         ▼       │
│                    ┌──────────┐   ┌──────────┐   ┌──────────┐  │
│                    │  Spool   │◀──│ Uploader │──▶│ MD Cloud │  │
│                    │ (local)  │   │ (async)  │   │          │  │
│                    └──────────┘   └──────────┘   └──────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 Implementation

| Aspect | Decision |
|--------|----------|
| Language | Rust |
| Deployment | Single signed MSI installer |
| Runtime | Windows Service (`windows-service` crate) |
| Service Name | `MassDynamicsQC` |
| Display Name | `Mass Dynamics QC Agent` |

### 4.3 Deployment Modes

| Mode | Description |
|------|-------------|
| Instrument PC | Agent runs on the acquisition PC (low process priority) |
| Dedicated QC Node | Agent runs on a separate PC with network access to raw data |

---

## 5. File Detection & Finalization

### 5.1 Trigger Model

The agent is triggered by **file finalization on disk**—not by vendor software.

**Universal invariant:** A run is complete when the raw data artifact (file or directory) is closed and stable.

### 5.2 Watch Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       Watch Layer                                │
├─────────────────────────────────────────────────────────────────┤
│  Primary: ReadDirectoryChangesW (notify crate)                  │
│  Fallback: Directory scan every 30 seconds                       │
│                                                                  │
│  IMPORTANT: Events are HINTS, not truth.                        │
│  Always verify via finalization state machine.                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Finalization State Machine                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  DETECTED ──▶ STABILIZING ──▶ READY ──▶ PROCESSING ──▶ DONE    │
│      │            │             │            │                   │
│      │            │             │            └─▶ FAILED          │
│      │            │             │                                │
│      │            │             └─ Non-sharing open succeeds     │
│      │            │                                              │
│      │            └─ Size + mtime stable for N seconds           │
│      │                                                           │
│      └─ File/directory detected via event or scan               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 5.3 Finalization States

| State | Entry Condition | Actions | Timeout |
|-------|-----------------|---------|---------|
| `DETECTED` | New artifact seen | Record initial size/mtime, start timer | — |
| `STABILIZING` | Timer started | Check size/mtime every 5s | 10 min |
| `READY` | Stable for `stability_window` | Attempt non-sharing open | 30s |
| `PROCESSING` | Open succeeded | Queue for extraction | — |
| `DONE` | Extraction complete | Remove from state machine | — |
| `FAILED` | Error occurred | Log, alert, optionally retry | — |

### 5.4 Vendor-Specific Handling

| Vendor | Artifact Type | Finalization Check |
|--------|---------------|-------------------|
| Thermo | `.raw` file | Size + mtime stable, non-sharing open |
| Bruker | `.d` directory | `analysis.tdf` present and stable, lock file absent |
| Sciex | `.wiff` + `.wiff.scan` | Both files stable |
| Waters | `.raw` directory | `_FUNC001.DAT` present and stable |
| Agilent | `.d` directory | `AcqData` subdirectory complete |

### 5.5 Configuration Parameters

```toml
[watcher]
# Primary watch mechanism
use_filesystem_events = true

# Fallback scan interval (seconds)
scan_interval_seconds = 30

# Stability window before processing (seconds)
stability_window_seconds = 60

# Maximum time to wait for stabilization (seconds)
stabilization_timeout_seconds = 600

# Vendor-specific overrides
[watcher.vendor_overrides.bruker]
stability_window_seconds = 90  # Bruker .d folders take longer
```

### 5.6 Network Share Considerations

- Filesystem events are unreliable on SMB/CIFS shares
- Agent MUST use poll fallback as primary on network paths
- Detect network paths via `GetDriveType()` or UNC prefix
- Log warning if watching network path with events-only

---

## 6. Run Classification

### 6.1 Filename Convention (PRESCRIPTIVE)

**Canonical format:**
```
{INSTRUMENT}_{CONTROL_TYPE}_{WELL}_{DATE}[_{SUFFIX}].{ext}
```

| Component | Format | Examples |
|-----------|--------|----------|
| `INSTRUMENT` | Alphanumeric, underscores | `TIMSTOF01`, `EXPLORIS_01` |
| `CONTROL_TYPE` | `SSC0`, `QCA`, `QCB`, `SAMPLE`, `BLANK` | `QCB` |
| `WELL` | `[A-H][1-12]` | `A1`, `A3`, `E5` |
| `DATE` | `YYYY-MM-DD` or `YYYYMMDD` | `2026-01-27` |
| `SUFFIX` | Optional free-form | `rep1`, `batch2` |
| `ext` | Vendor extension | `.raw`, `.d` |

**Examples:**
```
TIMSTOF01_SSC0_A1_2026-01-27.d
EXPLORIS01_QCB_A3_2026-01-27_rep1.raw
TIMSTOF01_QCA_A1_20260127.d
```

### 6.2 Token Matching Rules

1. Tokens are **case-insensitive**
2. Tokens can be separated by `_`, `-`, or `.`
3. Control type tokens:
   - `SSC0`, `SSC_0`, `SSC-0` → `SSC0`
   - `QCA`, `QC_A`, `QC-A`, `QC_A` → `QC_A`
   - `QCB`, `QC_B`, `QC-B`, `QC_B` → `QC_B`
   - `BLANK`, `BLK` → `BLANK`
   - No match → `SAMPLE`

### 6.3 Classification Priority

1. **Filename tokens** (primary)
2. **Sample name in vendor metadata** (if accessible without SDK)
3. **Well position inference** (A1-A2 = QC_A, A3-A4 = QC_B)
4. **Default to SAMPLE** if no match

### 6.4 Classification Output

```rust
struct RunClassification {
    control_type: ControlType,      // SSC0 | QC_A | QC_B | SAMPLE | BLANK
    well_position: Option<WellId>,  // e.g., A3
    instrument_id: String,          // e.g., TIMSTOF01
    plate_id: Option<String>,       // if determinable
    confidence: ClassificationConfidence,  // HIGH | MEDIUM | LOW
    source: ClassificationSource,   // FILENAME | METADATA | POSITION | DEFAULT
}
```

---

## 7. Extraction Backend (Skyline)

### 7.1 Invocation Model

- **Backend:** SkylineCmd.exe (headless CLI)
- **Document:** Prebuilt `.sky` file owned by EvoSep/users
- **Agent role:** Orchestrator only; never parses raw files directly

### 7.2 Skyline Discovery

**Discovery order (first match wins):**

1. Explicit config: `skyline_path` in config file
2. Registry: `HKLM\SOFTWARE\ProteoWizard\Skyline\InstallPath`
3. Known paths:
   - `C:\Program Files\Skyline\SkylineCmd.exe`
   - `C:\Program Files (x86)\Skyline\SkylineCmd.exe`
4. PATH environment variable

### 7.3 Skyline Invocation

```bash
SkylineCmd.exe \
  --in="{template_path}" \
  --import-file="{raw_file_path}" \
  --report-name="MD_QC_Report" \
  --report-file="{output_csv_path}" \
  --report-format=csv
```

**Process constraints:**
- Timeout: 300 seconds (configurable)
- Priority: `BELOW_NORMAL_PRIORITY_CLASS`
- Working directory: spool directory
- Capture stdout/stderr for diagnostics

### 7.4 Skyline Document Ownership

| Owner | Responsibility |
|-------|----------------|
| EvoSep / User | Creates and maintains `.sky` template |
| EvoSep / User | Edits peptide targets in Skyline UI |
| Agent | Treats `.sky` as opaque contract |
| Agent | Records template hash in payload |
| MD Cloud | Segments trends when template hash changes |

### 7.5 Template Management

```
Location: C:\ProgramData\MassDynamics\QC\templates\
Naming:   {template_name}_v{version}.sky

Example:  evosep_hela_qc_v1.sky
```

**Template metadata recorded:**
- File path
- SHA-256 hash
- Skyline version used to create
- Last modified timestamp

### 7.6 Vendor Reader Requirements

Skyline requires vendor-specific readers. The agent does NOT install these.

| Vendor | Reader | Detection Method |
|--------|--------|------------------|
| Thermo | MSFileReader or RawFileReader | Try to open test file |
| Bruker | Bruker Compass | Check for `timsdata.dll` |
| Sciex | Sciex Data Access | Check registry |
| Waters | Waters Raw SDK | Check registry |

---

## 8. Metrics Computation

### 8.1 Target-Level Metrics

| Metric | Unit | Description |
|--------|------|-------------|
| `retention_time` | minutes | Observed RT |
| `rt_delta` | minutes | RT - expected RT |
| `peak_area` | arbitrary | Integrated peak area |
| `peak_height` | arbitrary | Maximum intensity |
| `peak_width_fwhm` | minutes | Full width at half maximum |
| `peak_symmetry` | ratio | Asymmetry factor |
| `mass_error_ppm` | ppm | (observed - expected) / expected × 10⁶ |
| `isotope_dot_product` | 0-1 | Isotope distribution match |
| `fragment_ratios` | array | For PRM: fragment ion ratios |

### 8.2 Run-Level Metrics

| Metric | Description |
|--------|-------------|
| `targets_found` | Count of targets detected |
| `targets_expected` | Count of targets in template |
| `target_recovery_pct` | `targets_found / targets_expected × 100` |
| `median_rt_shift` | Median RT delta across targets |
| `median_mass_error_ppm` | Median mass error |
| `total_ion_current` | Sum of TIC (if available) |
| `chromatography_score` | Composite peak quality score |

### 8.3 Comparison Computation

| Run Type | Reference | Computed Deltas |
|----------|-----------|-----------------|
| QC_B | Active SSC0 | All target + run metrics vs baseline |
| QC_A | Active SSC0 | All target + run metrics vs baseline |
| SSC0 | Previous SSC0 (if exists) | Optional trend tracking |

---

## 9. Baseline Management

### 9.1 Baseline Identity

A baseline is uniquely identified by:
```
baseline_key = hash(instrument_id, method_id, template_hash, kit_install_id)
```

### 9.2 Baseline Lifecycle

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  CANDIDATE  │────▶│  VALIDATING │────▶│   ACTIVE    │
└─────────────┘     └─────────────┘     └─────────────┘
      │                   │                    │
      │                   │                    │
      ▼                   ▼                    ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  REJECTED   │     │   FAILED    │     │  ARCHIVED   │
└─────────────┘     └─────────────┘     └─────────────┘
```

### 9.3 Baseline State Transitions

| Event | From State | To State | Notes |
|-------|------------|----------|-------|
| SSC0 run detected | — | CANDIDATE | New baseline candidate created |
| SSC0 passes validation | CANDIDATE | VALIDATING | Cloud-side validation begins |
| Validation succeeds | VALIDATING | ACTIVE | Becomes active baseline |
| Validation fails | VALIDATING | FAILED | Alert generated |
| Kit change event | ACTIVE | ARCHIVED | Manual trigger required |
| Template hash changes | ACTIVE | ARCHIVED | Automatic |
| New baseline activated | ACTIVE | ARCHIVED | Only one active per key |
| Manual rejection | CANDIDATE | REJECTED | User action |

### 9.4 Baseline Policy Rules

| Rule | Behavior |
|------|----------|
| Auto-reset on QC failure | **NO** — Never auto-reset baseline |
| Consecutive failures | Alert after 3, but do NOT reset |
| Template change | **REQUIRES** new SSC0 baseline |
| Kit change | **REQUIRES** explicit trigger + new SSC0 |
| Baseline retention | Keep last 3 baselines per key |
| Baseline immutability | Once ACTIVE, baseline metrics are frozen |
| Reset authority | Cloud admin or explicit `mdqc baseline reset` command |

### 9.5 Baseline Validation Criteria

A baseline candidate must meet:
- All expected targets detected
- Peak areas within expected range
- RT values within expected range
- No chromatographic anomalies

---

## 10. Local Reliability & Spooling

### 10.1 Spool Architecture

All results are written to local spool before upload:

```
C:\ProgramData\MassDynamics\QC\spool\
├── pending\
│   ├── {run_uuid}_payload.json
│   └── {run_uuid}_payload.json
├── uploading\
│   └── {run_uuid}_payload.json
├── failed\
│   └── {run_uuid}_payload.json
└── completed\
    └── (empty, or last N for debugging)
```

### 10.2 Spool Workflow

1. Extraction completes → write to `pending/`
2. Uploader picks from `pending/` → move to `uploading/`
3. Upload succeeds → move to `completed/` (or delete)
4. Upload fails → retry with backoff, eventually move to `failed/`

### 10.3 Reliability Guarantees

| Guarantee | Implementation |
|-----------|----------------|
| No data loss | Spool to disk before any processing |
| Idempotent uploads | `run_uuid` in payload; cloud deduplicates |
| Offline operation | Spool locally indefinitely (within disk limits) |
| Crash recovery | Service restart resumes from spool |
| Atomic writes | Write to temp file, then rename |

### 10.4 Spool Limits

```toml
[spool]
max_pending_mb = 1000          # 1 GB max pending
max_age_days = 30              # Discard payloads older than 30 days
completed_retention_count = 10 # Keep last 10 for debugging
```

---

## 11. Cloud Upload & Security

### 11.1 Transport

| Aspect | Specification |
|--------|---------------|
| Protocol | HTTPS (TLS 1.2+) |
| Direction | Outbound only |
| Ports | 443 |
| Proxy support | System proxy + explicit config |

### 11.2 Endpoints

| Environment | Endpoint |
|-------------|----------|
| Production | `https://qc-ingest.massdynamics.com/v1/` |
| Staging | `https://qc-ingest.staging.massdynamics.com/v1/` |

### 11.3 Authentication

**Method:** Mutual TLS (mTLS)

| Aspect | Specification |
|--------|---------------|
| Client certificate | X.509, issued by MD CA |
| Storage | Windows Certificate Store (LocalMachine\My) |
| Private key | Non-exportable |
| Reference | By thumbprint in config |

### 11.4 Connectivity Modes

| Mode | Description |
|------|-------------|
| Standard | Public HTTPS + mTLS |
| Enterprise | AWS PrivateLink + private DNS + mTLS |

### 11.5 Upload Retry Policy

```
Retry attempts: 5
Backoff: exponential with jitter
  Attempt 1: immediate
  Attempt 2: 30s ± 10s
  Attempt 3: 2m ± 30s
  Attempt 4: 10m ± 2m
  Attempt 5: 1h ± 10m
After 5 failures: move to failed/, alert
```

---

## 12. Failure Handling & Alerting

### 12.1 Failure Taxonomy

| Failure | Local Behavior | Cloud Notification | Severity |
|---------|----------------|-------------------|----------|
| Skyline not found | Halt processing, log | Yes | CRITICAL |
| Template missing | Halt processing for instrument | Yes | CRITICAL |
| Skyline extraction fails | Retry once, spool error | Yes (with stderr) | ERROR |
| Raw file unreadable | Skip, log, continue | Yes | WARNING |
| Cloud unreachable | Spool locally, retry | N/A | WARNING |
| Disk full (spool) | Stop processing, alert | If possible | CRITICAL |
| QC deviates >3σ | Process normally, flag | Yes (outlier flag) | INFO |
| Certificate expiring | Warn in logs | Yes | WARNING |
| Certificate expired | Degraded mode | If possible | CRITICAL |

### 12.2 Local Alerting

**Windows Event Log:**
- Source: `MassDynamicsQC`
- Log: Application
- Event IDs:
  - 1000-1099: Informational
  - 1100-1199: Warnings
  - 1200-1299: Errors
  - 1300-1399: Critical

**Optional: Windows Toast Notification**
- Only for CRITICAL failures
- Disabled by default
- Enable via config: `enable_toast_notifications = true`

### 12.3 Cloud Alerting

Alerts sent to MD cloud include:
- Alert type and severity
- Instrument ID
- Timestamp
- Error details / stack trace
- Agent version
- Last successful upload timestamp

---

## 13. Configuration

### 13.1 Config File Location

```
Primary:   C:\ProgramData\MassDynamics\QC\config.toml
Override:  --config flag on CLI
```

### 13.2 Config Schema

```toml
# MD Local QC Agent Configuration

[agent]
# Unique identifier for this agent instance
agent_id = "auto"  # "auto" = generate from hardware ID

# Log level: error, warn, info, debug, trace
log_level = "info"

# Enable Windows toast notifications for critical errors
enable_toast_notifications = false

[cloud]
# Cloud endpoint
endpoint = "https://qc-ingest.massdynamics.com/v1/"

# Certificate thumbprint (from Windows cert store)
certificate_thumbprint = "A1B2C3D4E5F6..."

# Proxy settings (optional)
# proxy = "http://proxy.corp.local:8080"

[skyline]
# Path to SkylineCmd.exe (optional, will auto-discover)
# path = "C:\\Program Files\\Skyline\\SkylineCmd.exe"

# Extraction timeout in seconds
timeout_seconds = 300

# Process priority: normal, below_normal, idle
process_priority = "below_normal"

[watcher]
# Enable filesystem event watching
use_filesystem_events = true

# Fallback scan interval (seconds)
scan_interval_seconds = 30

# Stability window before processing (seconds)
stability_window_seconds = 60

# Maximum stabilization wait (seconds)
stabilization_timeout_seconds = 600

[spool]
# Maximum pending spool size in MB
max_pending_mb = 1000

# Maximum age of spooled items in days
max_age_days = 30

# Number of completed items to retain
completed_retention_count = 10

[[instruments]]
id = "TIMSTOF01"
vendor = "bruker"
watch_path = "D:\\Data\\TIMSTOF01"
file_pattern = "*.d"
template = "evosep_hela_qc_v1.sky"

[[instruments]]
id = "EXPLORIS01"
vendor = "thermo"
watch_path = "D:\\Data\\Exploris"
file_pattern = "*.raw"
template = "evosep_hela_qc_v1.sky"

# Vendor-specific overrides (optional)
[instruments.watcher_overrides]
stability_window_seconds = 90
```

### 13.3 Secrets Management

| Secret | Storage |
|--------|---------|
| Client certificate | Windows Certificate Store |
| Private key | Windows Certificate Store (non-exportable) |

**Never store in config file:**
- Private keys
- Passwords
- API tokens

---

## 14. Logging & Diagnostics

### 14.1 Log Location

```
C:\ProgramData\MassDynamics\QC\logs\
├── mdqc.log           # Current log
├── mdqc.log.1         # Previous
├── mdqc.log.2
└── ...
```

### 14.2 Log Rotation

| Parameter | Value |
|-----------|-------|
| Max file size | 10 MB |
| Max files | 10 |
| Total max | 100 MB |

### 14.3 Log Format

**Structured JSON (default):**
```json
{
  "timestamp": "2026-01-27T14:30:00.123Z",
  "level": "INFO",
  "target": "mdqc::watcher",
  "message": "File detected",
  "run_id": "abc123",
  "file_path": "D:\\Data\\TIMSTOF01_QCB_A3.d",
  "correlation_id": "req-456"
}
```

**Human-readable (optional, for debugging):**
```
2026-01-27T14:30:00.123Z INFO  [mdqc::watcher] File detected: D:\Data\TIMSTOF01_QCB_A3.d
```

### 14.4 Correlation IDs

Every run gets a `correlation_id` that appears in:
- All log entries for that run
- The uploaded payload
- Cloud-side logs

Format: `{agent_id}-{timestamp}-{random}`

---

## 15. Installation & Deployment

### 15.1 Installer

| Aspect | Specification |
|--------|---------------|
| Format | MSI (Windows Installer) |
| Build tool | WiX Toolset |
| Signing | EV code signing certificate |
| Silent install | `msiexec /i MassDynamicsQC.msi /qn` |

### 15.2 Installation Directory

```
C:\Program Files\MassDynamics\QC\
├── mdqc.exe           # Main agent binary
├── LICENSE
└── README.txt
```

### 15.3 Data Directory

```
C:\ProgramData\MassDynamics\QC\
├── config.toml
├── logs\
├── spool\
│   ├── pending\
│   ├── uploading\
│   ├── failed\
│   └── completed\
└── templates\
    └── *.sky
```

### 15.4 Service Installation

The installer creates:
- Service name: `MassDynamicsQC`
- Display name: `Mass Dynamics QC Agent`
- Startup type: Automatic (Delayed Start)
- Recovery: Restart on failure (3 attempts, then stop)
- Account: `NT SERVICE\MassDynamicsQC`

### 15.5 Required Permissions

| Path | Permission |
|------|------------|
| Raw data directories | Read |
| `C:\ProgramData\MassDynamics\QC\` | Read/Write |
| Certificate store (LocalMachine\My) | Read |
| Network (outbound HTTPS) | Allow |

### 15.6 Prerequisites

| Prerequisite | Required | Notes |
|--------------|----------|-------|
| Windows | 10/11 or Server 2016+ | x64 only |
| .NET Framework | Not required | Rust binary |
| Skyline | Yes | User must install separately |
| Vendor readers | Yes | For vendors being monitored |
| VC++ Runtime | Maybe | Depending on Rust build |

### 15.7 Enrollment Flow

```
1. Install MSI
   └─▶ Service created and started

2. First launch
   └─▶ Agent generates device identity
   └─▶ Agent attempts cloud enrollment

3. Enrollment request sent to cloud
   └─▶ Cloud provisions device record
   └─▶ Cloud issues client certificate
   └─▶ Agent stores cert in Windows cert store

4. First successful QC upload
   └─▶ Cloud provisions dashboard
   └─▶ Cloud sends magic-link email to admin

5. User clicks email link
   └─▶ Views QC dashboard
```

### 15.8 Upgrade Behavior

- MSI handles upgrade-in-place
- Service stopped during upgrade
- Config file preserved
- Spool directory preserved
- Logs preserved

### 15.9 Uninstall Behavior

**Default uninstall:**
- Removes service
- Removes `C:\Program Files\MassDynamics\QC\`
- **Preserves** `C:\ProgramData\MassDynamics\QC\`

**Clean uninstall (optional property):**
```
msiexec /x MassDynamicsQC.msi REMOVE_DATA=1
```
- Removes everything including data directory

---

## 16. CLI Commands

### 16.1 Command Overview

```
mdqc <command> [options]

Commands:
  run         Run the agent (normally called by service)
  doctor      Check system health and dependencies
  classify    Preview run classification without processing
  status      Show agent status and queue
  baseline    Manage baselines
  config      Validate or show configuration
  version     Show version information
```

### 16.2 `mdqc doctor`

Checks system health and reports issues:

```
$ mdqc doctor

MD Local QC Agent - System Health Check
========================================

[OK] Agent version: 1.0.0
[OK] Config file: C:\ProgramData\MassDynamics\QC\config.toml
[OK] Config syntax: valid

Skyline
-------
[OK] SkylineCmd.exe: C:\Program Files\Skyline\SkylineCmd.exe
[OK] Skyline version: 24.1.0.198 (minimum: 23.1)

Vendor Readers
--------------
[OK] Thermo RawFileReader: installed
[OK] Bruker timsdata.dll: found
[--] Sciex: not configured
[--] Waters: not configured

Templates
---------
[OK] evosep_hela_qc_v1.sky: found
    Hash: sha256:a1b2c3d4...
    Targets: 25 peptides

Instruments
-----------
[OK] TIMSTOF01: D:\Data\TIMSTOF01 (accessible)
[OK] EXPLORIS01: D:\Data\Exploris (accessible)

Certificates
------------
[OK] Client certificate: valid
    Thumbprint: A1B2C3D4...
    Expires: 2028-03-15 (754 days)

Cloud Connectivity
------------------
[OK] Endpoint: https://qc-ingest.massdynamics.com
[OK] mTLS handshake: successful

Spool
-----
[OK] Spool directory: writable
[OK] Pending items: 0
[OK] Failed items: 0

Overall: HEALTHY
```

### 16.3 `mdqc classify`

Preview classification without processing:

```
$ mdqc classify "D:\Data\TIMSTOF01_QCB_A3_2026-01-27.d"

Classification Result
=====================
File: D:\Data\TIMSTOF01_QCB_A3_2026-01-27.d
Control Type: QC_B
Well Position: A3
Instrument: TIMSTOF01
Date: 2026-01-27
Confidence: HIGH
Source: FILENAME

Baseline Binding
----------------
Would compare against: SSC0 baseline
  Baseline ID: base_abc123
  Established: 2026-01-15
  Template: evosep_hela_qc_v1.sky
```

### 16.4 `mdqc status`

Show agent status:

```
$ mdqc status

Agent Status
============
Service: running
Uptime: 3d 14h 22m
Last heartbeat: 2026-01-27T14:30:00Z

Queue
-----
Pending: 0
Processing: 1
Failed: 0

Recent Activity
---------------
2026-01-27 14:25  TIMSTOF01_QCB_A3.d  uploaded
2026-01-27 14:20  TIMSTOF01_QCA_A1.d  uploaded
2026-01-27 10:15  EXPLORIS01_QCB_A3.raw  uploaded
```

### 16.5 `mdqc baseline`

Manage baselines:

```
$ mdqc baseline list

Baselines for TIMSTOF01
=======================
[ACTIVE]   base_abc123  2026-01-15  evosep_hela_qc_v1.sky
[ARCHIVED] base_xyz789  2025-12-01  evosep_hela_qc_v1.sky
[ARCHIVED] base_def456  2025-10-15  evosep_hela_qc_v0.sky

$ mdqc baseline reset --instrument TIMSTOF01 --confirm

WARNING: This will archive the current baseline.
A new SSC0 run will be required to establish a new baseline.
Proceed? [y/N] y

Baseline archived. Awaiting new SSC0 run.
```

---

## 17. Security Model

### 17.1 Threat Model

| Threat | Mitigation |
|--------|------------|
| Raw data exfiltration | Only derived metrics uploaded; no raw spectra |
| Man-in-the-middle | mTLS with pinned CA |
| Credential theft | Non-exportable private key in cert store |
| Tampering with agent | Code signing; Windows service permissions |
| Unauthorized baseline reset | Requires explicit command + confirmation |

### 17.2 Data Classification

| Data Type | Leaves Site? | Storage |
|-----------|--------------|---------|
| Raw spectra | NO | Never accessed |
| mzML | NO | Never generated |
| QC metrics | YES | Encrypted in transit |
| Instrument ID | YES | Pseudonymized option |
| Sample names | NO | Only QC control names |
| File paths | Partial | Only filename, not full path |

### 17.3 Certificate Lifecycle

| Event | Timeline | Action |
|-------|----------|--------|
| Initial enrollment | Day 0 | Cloud issues 2-year cert |
| Expiry warning | 30 days before | Log warning, cloud alert |
| Renewal request | 14 days before | Agent requests renewal |
| Renewal issued | On request | New cert installed |
| Expiry | Day 730 | Agent enters degraded mode |

**Degraded mode behavior:**
- Continues extracting and spooling
- Logs error on every upload attempt
- Alerts via Windows Event Log
- Retries renewal daily

---

## 18. Payload Schema

### 18.1 Top-Level Structure

```json
{
  "schema_version": "1.0",
  "payload_id": "uuid-v4",
  "agent_id": "agent-uuid",
  "agent_version": "1.0.0",
  "timestamp": "2026-01-27T14:30:00.123Z",

  "run": {
    "run_id": "uuid-v4",
    "raw_file_name": "TIMSTOF01_QCB_A3_2026-01-27.d",
    "raw_file_hash": "sha256:...",
    "acquisition_time": "2026-01-27T14:00:00Z",
    "instrument_id": "TIMSTOF01",
    "vendor": "bruker",
    "control_type": "QC_B",
    "well_position": "A3",
    "plate_id": null,
    "classification_confidence": "HIGH",
    "classification_source": "FILENAME"
  },

  "extraction": {
    "backend": "skyline",
    "backend_version": "24.1.0.198",
    "template_name": "evosep_hela_qc_v1.sky",
    "template_hash": "sha256:...",
    "extraction_time_ms": 45000,
    "status": "SUCCESS"
  },

  "baseline_context": {
    "baseline_id": "base_abc123",
    "baseline_established": "2026-01-15T10:00:00Z",
    "baseline_template_hash": "sha256:..."
  },

  "target_metrics": [
    {
      "target_id": "PEPTIDE_1",
      "peptide_sequence": "EXAMPLE[+57]PEPTIDE",
      "precursor_mz": 500.1234,
      "retention_time": 12.34,
      "rt_expected": 12.30,
      "rt_delta": 0.04,
      "peak_area": 1.23e8,
      "peak_height": 4.56e7,
      "peak_width_fwhm": 0.15,
      "peak_symmetry": 1.05,
      "mass_error_ppm": 2.3,
      "isotope_dot_product": 0.98,
      "detected": true
    }
  ],

  "run_metrics": {
    "targets_found": 24,
    "targets_expected": 25,
    "target_recovery_pct": 96.0,
    "median_rt_shift": 0.03,
    "median_mass_error_ppm": 1.8,
    "chromatography_score": 0.95
  },

  "comparison_metrics": {
    "vs_baseline": {
      "rt_shift_mean": 0.02,
      "rt_shift_std": 0.01,
      "area_ratio_mean": 0.98,
      "area_ratio_std": 0.05,
      "outlier_targets": []
    }
  }
}
```

### 18.2 Explicit Exclusions

**Never include:**
- ❌ Raw spectra
- ❌ mzML data
- ❌ Full file paths
- ❌ Biological sample identifiers
- ❌ Patient/study metadata
- ❌ Vendor proprietary data beyond QC needs

---

## 19. Build & Release

### 19.1 Development Environment

| Aspect | Recommendation |
|--------|----------------|
| Primary dev | macOS or Linux |
| Build target | Windows (via CI) |
| CI platform | GitHub Actions |
| Windows testing | Local VM or CI |

### 19.2 Build Targets

```
Target: x86_64-pc-windows-msvc
Toolchain: stable-x86_64-pc-windows-msvc
```

### 19.3 CI Pipeline

```yaml
name: Build and Release

on:
  push:
    branches: [main]
    tags: ['v*']
  pull_request:
    branches: [main]

jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: Build
        run: cargo build --release

      - name: Run tests
        run: cargo test --release

      - name: Build MSI
        run: |
          # Install WiX
          # Build MSI

      - name: Sign binaries
        if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')
        run: |
          # Sign with EV certificate

      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: mdqc-windows
          path: |
            target/release/mdqc.exe
            installer/*.msi

  release:
    needs: build
    if: startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-latest
    steps:
      - name: Create Release
        # Upload to GitHub Releases
```

### 19.4 Code Signing

| Artifact | Signing |
|----------|---------|
| `mdqc.exe` | EV code signing (Authenticode) |
| `MassDynamicsQC.msi` | EV code signing |
| Timestamp | Yes (survives cert expiry) |

### 19.5 Release Artifacts

```
MassDynamicsQC-1.0.0-x64.msi
MassDynamicsQC-1.0.0-x64.msi.sha256
mdqc-1.0.0-x64.exe
mdqc-1.0.0-x64.exe.sha256
```

---

## 20. Operational Guidance

### 20.1 Recommended Antivirus Exclusions

| Path/Process | Reason |
|--------------|--------|
| `C:\ProgramData\MassDynamics\QC\spool\` | Frequent writes |
| `C:\ProgramData\MassDynamics\QC\logs\` | Frequent writes |
| `mdqc.exe` | Long-running process |
| `SkylineCmd.exe` | CPU-intensive extraction |
| Raw data directories | Already usually excluded |

### 20.2 Firewall Requirements

| Direction | Port | Destination | Protocol |
|-----------|------|-------------|----------|
| Outbound | 443 | `qc-ingest.massdynamics.com` | HTTPS |

### 20.3 Monitoring

**Windows Event Log queries:**
```powershell
# Recent errors
Get-EventLog -LogName Application -Source MassDynamicsQC -EntryType Error -Newest 10

# All recent events
Get-EventLog -LogName Application -Source MassDynamicsQC -Newest 50
```

**Service status:**
```powershell
Get-Service MassDynamicsQC
```

### 20.4 Troubleshooting

| Symptom | Check |
|---------|-------|
| No files processing | `mdqc doctor`, check watch paths |
| Extraction failures | Check Skyline install, vendor readers |
| Upload failures | Check cert, network, `mdqc doctor` |
| High CPU | Check Skyline process priority config |
| Disk filling | Check spool directory, max_pending_mb |

---

## 21. Future Considerations

### 21.1 Deferred to v1.1+

| Feature | Notes |
|---------|-------|
| Ion mobility QC (timsTOF 4D) | Requires IM-aware metrics |
| Predictive maintenance models | ML on longitudinal data |
| Fleet benchmarking (opt-in) | Cross-site comparisons |
| Auto-update | Enterprise IT sensitivity |
| Linux agent | Different deployment model |

### 21.2 Template Evolution

When Skyline templates change:
- Old data remains segmented under old template hash
- New baseline (SSC0) required
- Dashboard shows template version transitions

### 21.3 Multi-Site Deployment

For organizations with multiple sites:
- Each site gets own agent instance(s)
- Cloud aggregates by organization
- Site-level and org-level dashboards

---

## Appendix A: Glossary

| Term | Definition |
|------|------------|
| SSC0 | System Suitability Control - baseline reference |
| QC_A | 500ng lysate full workflow control |
| QC_B | 50ng digest LCMS + loading control |
| Baseline | Reference metrics from SSC0 run |
| Template | Skyline `.sky` document with target definitions |
| Spool | Local disk queue for pending uploads |
| mTLS | Mutual TLS - both client and server present certificates |

---

## Appendix B: Configuration Reference

See [Section 13.2](#132-config-schema) for full config file reference.

---

## Appendix C: Event Log IDs

| ID | Level | Description |
|----|-------|-------------|
| 1000 | Info | Service started |
| 1001 | Info | Service stopped |
| 1010 | Info | Run detected |
| 1011 | Info | Run processed |
| 1012 | Info | Run uploaded |
| 1100 | Warning | Upload retry |
| 1101 | Warning | Certificate expiring |
| 1102 | Warning | QC deviation detected |
| 1200 | Error | Extraction failed |
| 1201 | Error | Upload failed (exhausted retries) |
| 1202 | Error | Configuration error |
| 1300 | Critical | Skyline not found |
| 1301 | Critical | Template missing |
| 1302 | Critical | Certificate expired |
| 1303 | Critical | Disk full |

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-01-27 | MD | Initial draft |
| 2.0 | 2026-01-27 | MD | Incorporated engineering review; hardened operational requirements |
