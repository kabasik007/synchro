# Synchro: one-time bootstrap of the official Gradle 8.13 Wrapper JAR.
# We verify its SHA-256 against the value published by gradle.org.
$ErrorActionPreference = 'Stop'
$wrapperDir = Join-Path $PSScriptRoot '..\gradle\wrapper'
$jar = Join-Path $wrapperDir 'gradle-wrapper.jar'
$expected = '81a82aaea5abcc8ff68b3dfcb58b3c3c429378efd98e7433460610fecd7ae45f'
$url = 'https://services.gradle.org/distributions/gradle-8.13-wrapper.jar'
New-Item -ItemType Directory -Force -Path $wrapperDir | Out-Null
if (Test-Path -LiteralPath $jar) {
    $existingHash = (Get-FileHash -LiteralPath $jar -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($existingHash -ne $expected) {
        throw "Existing Gradle Wrapper JAR has wrong SHA-256. Remove $jar and retry."
    }
    exit 0
}
$temp = Join-Path $wrapperDir ("gradle-wrapper.jar.download." + $PID)
try {
    Write-Host 'Downloading verified Gradle 8.13 Wrapper JAR from services.gradle.org...'
    Invoke-WebRequest -Uri $url -OutFile $temp -UseBasicParsing
    $downloadHash = (Get-FileHash -LiteralPath $temp -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($downloadHash -ne $expected) {
        throw "Downloaded Gradle Wrapper JAR checksum mismatch: $downloadHash"
    }
    Move-Item -LiteralPath $temp -Destination $jar
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Force }
}
Write-Host 'Verified Gradle 8.13 Wrapper JAR.'
