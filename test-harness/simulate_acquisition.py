#!/usr/bin/env python3
"""
MD QC Agent Test Harness - Acquisition Simulator

Simulates mass spectrometry file acquisition to test the watcher functionality.
Creates realistic file structures for different vendors and simulates the
gradual writing process that occurs during real acquisitions.

Supported vendors:
- Thermo (.raw files)
- Bruker (.d folders with analysis.tdf)
- Agilent (.d folders with AcqData/MSScan.bin)
- Waters (.raw folders with _FUNC001.DAT)

Usage:
    python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo
    python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor bruker --duration 30
    python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo --test-timeout
"""

import argparse
import os
import sys
import time
import random
import string
import shutil
from pathlib import Path
from datetime import datetime


def generate_random_data(size_bytes: int) -> bytes:
    """Generate random binary data to simulate file content."""
    return os.urandom(size_bytes)


def generate_run_name(prefix: str = "QC") -> str:
    """Generate a realistic run name with timestamp."""
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    random_suffix = ''.join(random.choices(string.ascii_uppercase + string.digits, k=4))
    return f"{prefix}_{timestamp}_{random_suffix}"


class AcquisitionSimulator:
    """Base class for simulating file acquisition."""

    def __init__(self, watch_folder: Path, run_name: str):
        self.watch_folder = watch_folder
        self.run_name = run_name
        self.file_path = None

    def simulate(self, duration_seconds: int = 10, final_size_mb: float = 5.0):
        """Simulate a complete acquisition cycle."""
        raise NotImplementedError

    def cleanup(self):
        """Remove the simulated file/folder."""
        if self.file_path and self.file_path.exists():
            if self.file_path.is_dir():
                shutil.rmtree(self.file_path)
            else:
                self.file_path.unlink()
            print(f"  Cleaned up: {self.file_path}")


class ThermoSimulator(AcquisitionSimulator):
    """Simulate Thermo .raw file acquisition."""

    def simulate(self, duration_seconds: int = 10, final_size_mb: float = 5.0):
        self.file_path = self.watch_folder / f"{self.run_name}.raw"
        final_size = int(final_size_mb * 1024 * 1024)

        print(f"\n[Thermo] Starting acquisition: {self.file_path.name}")
        print(f"  Duration: {duration_seconds}s, Final size: {final_size_mb:.1f} MB")

        # Simulate gradual file growth
        chunk_count = max(10, duration_seconds)
        chunk_size = final_size // chunk_count

        with open(self.file_path, 'wb') as f:
            for i in range(chunk_count):
                # Write a chunk
                data = generate_random_data(chunk_size)
                f.write(data)
                f.flush()
                os.fsync(f.fileno())

                progress = (i + 1) / chunk_count * 100
                current_size = (i + 1) * chunk_size / (1024 * 1024)
                print(f"  Writing... {progress:.0f}% ({current_size:.2f} MB)", end='\r')

                time.sleep(duration_seconds / chunk_count)

        print(f"\n  Acquisition complete: {self.file_path.name}")
        print(f"  Final size: {self.file_path.stat().st_size / (1024*1024):.2f} MB")
        return self.file_path


class BrukerSimulator(AcquisitionSimulator):
    """Simulate Bruker .d folder acquisition with analysis.tdf."""

    def simulate(self, duration_seconds: int = 10, final_size_mb: float = 5.0):
        self.file_path = self.watch_folder / f"{self.run_name}.d"
        self.file_path.mkdir(parents=True, exist_ok=True)

        analysis_tdf = self.file_path / "analysis.tdf"
        lock_file = self.file_path / "analysis.tdf-journal"
        final_size = int(final_size_mb * 1024 * 1024)

        print(f"\n[Bruker] Starting acquisition: {self.file_path.name}")
        print(f"  Duration: {duration_seconds}s, Final size: {final_size_mb:.1f} MB")

        # Create lock file (indicates acquisition in progress)
        lock_file.write_bytes(b"LOCK")
        print(f"  Lock file created: {lock_file.name}")

        # Simulate gradual file growth
        chunk_count = max(10, duration_seconds)
        chunk_size = final_size // chunk_count

        with open(analysis_tdf, 'wb') as f:
            for i in range(chunk_count):
                data = generate_random_data(chunk_size)
                f.write(data)
                f.flush()
                os.fsync(f.fileno())

                progress = (i + 1) / chunk_count * 100
                current_size = (i + 1) * chunk_size / (1024 * 1024)
                print(f"  Writing... {progress:.0f}% ({current_size:.2f} MB)", end='\r')

                time.sleep(duration_seconds / chunk_count)

        # Remove lock file (indicates acquisition complete)
        lock_file.unlink()
        print(f"\n  Lock file removed")
        print(f"  Acquisition complete: {self.file_path.name}")
        print(f"  Final size: {analysis_tdf.stat().st_size / (1024*1024):.2f} MB")
        return self.file_path


