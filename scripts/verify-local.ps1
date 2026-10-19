# Synchro stability verification. Run on Windows with Rust installed.
param([switch]$Android)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location (Join-Path $repoRoot 'desktop')
try {
    Write-Host 'Running Rust unit tests...' -ForegroundColor Cyan
    cargo test
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    Write-Host 'Checking Rust types...' -ForegroundColor Cyan
    cargo check
    if ($LASTEXITCODE -ne 0) { throw 'cargo check failed' }
    Write-Host 'Running Rust clippy...' -ForegroundColor Cyan
    cargo clippy --all-targets
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
} finally {
    Pop-Location
}
if ($Android) {
    Write-Host 'Building Android debug APK using compatible JDK...' -ForegroundColor Cyan
    & (Join-Path $PSScriptRoot 'build-android.ps1')
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw 'Android build failed' }
}
Write-Host 'All requested local verification steps passed.' -ForegroundColor Green
