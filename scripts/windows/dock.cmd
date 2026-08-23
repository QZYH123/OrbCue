@echo off
setlocal
set "EXE=%~dp0dock.exe"
if not exist "%EXE%" (
  where dock.exe >nul 2>&1 && for /f "delims=" %%I in ('where dock.exe') do set "EXE=%%I"
)
set "BACKEND=%AGENT_ACTIVITY_DOCK_BACKEND%"
if "%BACKEND%"=="" set "BACKEND=wsl"
if /I "%BACKEND%"=="local" (
  if not exist "%EXE%" (
    echo dock.exe was not found. Install the Windows presenter or set AGENT_ACTIVITY_DOCK_BACKEND=wsl. >&2
    exit /b 1
  )
  "%EXE%" %*
  exit /b %ERRORLEVEL%
)
where wsl.exe >nul 2>&1
if errorlevel 1 (
  echo wsl.exe was not found. Install WSL or run dock from a WSL terminal. >&2
  exit /b 1
)
wsl.exe -e sh -c "exec \"$HOME/.local/bin/dock\" \"$@\"" sh %*
exit /b %ERRORLEVEL%
