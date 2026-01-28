#!/usr/bin/env python3
"""
MD QC Agent Test Harness - Agent Monitor

Monitors the MD QC Agent's behavior by watching:
- Log files for processing events
- Failed files JSON for timeout/error tracking
- Watch folder for file state changes

Usage:
    python monitor_agent.py
    python monitor_agent.py --log-dir "C:\\Users\\...\\AppData\\Local\\MassDynamics\\QC\\logs"
    python monitor_agent.py --follow  # Continuous monitoring
"""

import argparse
import json
import os
import sys
import time
from pathlib import Path
from datetime import datetime


def get_default_paths():
    """Get default paths for MD QC Agent data."""
    if sys.platform == 'win32':
        local_app_data = os.environ.get('LOCALAPPDATA', '')
        base_dir = Path(local_app_data) / 'MassDynamics' / 'QC'
    else:
        home = Path.home()
        base_dir = home / '.local' / 'share' / 'mdqc'

    return {
        'data_dir': base_dir,
        'log_dir': base_dir / 'logs',
        'config_file': base_dir / 'config.toml',
        'failed_files': base_dir / 'failed_files.json',
    }


def check_agent_status():
    """Check if the MD QC Agent is running."""
    import subprocess

    try:
        result = subprocess.run(
            ['tasklist', '/FI', 'IMAGENAME eq mdqc.exe'],
            capture_output=True,
            text=True
        )
        return 'mdqc.exe' in result.stdout
    except Exception:
        return None  # Can't determine


def read_failed_files(failed_files_path: Path) -> list:
    """Read the failed files JSON."""
    if not failed_files_path.exists():
        return []

    try:
        with open(failed_files_path, 'r') as f:
            data = json.load(f)
            return list(data.get('files', {}).values())
    except (json.JSONDecodeError, KeyError):
        return []


def read_recent_logs(log_dir: Path, lines: int = 50) -> list:
    """Read recent log entries from the latest log file."""
    if not log_dir.exists():
        return []

    # Find the most recent log file
    log_files = list(log_dir.glob('mdqc*.log'))
    if not log_files:
        return []

    latest_log = max(log_files, key=lambda p: p.stat().st_mtime)

    try:
        with open(latest_log, 'r', encoding='utf-8', errors='ignore') as f:
            all_lines = f.readlines()
            return all_lines[-lines:]
    except Exception as e:
        return [f"Error reading log: {e}"]


def parse_log_line(line: str) -> dict:
    """Parse a JSON log line."""
    try:
        return json.loads(line.strip())
    except json.JSONDecodeError:
        return {'message': line.strip(), 'raw': True}


def format_log_entry(entry: dict) -> str:
    """Format a log entry for display."""
    if entry.get('raw'):
        return entry.get('message', '')

    timestamp = entry.get('timestamp', '')
    level = entry.get('level', 'INFO')
    message = entry.get('message', entry.get('fields', {}).get('message', ''))
    target = entry.get('target', '')

    # Color codes for terminal
    colors = {
        'ERROR': '\033[91m',  # Red
        'WARN': '\033[93m',   # Yellow
        'INFO': '\033[92m',   # Green
        'DEBUG': '\033[94m',  # Blue
        'TRACE': '\033[90m',  # Gray
    }
    reset = '\033[0m'
    color = colors.get(level, '')

    # Extract relevant fields
    path = entry.get('fields', {}).get('path', '')
    instrument = entry.get('fields', {}).get('instrument', '')

    parts = [f"{color}[{level}]{reset}"]
    if instrument:
        parts.append(f"[{instrument}]")
    parts.append(message)
    if path:
        parts.append(f"({path})")

    return ' '.join(parts)


def display_status(paths: dict, verbose: bool = False):
    """Display current agent status."""
    print("\n" + "=" * 60)
    print("MD QC Agent Status")
    print("=" * 60)

    # Check if agent is running
    is_running = check_agent_status()
    if is_running is True:
        print("Agent: \033[92mRUNNING\033[0m")
    elif is_running is False:
        print("Agent: \033[91mNOT RUNNING\033[0m")
    else:
        print("Agent: \033[93mUNKNOWN\033[0m")

    # Check config
    config_path = paths['config_file']
    if config_path.exists():
        print(f"Config: {config_path}")
    else:
        print(f"Config: \033[91mNOT FOUND\033[0m ({config_path})")

    # Check failed files
    failed_files = read_failed_files(paths['failed_files'])
    if failed_files:
        print(f"Failed files: \033[93m{len(failed_files)}\033[0m")
        for ff in failed_files[:5]:  # Show first 5
            print(f"  - {ff.get('path', 'unknown')}")
            print(f"    Reason: {ff.get('reason', 'unknown')}")
    else:
        print("Failed files: \033[92m0\033[0m")

    # Show recent log entries
    print("\n" + "-" * 60)
    print("Recent Log Entries:")
    print("-" * 60)

    log_entries = read_recent_logs(paths['log_dir'], lines=20)
    if log_entries:
        for line in log_entries:
            entry = parse_log_line(line)
            print(format_log_entry(entry))
    else:
        print("  (No log entries found)")


