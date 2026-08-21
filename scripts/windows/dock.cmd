@echo off
rem Forwards Windows cmd `dock` calls into the WSL daemon.
where wsl.exe >nul 2>&1
if errorlevel 1 (
  echo wsl.exe was not found. Install WSL or run dock from a WSL terminal. >&2
  exit /b 1
)
wsl.exe -e sh -c "exec \"$HOME/.local/bin/dock\" \"$@\"" sh %*
exit /b %ERRORLEVEL%