class AgilentSimulator(AcquisitionSimulator):
    """Simulate Agilent .d folder acquisition with AcqData/MSScan.bin."""

    def simulate(self, duration_seconds: int = 10, final_size_mb: float = 5.0):
        self.file_path = self.watch_folder / f"{self.run_name}.d"
        acq_data = self.file_path / "AcqData"
        acq_data.mkdir(parents=True, exist_ok=True)

        ms_scan = acq_data / "MSScan.bin"
        final_size = int(final_size_mb * 1024 * 1024)

        print(f"\n[Agilent] Starting acquisition: {self.file_path.name}")
        print(f"  Duration: {duration_seconds}s, Final size: {final_size_mb:.1f} MB")

        # Simulate gradual file growth
        chunk_count = max(10, duration_seconds)
        chunk_size = final_size // chunk_count

        with open(ms_scan, 'wb') as f:
            for i in range(chunk_count):
                data = generate_random_data(chunk_size)
                f.write(data)
                f.flush()
                os.fsync(f.fileno())

                progress = (i + 1) / chunk_count * 100
                current_size = (i + 1) * chunk_size / (1024 * 1024)
                print(f"  Writing... {progress:.0f}% ({current_size:.2f} MB)", end='\r')

                time.sleep(duration_seconds / chunk_count)

        print(f"\n  Acquisition complete: {self.file_path.name}")
        print(f"  Final size: {ms_scan.stat().st_size / (1024*1024):.2f} MB")
        return self.file_path


class WatersSimulator(AcquisitionSimulator):
    """Simulate Waters .raw folder acquisition."""

    def simulate(self, duration_seconds: int = 10, final_size_mb: float = 5.0):
        self.file_path = self.watch_folder / f"{self.run_name}.raw"
        self.file_path.mkdir(parents=True, exist_ok=True)

        func_file = self.file_path / "_FUNC001.DAT"
        extern_inf = self.file_path / "_extern.inf"
        lock_file = self.file_path / "_LOCK_"
        final_size = int(final_size_mb * 1024 * 1024)

        print(f"\n[Waters] Starting acquisition: {self.file_path.name}")
        print(f"  Duration: {duration_seconds}s, Final size: {final_size_mb:.1f} MB")

        # Create lock file
        lock_file.write_bytes(b"LOCK")
        print(f"  Lock file created: {lock_file.name}")

        # Simulate gradual file growth
        chunk_count = max(10, duration_seconds)
        chunk_size = final_size // chunk_count

        with open(func_file, 'wb') as f:
            for i in range(chunk_count):
                data = generate_random_data(chunk_size)
                f.write(data)
                f.flush()
                os.fsync(f.fileno())

                progress = (i + 1) / chunk_count * 100
                current_size = (i + 1) * chunk_size / (1024 * 1024)
                print(f"  Writing... {progress:.0f}% ({current_size:.2f} MB)", end='\r')

                time.sleep(duration_seconds / chunk_count)

        # Remove lock and create _extern.inf (indicates complete)
        lock_file.unlink()
        extern_inf.write_text("Acquisition Complete\n")

        print(f"\n  Lock file removed, _extern.inf created")
        print(f"  Acquisition complete: {self.file_path.name}")
        print(f"  Final size: {func_file.stat().st_size / (1024*1024):.2f} MB")
        return self.file_path


def get_simulator(vendor: str, watch_folder: Path, run_name: str) -> AcquisitionSimulator:
    """Factory function to get the appropriate simulator."""
    simulators = {
        'thermo': ThermoSimulator,
        'bruker': BrukerSimulator,
        'agilent': AgilentSimulator,
        'waters': WatersSimulator,
    }

    if vendor.lower() not in simulators:
        raise ValueError(f"Unknown vendor: {vendor}. Supported: {list(simulators.keys())}")

    return simulators[vendor.lower()](watch_folder, run_name)


def monitor_agent_response(watch_folder: Path, file_path: Path, timeout: int = 120):
    """Monitor the agent's response to the simulated file."""
    print(f"\n[Monitor] Watching for agent response...")
    print(f"  File: {file_path.name}")
    print(f"  Timeout: {timeout}s")
    print(f"  (The agent should detect the file and start processing)")

    start_time = time.time()
    last_mtime = file_path.stat().st_mtime if file_path.exists() else 0

    while time.time() - start_time < timeout:
        elapsed = time.time() - start_time

        # Check if file still exists (might be moved by agent)
        if not file_path.exists():
            print(f"\n  [OK] File was processed/moved after {elapsed:.1f}s")
            return True

        # Check if file was modified (agent might touch it)
        current_mtime = file_path.stat().st_mtime
        if current_mtime != last_mtime:
            print(f"\n  [INFO] File was touched at {elapsed:.1f}s")
            last_mtime = current_mtime

        # Show progress
        print(f"  Waiting... {elapsed:.0f}s / {timeout}s", end='\r')
        time.sleep(1)

    print(f"\n  [TIMEOUT] No agent response after {timeout}s")
    return False