def follow_logs(paths: dict):
    """Continuously follow log output (like tail -f)."""
    print("Following agent logs... (Ctrl+C to stop)")
    print("-" * 60)

    log_dir = paths['log_dir']
    last_log_file = None
    last_position = 0

    try:
        while True:
            # Find the most recent log file
            log_files = list(log_dir.glob('mdqc*.log')) if log_dir.exists() else []

            if log_files:
                current_log = max(log_files, key=lambda p: p.stat().st_mtime)

                # If log file changed, reset position
                if current_log != last_log_file:
                    last_log_file = current_log
                    last_position = 0
                    print(f"\n[Switched to log: {current_log.name}]")

                # Read new content
                try:
                    with open(current_log, 'r', encoding='utf-8', errors='ignore') as f:
                        f.seek(last_position)
                        new_content = f.read()
                        last_position = f.tell()

                        if new_content:
                            for line in new_content.strip().split('\n'):
                                if line:
                                    entry = parse_log_line(line)
                                    print(format_log_entry(entry))
                except Exception as e:
                    print(f"[Error reading log: {e}]")

            # Check for failed files updates
            failed_files = read_failed_files(paths['failed_files'])
            if failed_files:
                # Just show a notification, don't spam
                pass

            time.sleep(0.5)

    except KeyboardInterrupt:
        print("\n\n[Stopped following logs]")


def run_health_check(paths: dict) -> bool:
    """Run a health check and return True if everything looks good."""
    print("\nRunning health check...")

    issues = []

    # Check agent running
    if not check_agent_status():
        issues.append("Agent is not running")

    # Check config exists
    if not paths['config_file'].exists():
        issues.append(f"Config file not found: {paths['config_file']}")

    # Check log directory
    if not paths['log_dir'].exists():
        issues.append(f"Log directory not found: {paths['log_dir']}")

    # Check for recent log activity
    log_files = list(paths['log_dir'].glob('mdqc*.log')) if paths['log_dir'].exists() else []
    if log_files:
        latest_log = max(log_files, key=lambda p: p.stat().st_mtime)
        age_seconds = time.time() - latest_log.stat().st_mtime
        if age_seconds > 300:  # 5 minutes
            issues.append(f"No recent log activity (last update: {age_seconds/60:.1f} min ago)")

    # Check for failed files
    failed_files = read_failed_files(paths['failed_files'])
    if failed_files:
        issues.append(f"{len(failed_files)} files in failed state")

    if issues:
        print("\033[93mIssues found:\033[0m")
        for issue in issues:
            print(f"  - {issue}")
        return False
    else:
        print("\033[92mAll checks passed!\033[0m")
        return True


def main():
    parser = argparse.ArgumentParser(
        description="Monitor MD QC Agent status and logs",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    parser.add_argument(
        '--log-dir',
        type=Path,
        default=None,
        help='Path to log directory (default: auto-detect)'
    )

    parser.add_argument(
        '--data-dir',
        type=Path,
        default=None,
        help='Path to data directory (default: auto-detect)'
    )

    parser.add_argument(
        '--follow', '-f',
        action='store_true',
        help='Continuously follow log output'
    )

    parser.add_argument(
        '--health-check',
        action='store_true',
        help='Run health check and exit'
    )

    parser.add_argument(
        '--verbose', '-v',
        action='store_true',
        help='Show verbose output'
    )

    args = parser.parse_args()

    # Get paths
    paths = get_default_paths()
    if args.log_dir:
        paths['log_dir'] = args.log_dir
    if args.data_dir:
        paths['data_dir'] = args.data_dir
        paths['log_dir'] = args.data_dir / 'logs'
        paths['config_file'] = args.data_dir / 'config.toml'
        paths['failed_files'] = args.data_dir / 'failed_files.json'

    if args.health_check:
        success = run_health_check(paths)
        return 0 if success else 1

    if args.follow:
        display_status(paths, args.verbose)
        follow_logs(paths)
    else:
        display_status(paths, args.verbose)

    return 0


if __name__ == '__main__':
    sys.exit(main())
