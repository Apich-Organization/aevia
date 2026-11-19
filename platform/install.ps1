<#
.SYNOPSIS
    Aevia toolchain installer for Windows.
.DESCRIPTION
    Downloads the Aevia release archive for windows-x86_64 or windows-aarch64,
    verifies its SHA-256 checksum and installs the toolchain to %LOCALAPPDATA%\aevia\bin,
    adds it to the user PATH, and installs PowerShell completions.
.PARAMETER Version
    Pass a published version to install a specific release, including a
    prerelease. The default is the latest stable GitHub release.
.EXAMPLE
    irm https://aevia.apich.org/install.ps1 | iex
.EXAMPLE
    .\platform\install.ps1 -Version 0.1.0-beta.1
    Run from a source checkout after this beta is published.
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
    try {
        $release = Invoke-RestMethod -Uri $apiUrl -Headers $headers
    } catch {
        throw "Failed to query the latest stable release from ${apiUrl}. Use -Version for a published prerelease. $($_.Exception.Message)"
    }
    if (-not $release.tag_name) {
        throw "Could not determine the latest stable release tag. Use -Version for a published prerelease."
    }
    return $release.tag_name
}

function Receive-ReleaseArchive {
    param([string]$Name, [switch]$AllowMissing)

    $uri = "$releaseUrl/$Name"
    $path = Join-Path $tmpDir $Name
    $headers = @{ "User-Agent" = "aevia-installer" }
    try {
        Invoke-WebRequest -Uri $uri -OutFile $path -Headers $headers -UseBasicParsing
    } catch {
        if ($AllowMissing -and $_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) {
            return $null
        }
        throw
    }

    $checksumPath = "$path.sha256"
    try {
        Invoke-WebRequest -Uri "$uri.sha256" -OutFile $checksumPath -Headers $headers -UseBasicParsing
    } catch {
        throw "Could not download the release checksum for ${Name}. $($_.Exception.Message)"
    }
    $checksum = Get-Content -LiteralPath $checksumPath -Raw
    $pattern = '\A([0-9a-fA-F]{64})[ \t]+\*?' + [regex]::Escape($Name) + '(?:\r?\n)?\z'
    if ($checksum -cnotmatch $pattern) {
        throw "Invalid release checksum for $Name."
    }
    $expected = $Matches[1]
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash -ine $expected) {
        throw "SHA-256 mismatch for $Name."
    }
    return $path
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
$ver = $tag -creplace '^v', ''
if ($ver -cnotmatch '\A[A-Za-z0-9._+-]+\z') {
    throw "Invalid release version: $tag"
}

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("aevia-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
    $releaseUrl = "https://github.com/$Repo/releases/download/$tag"
    $tarName = "aevia-$ver-$target.tar.gz"
    $zipName = "aevia-$ver-$target.zip"

    $extractDir = Join-Path $tmpDir "extract"
    New-Item -ItemType Directory -Path $extractDir -Force | Out-Null

    Write-Host "Downloading Aevia $tag ($target)..."
    $tar = Get-Command tar -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $tarPath = if ($tar) { Receive-ReleaseArchive -Name $tarName -AllowMissing }
    if ($tarPath) {
        & $tar.Source -xzf $tarPath -C $extractDir
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to extract $tarName (tar exited with status $LASTEXITCODE)."
        }
    } else {
        $zipPath = Receive-ReleaseArchive -Name $zipName
        Expand-Archive -LiteralPath $zipPath -DestinationPath $extractDir -Force
    }

    $source = Get-Item -LiteralPath (Join-Path $extractDir "aevia-$ver-$target") -Force -ErrorAction SilentlyContinue
    if ($source) {
        if (-not $source.PSIsContainer -or ($source.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "Invalid toolchain directory in the release archive."
        }
        $sourceDir = $source.FullName
    } else {
        # Older assets may place the toolchain directly at the archive root.
        $sourceDir = $extractDir
    }

    $binaries = foreach ($binName in @("aevia.exe", "aevia-lsp.exe", "aevia-pkg.exe")) {
        $file = Get-Item -LiteralPath (Join-Path $sourceDir $binName) -Force -ErrorAction SilentlyContinue
        if (-not $file) {
            if ($binName -eq "aevia.exe") { throw "Missing aevia.exe in the release archive." }
            continue
        }
        if ($file.PSIsContainer -or ($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -or $file.Length -eq 0) {
            throw "Invalid $binName executable in the release archive."
        }
        $file
    }

    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    foreach ($binary in $binaries) {
        Copy-Item -LiteralPath $binary.FullName -Destination (Join-Path $BinDir $binary.Name) -Force
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
