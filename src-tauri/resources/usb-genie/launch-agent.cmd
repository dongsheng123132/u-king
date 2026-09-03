@echo off
setlocal
set "SCRIPT_DIR=%~dp0"
set "GENIE_ROOT=%SCRIPT_DIR%U-King\AI-Genie"
set "PICOCLAW_HOME=%GENIE_ROOT%\data"
set "PICOCLAW_CONFIG=%PICOCLAW_HOME%\config.json"
set "PICOCLAW_BINARY=%GENIE_ROOT%\runtime\current\picoclaw.exe"
set "PICOCLAW_BUILTIN_SKILLS=%PICOCLAW_HOME%\workspace\skills"
set "PICOCLAW_LOG_FILE=%PICOCLAW_HOME%\logs\picoclaw.log"
set "TEMP=%PICOCLAW_HOME%\tmp"
set "TMP=%PICOCLAW_HOME%\tmp"
if not exist "%PICOCLAW_HOME%\workspace" mkdir "%PICOCLAW_HOME%\workspace"
if not exist "%PICOCLAW_HOME%\logs" mkdir "%PICOCLAW_HOME%\logs"
if not exist "%PICOCLAW_HOME%\tmp" mkdir "%PICOCLAW_HOME%\tmp"
if not exist "%PICOCLAW_BINARY%" (
  echo [ERROR] picoclaw.exe not found.
  pause
  exit /b 1
)
cd /d "%PICOCLAW_HOME%\workspace"
"%PICOCLAW_BINARY%" agent
set "EXIT_CODE=%ERRORLEVEL%"
if not "%EXIT_CODE%"=="0" pause
cd /d "%SystemDrive%\"
exit /b %EXIT_CODE%
