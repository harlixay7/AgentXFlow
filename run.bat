@echo off
title Viducia - Cross-Agent Engineering Coordinator
cd /d "%~dp0"

echo ===================================================
echo   Starting Viducia Engineering Coordinator...
echo   Developer: harlixay7
echo ===================================================
echo.

npm run tauri dev

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Viducia exited with error code %ERRORLEVEL%.
    pause
)
