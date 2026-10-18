# Synchro Android debug build: detect an installed JDK compatible with Gradle 8.13.
# Works with Windows PowerShell 5.1 and PowerShell 7. Does not change machine settings.
param(
    [string]$JdkHome = ''
)

$ErrorActionPreference = 'Stop'
$projectDir = Join-Path $PSScriptRoot '..\android'
$previousJavaHome = $env:JAVA_HOME
$candidates = New-Object 'System.Collections.Generic.List[string]'

function Add-JdkCandidate([string]$candidatePath) {
    if ([string]::IsNullOrWhiteSpace($candidatePath)) { return }
    try {
        $normalized = [System.IO.Path]::GetFullPath($candidatePath.Trim().Trim('"'))
    } catch {
        return
    }
    if ((Test-Path -LiteralPath $normalized -PathType Container) -and -not $candidates.Contains($normalized)) {
        $candidates.Add($normalized)
    }
}

function Get-InstalledJdk([string]$jdkDirectory) {
    if ([string]::IsNullOrWhiteSpace($jdkDirectory)) { return $null }
    $exe = Join-Path $jdkDirectory 'bin\java.exe'
    $release = Join-Path $jdkDirectory 'release'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { return $null }
    if (-not (Test-Path -LiteralPath $release -PathType Leaf)) { return $null }
    $info = Get-Content -LiteralPath $release -Raw
    if ($info -notmatch '(?m)^JAVA_VERSION="?([0-9]+)(?:\.([0-9]+))?') { return $null }
    $major = [int]$Matches[1]
    $versionMatch = [regex]::Match($info, '(?m)^JAVA_VERSION="?([^"\r\n]+)')
    $version = if ($versionMatch.Success) { $versionMatch.Groups[1].Value } else { "$major" }
    return [pscustomobject]@{
        Directory = $jdkDirectory
        Major = $major
        Version = $version
        Compatible = ($major -ge 17 -and $major -le 23)
    }
}

# Explicit overrides take priority. Never silently ignore an invalid override.
if (-not [string]::IsNullOrWhiteSpace($JdkHome)) {
    Add-JdkCandidate $JdkHome
    $explicitJdk = Get-InstalledJdk $JdkHome
    if ($null -eq $explicitJdk -or -not $explicitJdk.Compatible) {
        throw "The supplied -JdkHome is not a compatible JDK (Java 17-23): $JdkHome"
    }
}

# Typical fixed locations and configured JAVA_HOME.
Add-JdkCandidate $env:JAVA_HOME
if ($env:LOCALAPPDATA) {
    Add-JdkCandidate (Join-Path $env:LOCALAPPDATA 'AndoCart\toolchains\jdk17')
    Add-JdkCandidate (Join-Path $env:LOCALAPPDATA 'Programs\Android Studio\jbr')
}
if ($env:ProgramFiles) {
    Add-JdkCandidate (Join-Path $env:ProgramFiles 'Android\Android Studio\jbr')
}

# Android Studio can register a separate JBR 21 under ~/.jdks; most JDK installers
# also put a versioned child directory under Program Files.
$scanRoots = New-Object 'System.Collections.Generic.List[string]'
if ($env:USERPROFILE) {
    $scanRoots.Add((Join-Path $env:USERPROFILE '.jdks'))
    $scanRoots.Add((Join-Path $env:USERPROFILE '.gradle\jdks'))
}
if ($env:LOCALAPPDATA) {
    $scanRoots.Add((Join-Path $env:LOCALAPPDATA 'AndoCart\toolchains'))
    $scanRoots.Add((Join-Path $env:LOCALAPPDATA 'Programs\Android Studio'))
}
if ($env:ProgramFiles) {
    foreach ($folder in @('Eclipse Adoptium', 'Microsoft', 'Java', 'Amazon Corretto', 'Zulu',
                          'BellSoft', 'Android', 'JetBrains')) {
        $scanRoots.Add((Join-Path $env:ProgramFiles $folder))
    }
}

foreach ($root in $scanRoots) {
    if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
    Add-JdkCandidate $root
    foreach ($child in (Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue)) {
        Add-JdkCandidate $child.FullName
        # For Android Studio's own bundled runtime and one extra nested JDK level.
        if ($child.Name -match '^(Android Studio|jdk|jbr|temurin|adoptium)' -or
            $root -like '*\.gradle\jdks') {
            foreach ($nested in (Get-ChildItem -LiteralPath $child.FullName -Directory -ErrorAction SilentlyContinue)) {
                if ($nested.Name -in @('jbr', 'jdk', 'Contents', 'Home') -or $root -like '*\.gradle\jdks') {
                    Add-JdkCandidate $nested.FullName
                }
            }
        }
    }
}

$selected = $null
foreach ($candidate in $candidates) {
    $found = Get-InstalledJdk $candidate
    if ($null -eq $found) { continue }
    if ($found.Compatible) {
        Write-Host ("Found compatible Java {0}: {1}" -f $found.Version, $found.Directory) -ForegroundColor DarkCyan
        if ($null -eq $selected) { $selected = $found }
    } else {
        Write-Host ("Skipping incompatible Java {0}: {1}" -f $found.Version, $found.Directory) -ForegroundColor Yellow
    }
}

if ($null -eq $selected) {
    Write-Host 'Synchro could not find an installed JDK 17-23.' -ForegroundColor Yellow
    Write-Host 'Look at Android Studio > Settings > Build Tools > Gradle > Gradle JDK.'
    Write-Host 'If JBR 21 is selected there, copy its full directory path and run:'
    Write-Host '  .\scripts\build-android.ps1 -JdkHome "C:\path\to\jbr-21"'
    Write-Host 'Alternatively install JDK 17, reopen PowerShell, then retry:'
    Write-Host '  winget install --exact --id EclipseAdoptium.Temurin.17.JDK'
    throw 'A compatible JDK is required to build the Android APK. Java 8 and Java 25 are not supported by Gradle 8.13.'
}

Push-Location $projectDir
try {
    $env:JAVA_HOME = $selected.Directory
    Write-Host ("Synchro: using Java {0} at {1}" -f $selected.Version, $env:JAVA_HOME) -ForegroundColor Cyan
    & '.\gradlew.bat' --version
    if ($LASTEXITCODE -ne 0) { throw "Gradle launcher failed (exit code $LASTEXITCODE)." }
    & '.\gradlew.bat' assembleDebug
    if ($LASTEXITCODE -ne 0) { throw "Android build failed (exit code $LASTEXITCODE)." }

    $apk = Join-Path $projectDir 'app\build\outputs\apk\debug\app-debug.apk'
    if (-not (Test-Path -LiteralPath $apk -PathType Leaf)) {
        throw "Gradle returned success but the APK is missing: $apk"
    }
    Write-Host "Synchro Android APK: $apk" -ForegroundColor Green
} finally {
    $env:JAVA_HOME = $previousJavaHome
    Pop-Location
}
