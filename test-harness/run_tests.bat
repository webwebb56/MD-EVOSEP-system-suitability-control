@echo off
REM MD QC Agent Test Harness - Quick Test Runner
REM
REM Usage: run_tests.bat <watch_folder>
REM Example: run_tests.bat D:\TestData

setlocal enabledelayedexpansion

if "%~1"=="" (
    echo Usage: run_tests.bat ^<watch_folder^>
    echo Example: run_tests.bat D:\TestData
    exit /b 1
)

set WATCH_FOLDER=%~1

echo ============================================================
echo MD QC Agent Test Harness
echo ============================================================
echo Watch folder: %WATCH_FOLDER%
echo.

REM Check Python is available
python --version >nul 2>&1
if errorlevel 1 (
    echo ERROR: Python is not installed or not in PATH
    exit /b 1
)

REM Check watch folder exists
if not exist "%WATCH_FOLDER%" (
    echo Creating watch folder: %WATCH_FOLDER%
    mkdir "%WATCH_FOLDER%"
)

echo.
echo [1/3] Running health check...
echo ------------------------------------------------------------
python "%~dp0monitor_agent.py" --health-check
if errorlevel 1 (
    echo.
    echo WARNING: Health check found issues. Continue anyway? [Y/N]
    set /p CONTINUE=
    if /i not "!CONTINUE!"=="Y" exit /b 1
)

echo.
echo [2/3] Running Thermo acquisition simulation...
echo ------------------------------------------------------------
python "%~dp0simulate_acquisition.py" -w "%WATCH_FOLDER%" -v thermo -d 10

echo.
echo [3/3] Waiting for agent to process (90 seconds)...
echo ------------------------------------------------------------
echo Monitoring agent response. Press Ctrl+C to skip.
timeout /t 90 /nobreak >nul 2>&1

echo.
echo ============================================================
echo Test Complete
echo ============================================================
echo.
echo Check results:
echo   - Run: mdqc failed list
echo   - Run: python monitor_agent.py
echo.

pause
