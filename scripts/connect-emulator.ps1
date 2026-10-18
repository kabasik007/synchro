# Connect an Android Studio emulator's Synchro receiver to localhost:8765.
# Supports a specific device via -DeviceId emulator-5554. Does not install anything.
param(
    [string]$DeviceId = ''
)

$ErrorActionPreference = 'Stop'
$adb = Get-Command adb -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
if (-not $adb -and $env:LOCALAPPDATA) {
    $sdkAdb = Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\adb.exe'
    if (Test-Path -LiteralPath $sdkAdb -PathType Leaf) { $adb = $sdkAdb }
}
if (-not $adb) {
    throw 'Android SDK Platform Tools (adb.exe) not found. Install Platform Tools in Android Studio.'
}

$devices = @(& $adb devices | Select-String '^(emulator-[0-9]+)\s+device$' |
    ForEach-Object { $_.Matches[0].Groups[1].Value })
if (-not $DeviceId) {
    if ($devices.Count -ne 1) {
        throw "Expected exactly one running emulator (found $($devices.Count)). Use -DeviceId emulator-5554. Run 'adb devices'."
    }
    $DeviceId = $devices[0]
}
if ($DeviceId -notmatch '^emulator-[0-9]+$') {
    throw 'This script only supports emulator-* device identifiers.'
}
Write-Host "Forwarding localhost:8765 to $($DeviceId):8765..." -ForegroundColor Cyan
& $adb -s $DeviceId forward tcp:8765 tcp:8765
if ($LASTEXITCODE -ne 0) { throw "ADB port-forward failed (exit code $LASTEXITCODE)." }
Write-Host "Use http://127.0.0.1:8765 in Synchro Desktop and the pairing token from the Android app." -ForegroundColor Green
Write-Host "Remove forward later: adb -s $DeviceId forward --remove tcp:8765"
