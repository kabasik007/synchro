$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..\desktop')
cargo test
cargo build --release
Write-Host 'Built: desktop\target\release\synchro-desktop.exe'
