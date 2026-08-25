@echo off
setlocal
set "EXE=%~dp0dock.exe"
if not exist "%EXE%" (
  if exist "%LOCALAPPDATA%\Agent Activity Dock\dock.exe" set "EXE=%LOCALAPPDATA%\Agent Activity Dock\dock.exe"
)
if not exist "%EXE%" (
  where dock.exe >nul 2>&1 && for /f "delims=" %%I in ('where dock.exe') do set "EXE=%%I"
)

set "BACKEND=%AGENT_ACTIVITY_DOCK_BACKEND%"
if "%BACKEND%"=="" (
  if exist "%LOCALAPPDATA%\Agent Activity Dock\backend" (
    set /p BACKEND=<"%LOCALAPPDATA%\Agent Activity Dock\backend"
  )
)
if "%BACKEND%"=="" set "BACKEND=local"

if /I "%BACKEND%"=="wsl" goto :wsl

if not exist "%EXE%" (
  echo dock.exe was not found. Start Agent Activity Dock, then open a new terminal. >&2
  exit /b 1
)
"%EXE%" %*
exit /b %ERRORLEVEL%

:wsl
where wsl.exe >nul 2>&1
if errorlevel 1 (
  echo wsl.exe was not found. Start Agent Activity Dock, or install WSL. >&2
  exit /b 1
)
wsl.exe -e sh -c "exec \"$HOME/.local/bin/dock\" \"$@\"" sh %*
exit /b %ERRORLEVEL%
