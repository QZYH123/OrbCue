# Prefer a same-directory or PATH dock.exe. Forward to WSL only while BACKEND is wsl.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $here 'dock.exe'
if (-not (Test-Path $exe)) {
    $found = Get-Command dock.exe -ErrorAction SilentlyContinue
    if ($found) {
        $exe = $found.Source
    }
}

$backend = $env:AGENT_ACTIVITY_DOCK_BACKEND
if ([string]::IsNullOrWhiteSpace($backend)) {
    $backend = 'wsl'
}

if ($backend -eq 'local') {
    if (-not (Test-Path $exe)) {
        Write-Error 'dock.exe was not found. Install the Windows presenter or set AGENT_ACTIVITY_DOCK_BACKEND=wsl.'
    }
    & $exe @args
    exit $LASTEXITCODE
}

$wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
if (-not $wsl) {
    Write-Error 'wsl.exe was not found. Install WSL or run dock from a WSL terminal.'
}
& $wsl.Source -e sh -c 'exec "$HOME/.local/bin/dock" "$@"' sh @args
exit $LASTEXITCODE
