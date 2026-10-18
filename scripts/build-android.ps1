$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..\android')
if (Test-Path '.\gradlew.bat') {
    & .\gradlew.bat assembleDebug
} else {
    & gradle assembleDebug
}
Write-Host 'Built: android\app\build\outputs\apk\debug\app-debug.apk'