def run_test_suite(watch_folder: Path, vendors: list, duration: int, cleanup: bool):
    """Run a complete test suite for specified vendors."""
    print("=" * 60)
    print("MD QC Agent Test Harness")
    print("=" * 60)
    print(f"Watch folder: {watch_folder}")
    print(f"Vendors to test: {', '.join(vendors)}")
    print(f"Acquisition duration: {duration}s")
    print(f"Cleanup after test: {cleanup}")
    print("=" * 60)

    # Ensure watch folder exists
    watch_folder.mkdir(parents=True, exist_ok=True)

    results = []
    simulators = []

    for vendor in vendors:
        run_name = generate_run_name(f"TEST_{vendor.upper()}")
        simulator = get_simulator(vendor, watch_folder, run_name)
        simulators.append(simulator)

        try:
            # Simulate acquisition
            file_path = simulator.simulate(duration_seconds=duration)

            # Monitor for agent response
            # Give it stability_window (60s default) + some buffer
            success = monitor_agent_response(watch_folder, file_path, timeout=90)

            results.append({
                'vendor': vendor,
                'file': file_path.name,
                'success': success,
            })

        except Exception as e:
            print(f"\n  [ERROR] {vendor}: {e}")
            results.append({
                'vendor': vendor,
                'file': 'N/A',
                'success': False,
                'error': str(e),
            })

    # Summary
    print("\n" + "=" * 60)
    print("Test Results Summary")
    print("=" * 60)

    for result in results:
        status = "PASS" if result['success'] else "FAIL"
        print(f"  [{status}] {result['vendor']}: {result['file']}")
        if 'error' in result:
            print(f"         Error: {result['error']}")

    # Cleanup
    if cleanup:
        print("\n[Cleanup] Removing test files...")
        for simulator in simulators:
            simulator.cleanup()
    else:
        print("\n[Note] Test files were NOT cleaned up (use --cleanup to remove)")

    # Return exit code
    all_passed = all(r['success'] for r in results)
    return 0 if all_passed else 1


def main():
    parser = argparse.ArgumentParser(
        description="MD QC Agent Test Harness - Simulate MS file acquisition",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Simulate a single Thermo acquisition
  python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo

  # Test multiple vendors
  python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo bruker agilent

  # Longer acquisition (30 seconds of writing)
  python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo --duration 30

  # Clean up files after test
  python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo --cleanup

  # Test timeout handling (acquisition never finishes)
  python simulate_acquisition.py --watch-folder "D:\\TestData" --vendor thermo --test-timeout
        """
    )

    parser.add_argument(
        '--watch-folder', '-w',
        type=Path,
        required=True,
        help='Path to the watch folder configured in the agent'
    )

    parser.add_argument(
        '--vendor', '-v',
        nargs='+',
        choices=['thermo', 'bruker', 'agilent', 'waters'],
        default=['thermo'],
        help='Vendor(s) to simulate (default: thermo)'
    )

    parser.add_argument(
        '--duration', '-d',
        type=int,
        default=10,
        help='Duration of simulated acquisition in seconds (default: 10)'
    )

    parser.add_argument(
        '--cleanup', '-c',
        action='store_true',
        help='Clean up test files after completion'
    )

    parser.add_argument(
        '--test-timeout',
        action='store_true',
        help='Test timeout handling (create file that never stops growing)'
    )

    parser.add_argument(
        '--run-name',
        type=str,
        default=None,
        help='Custom run name (default: auto-generated with timestamp)'
    )

    args = parser.parse_args()

    if args.test_timeout:
        # Special mode: simulate a file that never finishes
        print("=" * 60)
        print("TIMEOUT TEST MODE")
        print("=" * 60)
        print("This will create a file that keeps growing indefinitely.")
        print("The agent should eventually timeout and mark it as failed.")
        print("Press Ctrl+C to stop the test.")
        print("=" * 60)

        vendor = args.vendor[0]
        run_name = args.run_name or generate_run_name(f"TIMEOUT_{vendor.upper()}")
        simulator = get_simulator(vendor, args.watch_folder, run_name)

        try:
            # Simulate very long acquisition (will be interrupted by Ctrl+C)
            simulator.simulate(duration_seconds=3600, final_size_mb=1000)
        except KeyboardInterrupt:
            print("\n\n[Interrupted] Stopping acquisition simulation")
            if args.cleanup:
                simulator.cleanup()

        return 0

    # Normal test suite
    return run_test_suite(
        watch_folder=args.watch_folder,
        vendors=args.vendor,
        duration=args.duration,
        cleanup=args.cleanup,
    )


if __name__ == '__main__':
    sys.exit(main())
