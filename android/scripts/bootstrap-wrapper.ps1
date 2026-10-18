# Download the official Gradle 8.13 Wrapper JAR from Gradle's tagged source.
# Verify SHA-256 against https://gradle.org/release-checksums/ before executing it.
$ErrorActionPreference = 'Stop'
$wrapperDir = Join-Path $PSScriptRoot '..\gradle\wrapper'
$jar = Join-Path $wrapperDir 'gradle-wrapper.jar'
$expected = '81a82aaea5abcc8ff68b3dfcb58b3c3c429378efd98e7433460610fecd7ae45f'
$rawUrl = 'https://raw.githubusercontent.com/gradle/gradle/v8.13.0/gradle/wrapper/gradle-wrapper.jar'
$apiUrl = 'https://api.github.com/repos/gradle/gradle/contents/gradle/wrapper/gradle-wrapper.jar?ref=v8.13.0'

New-Item -ItemType Directory -Force -Path $wrapperDir | Out-Null

function Assert-WrapperHash([string]$filePath) {
    $actual = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Gradle Wrapper JAR SHA-256 mismatch: $actual (expected: $expected)."
    }
}

if (Test-Path -LiteralPath $jar) {
    Assert-WrapperHash $jar
    Write-Host 'Gradle 8.13 Wrapper JAR already installed and verified.'
    exit 0
}

$temp = Join-Path $wrapperDir ("gradle-wrapper.jar.download." + $PID)
try {
    $downloaded = $false
    try {
        Write-Host 'Downloading Gradle 8.13 Wrapper JAR from official Gradle GitHub tag...'
        Invoke-WebRequest -Uri $rawUrl -OutFile $temp -UseBasicParsing -ErrorAction Stop
        $downloaded = $true
    } catch {
        Write-Warning "Raw GitHub download failed: $($_.Exception.Message)"
    }

    if (-not $downloaded) {
        Write-Host 'Trying the official GitHub API fallback...'
        $headers = @{
            'Accept' = 'application/vnd.github+json'
            'User-Agent' = 'Synchro-Gradle-Wrapper'
        }
        $response = Invoke-RestMethod -Uri $apiUrl -Headers $headers -ErrorAction Stop
        if ($response.encoding -ne 'base64' -or [string]::IsNullOrWhiteSpace($response.content)) {
            throw 'GitHub API did not return the expected base64-encoded Gradle Wrapper JAR.'
        }
        $bytes = [Convert]::FromBase64String(($response.content -replace '\s', ''))
        [System.IO.File]::WriteAllBytes($temp, $bytes)
    }

    Assert-WrapperHash $temp
    Move-Item -LiteralPath $temp -Destination $jar -Force
    Write-Host 'Gradle 8.13 Wrapper JAR downloaded and SHA-256 verified.'
} finally {
    if (Test-Path -LiteralPath $temp) {
        Remove-Item -LiteralPath $temp -Force
    }
}
