# Build the Windows desktop safely without changing the caller's current directory.
$ErrorActionPreference = 'Stop'
$desktop = (Resolve-Path (Join-Path $PSScriptRoot '..\desktop')).Path

Push-Location -LiteralPath $desktop
try {
    & cargo test
    if ($LASTEXITCODE -ne 0) { throw "Windows desktop tests failed (exit $LASTEXITCODE)" }

    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "Windows desktop release build failed (exit $LASTEXITCODE)" }

    $exe = Join-Path $desktop 'target\release\synchro-desktop.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
        throw "Windows desktop build reported success but EXE is missing: $exe"
    }
    Write-Host "Built Windows EXE: $exe" -ForegroundColor Green
} finally {
    Pop-Location
}
