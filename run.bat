@echo off
title AgentXFlow Launcher (Viducia)
cd /d "%~dp0"
echo ===================================================
echo   Starting AgentXFlow Engineering Coordinator...
echo   Organization: Viducia • Developer: harlixay7
echo ===================================================
echo.
npm run tauri dev
pause
