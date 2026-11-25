@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0Connect-AgentHub.ps1" %*
if errorlevel 1 (
  echo.
  echo Connection failed. Review the message above and try again.
  pause
)
