#!/usr/bin/env python3
"""
MD QC Agent Test Harness - Copy Real Files as QC

Copies existing MS data files with QC-appropriate naming to test the full
processing pipeline (including Skyline extraction).

This is more realistic than simulate_acquisition.py because it uses real
data files that Skyline can actually process.

Usage:
    # Copy a Thermo .raw file as QCA
    python copy_as_qc.py --source "D:\\Data\\real_sample.raw" --dest "D:\\TestData" --control-type QCA

    # Copy a Bruker .d folder as SSC0
    python copy_as_qc.py --source "D:\\Data\\real_sample.d" --dest "D:\\TestData" --control-type SSC0 --well A1

    # Simulate slow copy (like acquisition)
    python copy_as_qc.py --source "D:\\Data\\real.raw" --dest "D:\\TestData" --control-type QCA --simulate-write 30
"""

import argparse
import os
import sys
import shutil
import time
from pathlib import Path
from datetime import datetime


def generate_qc_filename(source: Path, control_type: str, well: str = None, prefix: str = "TEST") -> str:
    """Generate a QC-appropriate filename.

    Args:
        source: Original file path (to get extension)
        control_type: SSC0, QCA, QCB, BLANK
        well: Well position (e.g., A1)
        prefix: Instrument prefix

    Returns:
        Filename like: TEST_QCA_A1_20260129_143052.raw
    """
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")

    # Get extension (handle .d directories)
    if source.suffix.lower() in ['.raw', '.d']:
        ext = source.suffix
    elif source.is_dir() and source.name.endswith('.d'):
        ext = '.d'
    else:
        ext = source.suffix or '.raw'

    parts = [prefix]
    if control_type and control_type.upper() != 'SAMPLE':
        parts.append(control_type.upper())
    if well:
        parts.append(well.upper())
    parts.append(timestamp)

    return "_".join(parts) + ext


