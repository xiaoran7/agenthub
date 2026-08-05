@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0Connect-AgentPulse.ps1" %*
if errorlevel 1 (
  echo.
  echo 接入失败，请根据上面的提示处理后重试。
  pause
)
