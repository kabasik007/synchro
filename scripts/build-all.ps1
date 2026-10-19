# Synchro: build/test Windows, Android and Windows relay, then package results.
# Run from any folder: & 'M:\projects\synchro\scripts\build-all.ps1'
param(
    [switch]$SkipAndroid,
    [switch]$SkipDesktop,
    [switch]$SkipServer,
    [switch]$NoPull
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$desktopManifest = Join-Path $root 'desktop\Cargo.toml'
$serverManifest = Join-Path $root 'server\Cargo.toml'
$androidScript = Join-Path $root 'scripts\build-android.ps1'
$webJs = Join-Path $root 'web\app.js'

function Require-LastExit([string]$stage) {
    if ($LASTEXITCODE -ne 0) {
        throw "$stage failed with exit code $LASTEXITCODE. No new package was published."
    }
}
function Require-File([string]$path) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Expected build artifact missing: $path"
    }
}
function Step([string]$message) {
    Write-Host ''
    Write-Host "== $message ==" -ForegroundColor Cyan
}

if ($env:OS -ne 'Windows_NT') {
    throw 'Use Windows for EXE packaging; the aaPanel Linux server must be built on Linux.'
}
if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'Git not found.' }
if ((-not $SkipDesktop -or -not $SkipServer) -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Rust cargo not found.'
}

if (-not $NoPull) {
    Step 'Updating main'
    & git -C $root pull --ff-only
    Require-LastExit 'git pull'
}
$commit = & git -C $root rev-parse --short HEAD
Require-LastExit 'git rev-parse'
Write-Host "Source revision: $commit" -ForegroundColor DarkCyan

if (-not $SkipDesktop) {
    Step 'Windows desktop: tests, check, clippy, release'
    & cargo test --manifest-path $desktopManifest
    Require-LastExit 'Windows tests'
    & cargo check --manifest-path $desktopManifest
    Require-LastExit 'Windows cargo check'
    & cargo clippy --manifest-path $desktopManifest --all-targets
    Require-LastExit 'Windows clippy'
    & cargo build --manifest-path $desktopManifest --release
    Require-LastExit 'Windows release'
    $desktopExe = Join-Path $root 'desktop\target\release\synchro-desktop.exe'
    Require-File $desktopExe
}

if (-not $SkipAndroid) {
    Step 'Android: Gradle debug APK (JDK auto-discovery)'
    & $androidScript
    # The Android script throws if Gradle fails.
    $androidApk = Join-Path $root 'android\app\build\outputs\apk\debug\app-debug.apk'
    Require-File $androidApk
}

if (-not $SkipServer) {
    Step 'Rust Internet server: Windows test build'
    & cargo test --manifest-path $serverManifest
    Require-LastExit 'Server tests'
    & cargo build --manifest-path $serverManifest --release
    Require-LastExit 'Server release'
    $serverExe = Join-Path $root 'server\target\release\synchro-server.exe'
    Require-File $serverExe
}

if (Get-Command node -ErrorAction SilentlyContinue) {
    Step 'Browser JavaScript syntax'
    & node --check $webJs
    Require-LastExit 'Web JavaScript check'
} else {
    Write-Warning 'Node.js unavailable: skipping browser JavaScript syntax check.'
}

# Only package when ALL requested builds passed. Never reuse old artifacts.
Step 'Packaging completed builds'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$runId = [guid]::NewGuid().ToString('N').Substring(0,8)
$out = Join-Path $root ("dist\test-" + $stamp + "-" + $runId)
New-Item -ItemType Directory -Force -Path $out | Out-Null
if (-not $SkipDesktop) {
    Copy-Item -LiteralPath $desktopExe -Destination (Join-Path $out 'Synchro-Windows.exe')
}
if (-not $SkipAndroid) {
    Copy-Item -LiteralPath $androidApk -Destination (Join-Path $out 'Synchro-Android-debug.apk')
}
if (-not $SkipServer) {
    Copy-Item -LiteralPath $serverExe -Destination (Join-Path $out 'Synchro-Server-Windows.exe')
}
Compress-Archive -Path (Join-Path $root 'web\*') -DestinationPath (Join-Path $out 'Synchro-Web.zip') -Force

@(
    "Synchro source commit: $commit"
    "Built: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
    'TEST BUILD. Windows server .exe is not compatible with aaPanel/Linux.'
) | Set-Content -LiteralPath (Join-Path $out 'BUILD-INFO.txt') -Encoding UTF8

Write-Host ''
Write-Host 'SYNCHRO: ALL REQUESTED BUILDS PASSED' -ForegroundColor Green
Get-ChildItem -LiteralPath $out | Select-Object Name, Length
Write-Host "Output folder: $out" -ForegroundColor Green
& explorer.exe $out
