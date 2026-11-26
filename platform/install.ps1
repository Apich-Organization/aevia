<#
.SYNOPSIS
    Bootstrap a verified Windows toolchain from an immutable GitHub release.
.DESCRIPTION
    Downloads over HTTPS. With GitHub CLI installed the release attestation is
    verified in addition to the SHA-256 checksum; without it the checksum is
    still enforced. The default installation directory is %LOCALAPPDATA%\aevia,
    or $env:AEVIA_HOME.
.PARAMETER Channel
    stable (default), nightly, or dev (an alias for nightly).
.PARAMETER Version
    A SemVer release with or without v, or nightly-YYYY-MM-DD.
.PARAMETER NoPath
    Skip PATH and PowerShell completion changes.
.PARAMETER NoActivate
    Install alongside other versions without changing the active toolchain.
.EXAMPLE
    irm https://aevia.apich.org/install.ps1 | iex
.EXAMPLE
    .\platform\install.ps1 -Version 0.1.0-beta.1
#>
[CmdletBinding(DefaultParameterSetName = 'Channel')]
param(
    [Parameter(ParameterSetName = 'Channel')]
    [ValidateSet('stable', 'dev', 'nightly')]
    [string]$Channel = 'stable',

    [Parameter(Mandatory, ParameterSetName = 'Version')]
    [ValidateNotNullOrEmpty()]
    [string]$Version,

    [ValidateNotNullOrEmpty()]
    [string]$HomeDirectory,

    [switch]$NoPath,

    [switch]$NoActivate
)

$ErrorActionPreference = 'Stop'

function Resolve-Tag {
    param([string]$Requested, [string]$SelectedChannel)

    $core = '(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
    $nightly = 'nightly-[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])'
    if ($SelectedChannel -eq 'exact') {
        if ($Requested -cmatch "\A$nightly\z") {
            return $Requested
        }
        $number = $Requested -creplace '^v', ''
        $identifier = '(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
        if ($number.Length -gt 80 -or $number -cnotmatch "\A$core(-$identifier(\.$identifier)*)?\z") {
            throw "Invalid release version: $Requested"
        }
        return "v$number"
    }

    if ($SelectedChannel -eq 'stable') {
        try {
            $latest = Get-Release 'releases/latest'
        } catch {
            throw "Could not look up the latest stable release. Pass -Version instead."
        }
        if ($latest.draft -ne $false -or $latest.prerelease -ne $false -or $latest.immutable -ne $true) {
            throw 'No immutable stable release found. Use -Version or -Channel nightly.'
        }
        $tagName = $latest.tag_name
        if ($tagName -isnot [string] -or $tagName -cnotmatch "\Av$core\z") {
            throw 'No immutable stable release found. Use -Version or -Channel nightly.'
        }
        return $tagName
    }

    $tag = $null
    for ($page = 1; ; $page++) {
        try {
            $releases = @(Get-Release "releases?per_page=100&page=$page")
        } catch {
            throw "Could not look up nightly releases. Pass -Version instead."
        }
        if ($releases.Count -eq 1 -and $releases[0] -is [array]) {
            $releases = $releases[0]
        }
        foreach ($entry in $releases) {
            if ($entry.draft -ne $false -or $entry.prerelease -ne $true -or $entry.immutable -ne $true) {
                continue
            }
            $candidate = $entry.tag_name
            if ($candidate -cnotmatch "\A$nightly\z") { continue }
            if ($tag -and [string]::CompareOrdinal($candidate, $tag) -le 0) { continue }
            $archive = "aevia-$candidate-$target.zip"
            $names = @($entry.assets | Where-Object { $_.state -ceq 'uploaded' } | ForEach-Object { $_.name })
            if ($names -ccontains $archive -and $names -ccontains "$archive.sha256") {
                $tag = $candidate
            }
        }
        if ($releases.Count -lt 100) { break }
    }
    if (-not $tag) { throw "No immutable nightly release with an archive for $target." }
    return $tag
}

function Enable-ShellIntegration {
    param([string]$Prefix, [string]$AeviaExe)

    $bin = Join-Path $Prefix 'bin'
    $segments = @([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ })
    if ($segments -inotcontains $bin) {
        [Environment]::SetEnvironmentVariable('Path', ((@($bin) + $segments) -join ';'), 'User')
    }
    if (($env:Path -split ';') -inotcontains $bin) {
        $env:Path = "$bin;$env:Path"
    }

    $script = & $AeviaExe completions powershell
    if ($LASTEXITCODE -ne 0 -or -not ($script -join "`n").Trim()) {
        Write-Warning 'Could not generate PowerShell completions.'
        return
    }
    $completion = Join-Path $Prefix 'aevia-completions.ps1'
    $stage = "$completion.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        $script | Set-Content -LiteralPath $stage -Encoding UTF8
        Move-Item -LiteralPath $stage -Destination $completion -Force
    } finally {
        if (Test-Path -LiteralPath $stage) {
            Remove-Item -LiteralPath $stage -Force
        }
    }

    $profileFile = $PROFILE.CurrentUserAllHosts
    $literal = "'" + $completion.Replace("'", "''") + "'"
    $line = "if (Test-Path -LiteralPath $literal) { . $literal }"
    if (-not (Test-Path -LiteralPath $profileFile) -or
        -not ((Get-Content -LiteralPath $profileFile) -ccontains $line)) {
        New-Item -ItemType Directory -Path (Split-Path -Parent $profileFile) -Force | Out-Null
        Add-Content -LiteralPath $profileFile -Value "`n$line"
    }
}

