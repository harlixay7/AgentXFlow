@echo off
setlocal enabledelayedexpansion
title AgentXFlow - Dependency & Environment Setup (Viducia)
cd /d "%~dp0"

echo ===================================================
echo   AgentXFlow Dependency & Environment Setup
echo   Organization: Viducia • Developer: harlixay7
echo ===================================================
echo.

set "MISSING_TOOLS=0"

:: 1. Check Node.js
echo [1/4] Checking Node.js...
where node >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [FAIL] Node.js is NOT installed or not in PATH!
    echo        Please install Node.js v20+ from: https://nodejs.org
    set "MISSING_TOOLS=1"
) else (
    for /f "tokens=*" %%i in ('node -v') do set NODE_VER=%%i
    echo [OK] Node.js is installed: !NODE_VER!
)

:: 2. Check Git CLI
echo.
echo [2/4] Checking Git CLI...
where git >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [FAIL] Git is NOT installed or not in PATH!
    echo        Please install Git from: https://git-scm.com
    set "MISSING_TOOLS=1"
) else (
    for /f "tokens=*" %%i in ('git --version') do set GIT_VER=%%i
    echo [OK] Git is installed: !GIT_VER!
)

:: 3. Check Rust / Cargo
echo.
echo [3/4] Checking Rust and Cargo...
where cargo >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [FAIL] Rust/Cargo is NOT installed or not in PATH!
    echo        Please install Rust from: https://rustup.rs
    set "MISSING_TOOLS=1"
) else (
    for /f "tokens=*" %%i in ('cargo --version') do set CARGO_VER=%%i
    echo [OK] Cargo is installed: !CARGO_VER!
)

if %MISSING_TOOLS% NEQ 0 (
    echo.
    echo ===================================================
    echo [ERROR] Required system tools are missing.
    echo Please install the missing prerequisites listed above
    echo and re-run this setup script.
    echo ===================================================
    echo.
    pause
    exit /b 1
)

:: 4. Install Node Dependencies if missing or outdated
echo.
echo [4/4] Verifying and installing project dependencies...
if not exist "node_modules\" (
    echo Node modules not found. Running npm install...
    call npm install
    if %ERRORLEVEL% NEQ 0 (
        echo [ERROR] npm install failed.
        pause
        exit /b 1
    )
) else (
    echo Node modules already installed. Updating dependencies...
    call npm install --prefer-offline --no-audit
)

echo.
echo Building frontend assets and verifying TypeScript types...
call npm run build
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Frontend build failed.
    pause
    exit /b 1
)

echo.
echo Checking Rust backend compilation...
call cargo check --manifest-path src-tauri/Cargo.toml
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Backend compilation check failed.
    pause
    exit /b 1
)

echo.
echo ===================================================
echo   All dependencies are verified and up to date!
echo   You can now launch AgentXFlow with: run.bat
echo ===================================================
echo.
pause