def copy_file_slow(source: Path, dest: Path, duration_seconds: int):
    """Copy a file slowly to simulate acquisition.

    For single files, copies in chunks with delays.
    For directories (.d folders), copies files one at a time with delays.
    """
    if source.is_file():
        # Single file - copy in chunks
        file_size = source.stat().st_size
        chunk_size = max(1024 * 1024, file_size // 10)  # At least 1MB chunks
        chunks = max(10, duration_seconds)

        print(f"  Copying {file_size / (1024*1024):.1f} MB in {chunks} chunks...")

        with open(source, 'rb') as src, open(dest, 'wb') as dst:
            bytes_written = 0
            chunk_num = 0

            while True:
                chunk = src.read(chunk_size)
                if not chunk:
                    break

                dst.write(chunk)
                dst.flush()
                os.fsync(dst.fileno())

                bytes_written += len(chunk)
                chunk_num += 1
                progress = bytes_written / file_size * 100
                print(f"  Copying... {progress:.0f}% ({bytes_written/(1024*1024):.1f} MB)", end='\r')

                time.sleep(duration_seconds / chunks)

        print(f"\n  Copy complete: {dest.name}")

    else:
        # Directory - copy files one at a time
        all_files = list(source.rglob('*'))
        files = [f for f in all_files if f.is_file()]
        total_size = sum(f.stat().st_size for f in files)

        print(f"  Copying directory with {len(files)} files ({total_size/(1024*1024):.1f} MB)...")

        # Create destination directory
        dest.mkdir(parents=True, exist_ok=True)

        delay_per_file = duration_seconds / max(len(files), 1)
        bytes_copied = 0

        for i, src_file in enumerate(files):
            # Compute relative path
            rel_path = src_file.relative_to(source)
            dst_file = dest / rel_path

            # Create parent directories
            dst_file.parent.mkdir(parents=True, exist_ok=True)

            # Copy file
            shutil.copy2(src_file, dst_file)
            bytes_copied += src_file.stat().st_size

            progress = (i + 1) / len(files) * 100
            print(f"  Copying... {progress:.0f}% ({i+1}/{len(files)} files)", end='\r')

            time.sleep(delay_per_file)

        print(f"\n  Copy complete: {dest.name}")


def copy_file_fast(source: Path, dest: Path):
    """Copy a file/directory immediately."""
    if source.is_file():
        shutil.copy2(source, dest)
    else:
        shutil.copytree(source, dest)
    print(f"  Copied: {dest.name}")


def main():
    parser = argparse.ArgumentParser(
        description="Copy real MS files with QC naming for testing",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Copy a Thermo .raw file as QCA control
  python copy_as_qc.py -s "D:\\Data\\sample.raw" -d "D:\\TestData" -t QCA

  # Copy a Bruker .d folder as SSC0 with well position
  python copy_as_qc.py -s "D:\\Data\\sample.d" -d "D:\\TestData" -t SSC0 -W A1

  # Simulate slow acquisition (30 seconds)
  python copy_as_qc.py -s "D:\\Data\\sample.raw" -d "D:\\TestData" -t QCA --simulate-write 30

Control Types:
  SSC0  - System Suitability Control
  QCA   - QC Type A (also: QC_A, QC-A)
  QCB   - QC Type B (also: QC_B, QC-B)
  BLANK - Blank run
        """
    )

    parser.add_argument(
        '--source', '-s',
        type=Path,
        required=True,
        help='Source file or .d folder to copy'
    )

    parser.add_argument(
        '--dest', '-d',
        type=Path,
        required=True,
        help='Destination folder (watch folder)'
    )

    parser.add_argument(
        '--control-type', '-t',
        type=str,
        choices=['SSC0', 'QCA', 'QCB', 'BLANK', 'ssc0', 'qca', 'qcb', 'blank'],
        required=True,
        help='QC control type for the filename'
    )

    parser.add_argument(
        '--well', '-W',
        type=str,
        default=None,
        help='Well position (e.g., A1, A3)'
    )

    parser.add_argument(
        '--prefix', '-p',
        type=str,
        default='TEST',
        help='Instrument prefix (default: TEST)'
    )

    parser.add_argument(
        '--simulate-write',
        type=int,
        default=0,
        metavar='SECONDS',
        help='Simulate slow write over N seconds (0 = instant copy)'
    )

    parser.add_argument(
        '--output-name', '-o',
        type=str,
        default=None,
        help='Custom output filename (without extension)'
    )

    args = parser.parse_args()

    # Validate source
    if not args.source.exists():
        print(f"ERROR: Source does not exist: {args.source}")
        return 1

    # Ensure destination folder exists
    args.dest.mkdir(parents=True, exist_ok=True)

    # Generate output filename
    if args.output_name:
        ext = args.source.suffix if args.source.is_file() else '.d'
        output_name = args.output_name + ext
    else:
        output_name = generate_qc_filename(
            args.source,
            args.control_type,
            args.well,
            args.prefix
        )

    dest_path = args.dest / output_name

    # Check if destination already exists
    if dest_path.exists():
        print(f"WARNING: Destination already exists: {dest_path}")
        response = input("Overwrite? [y/N] ")
        if response.lower() != 'y':
            print("Cancelled.")
            return 0
        if dest_path.is_dir():
            shutil.rmtree(dest_path)
        else:
            dest_path.unlink()

    print("=" * 60)
    print("Copy Real File as QC")
    print("=" * 60)
    print(f"Source: {args.source}")
    print(f"Destination: {dest_path}")
    print(f"Control type: {args.control_type.upper()}")
    if args.well:
        print(f"Well: {args.well.upper()}")
    if args.simulate_write > 0:
        print(f"Simulated write time: {args.simulate_write}s")
    print("=" * 60)

    # Copy the file
    try:
        if args.simulate_write > 0:
            copy_file_slow(args.source, dest_path, args.simulate_write)
        else:
            copy_file_fast(args.source, dest_path)

        print("\nFile ready for processing by MD QC Agent.")
        print(f"Path: {dest_path}")

        # Show expected classification
        print(f"\nExpected classification: {args.control_type.upper()}")
        if args.well:
            print(f"Well position: {args.well.upper()}")

    except Exception as e:
        print(f"\nERROR: Copy failed: {e}")
        return 1

    return 0


if __name__ == '__main__':
    sys.exit(main())