if (-not $HomeDirectory) {
    $HomeDirectory = $env:AEVIA_HOME
    if (-not $HomeDirectory) { $HomeDirectory = $env:AEVIA_INSTALL_DIR }
    if (-not $HomeDirectory) { $HomeDirectory = Join-Path $env:LOCALAPPDATA 'aevia' }
}
$provider = $null
$drive = $null
$prefix = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath(
    $HomeDirectory, [ref]$provider, [ref]$drive
)
if ($provider.Name -ne 'FileSystem') {
    throw 'The installation path must resolve to the filesystem.'
}
if (-not $NoPath -and -not $NoActivate -and $prefix -match '[;\r\n]') {
    throw 'Use -NoPath with a semicolon or newline in the installation path.'
}

$Repo = if ($env:AEVIA_GITHUB_REPO) { $env:AEVIA_GITHUB_REPO } else { 'Apich-Organization/aevia' }
if ($Repo -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9_][A-Za-z0-9_.-]*\z') {
    throw 'AEVIA_GITHUB_REPO must be OWNER/REPO.'
}
# GitHub CLI is optional: metadata comes from the public REST API and the
# archive from a plain HTTPS download. `gh`, when present, adds attestation
# verification on top of the SHA-256 checksum.
$gh = $null
$candidate = (Get-Command gh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1)
if ($candidate) {
    & $candidate.Source release verify-asset --help > $null
    if ($LASTEXITCODE -eq 0) { $gh = $candidate.Source }
}
$apiHeaders = @{ Accept = 'application/vnd.github+json' }
if ($env:GITHUB_TOKEN) { $apiHeaders['Authorization'] = "Bearer $($env:GITHUB_TOKEN)" }

function Get-Release {
    param([string]$Path)
    $uri = "https://api.github.com/repos/$Repo/$Path"
    Invoke-RestMethod -Uri $uri -Headers $apiHeaders -Method Get -ErrorAction Stop
}

try {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
} catch {
    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
}
$target = switch ($arch) {
    { $_ -in 'X64', 'AMD64' } { 'x86_64-pc-windows-msvc' }
    'ARM64' { 'aarch64-pc-windows-msvc' }
    default { throw "Unsupported Windows architecture: $arch" }
}

$selected = if ($Version) { 'exact' } elseif ($Channel -eq 'stable') { 'stable' } else { 'nightly' }
$tag = Resolve-Tag -Requested $Version -SelectedChannel $selected
$releaseId = $tag -creplace '^v', ''
$bundle = "aevia-$releaseId-$target"
$archive = "$bundle.zip"
$tmpDir = Join-Path ([IO.Path]::GetTempPath()) ("aevia-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmpDir | Out-Null
try {
    Write-Host "Downloading Aevia $tag ($target)"
    foreach ($name in @($archive, "$archive.sha256")) {
        $uri = "https://github.com/$Repo/releases/download/$tag/$name"
        try {
            Invoke-WebRequest -Uri $uri -OutFile (Join-Path $tmpDir $name) -UseBasicParsing -ErrorAction Stop
        } catch {
            throw "Could not download $name from $Repo/$tag."
        }
    }

    $path = Join-Path $tmpDir $archive
    $checksum = Get-Content -LiteralPath "$path.sha256" -Raw
    $pattern = '\A([a-fA-F0-9]{64})[ \t]+\*?' + [regex]::Escape($archive) + '(?:\r?\n)?\z'
    if ($checksum -cnotmatch $pattern) { throw "Invalid checksum for $archive." }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash -ine $Matches[1]) {
        throw "SHA-256 mismatch for $archive."
    }
    if ($gh) {
        & $gh release verify-asset $tag $path --repo "github.com/$Repo"
        if ($LASTEXITCODE -ne 0) { throw "GitHub release attestation verification failed for $archive." }
    } else {
        Write-Warning 'Checksum verified; install GitHub CLI to also verify the release attestation.'
    }

    $unpacked = Join-Path $tmpDir 'unpacked'
    Expand-Archive -LiteralPath $path -DestinationPath $unpacked
    $source = Get-Item -LiteralPath (Join-Path $unpacked $bundle) -Force -ErrorAction SilentlyContinue
    if ($null -eq $source -or -not $source.PSIsContainer -or ($source.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Invalid archive directory: $bundle."
    }
    foreach ($executable in 'aevia.exe', 'aevia-lsp.exe', 'aevia-pkg.exe') {
        $file = Get-Item -LiteralPath (Join-Path $source.FullName $executable) -Force -ErrorAction SilentlyContinue
        if ($null -eq $file -or $file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $file.Length -eq 0) {
            throw "Invalid executable: $executable."
        }
    }
    $aevia = Join-Path $source.FullName 'aevia.exe'
    $register = @('toolchain', 'register', $tag, $source.FullName, '--home', $prefix, '--channel', $selected)
    if ($NoActivate) { $register += '--no-activate' }
    & $aevia @register
    if ($LASTEXITCODE -ne 0) { throw 'Could not register the verified toolchain.' }
    if (-not $NoPath -and -not $NoActivate) {
        Enable-ShellIntegration -Prefix $prefix -AeviaExe $aevia
        Write-Host 'Restart other terminals to use the updated PATH.'
    }
} finally {
    Remove-Item -LiteralPath $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
