# Forwards Windows PowerShell `dock` calls into the WSL daemon.
# The Win+WSL minimum path keeps one dockd in WSL; Windows terminals use this trampoline.
$ErrorActionPreference = 'Stop'
$wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
if (-not $wsl) {
    Write-Error 'wsl.exe was not found. Install WSL or run dock from a WSL terminal.'
}
& $wsl.Source -e sh -c 'exec "$HOME/.local/bin/dock" "$@"' sh @args
exit $LASTEXITCODE
