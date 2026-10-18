# Synchro Android debug build: select a Gradle 8.13-compatible Java runtime
# without changing machine-wide JAVA_HOME or PATH.
$ErrorActionPreference = 'Stop'
$projectDir = Join-Path $PSScriptRoot '..\android'

$locations = New-Object System.Collections.Generic.List[string]
# The known local AndoCart toolchain is preferred for repeatable Kotlin builds.
if ($env:LOCALAPPDATA) {
    $locations.Add((Join-Path $env:LOCALAPPDATA 'AndoCart\toolchains\jdk17'))
}
if ($env:JAVA_HOME) {
    $locations.Add($env:JAVA_HOME)
}
if ($env:ProgramFiles) {
    $locations.Add((Join-Path $env:ProgramFiles 'Android\Android Studio\jbr'))
    $locations.Add((Join-Path $env:ProgramFiles 'Eclipse Adoptium'))
}
if ($env:LOCALAPPDATA) {
    $locations.Add((Join-Path $env:LOCALAPPDATA 'Programs\Android Studio\jbr'))
}

function Test-GradleCompatibleJdk([string]$home) {
    if (-not $home) { return $false }
    $java = Join-Path $home 'bin\java.exe'
    $release = Join-Path $home 'release'
    if (-not (Test-Path -LiteralPath $java -PathType Leaf)) { return $false }
    if (-not (Test-Path -LiteralPath $release -PathType Leaf)) { return $false }
    $info = Get-Content -LiteralPath $release -Raw
    if ($info -match '(?m)^JAVA_VERSION="?([0-9]+)') {
        $major = [int]$Matches[1]
        return ($major -ge 17 -and $major -le 23)
    }
    return $false
}

$jdk = $null
foreach ($location in $locations) {
    if (Test-GradleCompatibleJdk $location) {
        $jdk = $location
        break
    }
}
if (-not $jdk) {
    throw @'
No compatible JDK was found for Gradle 8.13.
Install JDK 17 or 21, or point JAVA_HOME at its installation directory.
Java 25 is not supported by Gradle 8.13.
Quick fix: $env:JAVA_HOME = "$env:LOCALAPPDATA\AndoCart\toolchains\jdk17"
'@
}

$env:JAVA_HOME = (Resolve-Path -LiteralPath $jdk).Path
$releaseInfo = Get-Content -LiteralPath (Join-Path $env:JAVA_HOME 'release') -Raw
$version = [regex]::Match($releaseInfo, '(?m)^JAVA_VERSION="([^"]+)"').Groups[1].Value
Write-Host ("Synchro: using JDK {0} from {1}" -f $version, $env:JAVA_HOME) -ForegroundColor Cyan

Push-Location $projectDir
try {
    & '.\gradlew.bat' --version
    if ($LASTEXITCODE -ne 0) { throw "Gradle launcher failed (exit code $LASTEXITCODE)." }
    & '.\gradlew.bat' assembleDebug
    if ($LASTEXITCODE -ne 0) { throw "Android build failed (exit code $LASTEXITCODE)." }

    $apk = Join-Path $projectDir 'app\build\outputs\apk\debug\app-debug.apk'
    if (-not (Test-Path -LiteralPath $apk -PathType Leaf)) {
        throw "Gradle returned success, but the expected APK was not found: $apk"
    }
    Write-Host "Synchro Android APK: $apk" -ForegroundColor Green
} finally {
    Pop-Location
}
