@echo off
setlocal enabledelayedexpansion
title AgentXFlow Launcher
cd /d "%~dp0"

echo ===================================================
echo   Starting AgentXFlow by Viducia...
echo ===================================================
echo.

:: 1. Check Node.js
where node >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Node.js is NOT installed or not in PATH!
    echo Please install Node.js v20+ from: https://nodejs.org
    echo.
    pause
    exit /b 1
)

:: 2. Check Cargo / Rust
where cargo >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Rust / Cargo is NOT installed or not in PATH!
    echo AgentXFlow requires Rust for its high-performance coordinator backend.
    echo Please install Rust from: https://rustup.rs
    echo.
    pause
    exit /b 1
)

:: 3. Auto-install dependencies if first-time run / node_modules missing
if not exist "node_modules\" (
    echo [INFO] First-time setup detected: Installing dependencies via npm install...
    call npm.cmd install
    if !ERRORLEVEL! NEQ 0 (
        echo [ERROR] npm install failed. Please check your network connection.
        pause
        exit /b 1
    )
    echo [OK] Dependencies installed successfully.
    echo.
)

:: 4. Launch AgentXFlow via Tauri CLI
echo Launching desktop application...
if exist "node_modules\.bin\tauri.cmd" (
    call "node_modules\.bin\tauri.cmd" dev
) else (
    call npx @tauri-apps/cli dev
)

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo ===================================================
    echo [ERROR] AgentXFlow exited with error code %ERRORLEVEL%.
    echo ===================================================
    echo.
    echo Troubleshooting:
    echo 1. Ensure Rust is up to date: rustup update
    echo 2. Run setup.bat to verify all system dependencies.
    echo.
)
pause
