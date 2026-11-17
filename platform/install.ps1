<#
.SYNOPSIS
    Aevia toolchain installer for Windows.
.DESCRIPTION
    Downloads the Aevia release archive for windows-x86_64 or windows-aarch64,
    extracts aevia.exe and aevia-lsp.exe to %LOCALAPPDATA%\aevia\bin,
    adds it to the user PATH, and installs PowerShell completions.
.EXAMPLE
    irm https://aevia.apich.org/install.ps1 | iex
.EXAMPLE
    .\install.ps1 -Version 0.1.0
#>
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"

$Repo = if ($env:AEVIA_GITHUB_REPO) { $env:AEVIA_GITHUB_REPO } else { "Apich-Organization/aevia" }
$InstallDir = if ($env:AEVIA_INSTALL_DIR) { $env:AEVIA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "aevia" }
$BinDir = Join-Path $InstallDir "bin"

function Get-Target {
    $arch = $null
    try {
        $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    } catch {
        $arch = $env:PROCESSOR_ARCHITECTURE.ToLowerInvariant()
    }

    switch ($arch) {
        "x64"   { return "windows-x86_64" }
        "amd64" { return "windows-x86_64" }
        "arm64" { return "windows-aarch64" }
        default { throw "Unsupported Windows architecture: $arch" }
    }
}

function Resolve-Tag {
    param([string]$Requested)

    if (-not [string]::IsNullOrWhiteSpace($Requested)) {
        if ($Requested.StartsWith("v")) {
            return $Requested
        }
        return "v$Requested"
    }

    $apiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    $headers = @{ "User-Agent" = "aevia-installer" }
    $release = Invoke-RestMethod -Uri $apiUrl -Headers $headers
    if (-not $release.tag_name) {
        throw "Could not determine latest release tag from $apiUrl"
    }
    return $release.tag_name
}

function Add-ToUserPath {
    param([string]$Dir)

    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $segments = @()
    if (-not [string]::IsNullOrWhiteSpace($currentUserPath)) {
        $segments = $currentUserPath.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    $normalizedDir = $Dir.TrimEnd("\")
    $alreadyPresent = $segments | Where-Object { $_.TrimEnd("\").Equals($normalizedDir, [System.StringComparison]::OrdinalIgnoreCase) }
    if (-not $alreadyPresent) {
        $newPath = if ($segments.Count -gt 0) { ($segments + $Dir) -join ";" } else { $Dir }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "Added $Dir to user PATH."
    }

    if (-not (($env:Path -split ";") | Where-Object { $_.TrimEnd("\").Equals($normalizedDir, [System.StringComparison]::OrdinalIgnoreCase) })) {
        $env:Path = "$Dir;$env:Path"
    }
}

function Install-Completions {
    param([string]$AeviaExe)

    if (-not (Test-Path $AeviaExe)) {
        return
    }

    try {
        $completionScript = & $AeviaExe completions powershell 2>$null
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace(($completionScript -join "`n"))) {
            $compFile = Join-Path $InstallDir "aevia-completions.ps1"
            $completionScript | Out-File -FilePath $compFile -Encoding utf8
            if ($PROFILE) {
                $profileDir = Split-Path -Parent $PROFILE
                if (-not (Test-Path $profileDir)) {
                    New-Item -ItemType Directory -Path $profileDir -Force | Out-Null
                }
                $sourceLine = ". `"$compFile`""
                if (-not (Test-Path $PROFILE) -or -not (Select-String -Path $PROFILE -Pattern ([regex]::Escape($compFile)) -Quiet)) {
                    Add-Content -Path $PROFILE -Value "`n# Aevia shell completions`n$sourceLine"
                }
            }
        }
    } catch {
        # Ignore if this version does not support `aevia completions powershell`.
    }
}

$target = Get-Target
$tag = Resolve-Tag -Requested $Version
$ver = $tag.TrimStart("v")

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("aevia-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
    $tarName = "aevia-$ver-$target.tar.gz"
    $tarUrl = "https://github.com/$Repo/releases/download/$tag/$tarName"
    $zipName = "aevia-$ver-$target.zip"
    $zipUrl = "https://github.com/$Repo/releases/download/$tag/$zipName"

    $extractDir = Join-Path $tmpDir "extract"
    New-Item -ItemType Directory -Path $extractDir -Force | Out-Null

    Write-Host "Downloading Aevia $tag ($target)..."
    $downloaded = $false
    $tarPath = Join-Path $tmpDir $tarName
    try {
        Invoke-WebRequest -Uri $tarUrl -OutFile $tarPath -Headers @{ "User-Agent" = "aevia-installer" } -UseBasicParsing
        tar -xzf $tarPath -C $extractDir
        $downloaded = $true
    } catch {
        $zipPath = Join-Path $tmpDir $zipName
        Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath -Headers @{ "User-Agent" = "aevia-installer" } -UseBasicParsing
        Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force
        $downloaded = $true
    }

    if (-not $downloaded) {
        throw "Failed to download release archive for $tag ($target)"
    }

    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    $installed = 0
    foreach ($binName in @("aevia.exe", "aevia-lsp.exe", "aevia-pkg.exe")) {
        $found = Get-ChildItem -Path $extractDir -Recurse -File -Filter $binName | Select-Object -First 1
        if ($found) {
            Copy-Item -Path $found.FullName -Destination (Join-Path $BinDir $binName) -Force
            $installed++
        }
    }

    if ($installed -eq 0) {
        throw "No aevia binaries found in downloaded archive."
    }

    Add-ToUserPath -Dir $BinDir
    Install-Completions -AeviaExe (Join-Path $BinDir "aevia.exe")

    Write-Host "Installed Aevia $tag to $BinDir"
    Write-Host "Run 'aevia --version' to verify (restart your terminal if PATH was just updated)."
} finally {
    if (Test-Path $tmpDir) {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
