@echo off
title AgentXFlow Launcher
cd /d "%~dp0"
echo ===================================================
echo   Starting AgentXFlow by Viducia...
echo ===================================================
echo.

call npm.cmd run tauri dev
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo ===================================================
    echo [ERROR] AgentXFlow exited with error code %ERRORLEVEL%.
    echo ===================================================
)
pause
