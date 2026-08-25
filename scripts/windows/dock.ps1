# Prefer a same-directory, LocalAppData, or PATH dock.exe.
# Forward to WSL only when the backend is explicitly wsl.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $here 'dock.exe'
if (-not (Test-Path $exe)) {
    $installed = Join-Path $env:LOCALAPPDATA 'Agent Activity Dock\dock.exe'
    if (Test-Path $installed) {
        $exe = $installed
    } else {
        $found = Get-Command dock.exe -ErrorAction SilentlyContinue
        if ($found) {
            $exe = $found.Source
        }
    }
}

function Resolve-DockBackend {
    $fromEnv = $env:AGENT_ACTIVITY_DOCK_BACKEND
    if (-not [string]::IsNullOrWhiteSpace($fromEnv)) {
        return $fromEnv.Trim().ToLowerInvariant()
    }
    $file = Join-Path $env:LOCALAPPDATA 'Agent Activity Dock\backend'
    if (Test-Path $file) {
        $line = Get-Content -Path $file -TotalCount 1 -ErrorAction SilentlyContinue
        if (-not [string]::IsNullOrWhiteSpace($line)) {
            return $line.Trim().ToLowerInvariant()
        }
    }
    return 'local'
}

$backend = Resolve-DockBackend

if ($backend -eq 'wsl') {
    $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
    if (-not $wsl) {
        Write-Error 'wsl.exe was not found. Start Agent Activity Dock, or install WSL.'
    }
    & $wsl.Source -e sh -c 'exec "$HOME/.local/bin/dock" "$@"' sh @args
    exit $LASTEXITCODE
}

if (-not (Test-Path $exe)) {
    Write-Error 'dock.exe was not found. Start Agent Activity Dock, then open a new terminal.'
}
& $exe @args
exit $LASTEXITCODE
