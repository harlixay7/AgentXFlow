@echo off
title AgentXFlow Launcher (Viducia)
cd /d "%~dp0"
echo ===================================================
echo   Starting AgentXFlow Engineering Coordinator...
echo   Organization: Viducia ^| Developer: harlixay7
echo ===================================================
echo.

call npm run tauri dev
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo ===================================================
    echo [ERROR] AgentXFlow exited with error code %ERRORLEVEL%.
    echo ===================================================
)
pause
