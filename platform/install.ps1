<#
.SYNOPSIS
    Bootstrap a verified Windows toolchain from an immutable GitHub release.
.DESCRIPTION
    Requires GitHub CLI with release verify-asset and 'gh auth login'.
    The default installation directory is %LOCALAPPDATA%\aevia, or $env:AEVIA_HOME.
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
        $query = 'select(.draft == false and .prerelease == false and .immutable == true) | .tag_name'
        $latest = & $gh api --hostname github.com "repos/$Repo/releases/latest" --jq $query
        if ($LASTEXITCODE -ne 0) {
            throw "Could not look up the latest stable release. Run 'gh auth login' or pass -Version."
        }
        if ($latest -isnot [string] -or $latest -cnotmatch "\Av$core\z") {
            throw 'No immutable stable release found. Use -Version or -Channel nightly.'
        }
        return $latest
    }

    # Parse each page returned by gh as one JSON array to avoid the quoting
    # differences for embedded jq literals between Windows PowerShell and pwsh.
    $tag = $null
    for ($page = 1; ; $page++) {
        $body = & $gh api --hostname github.com --method GET "repos/$Repo/releases" --field per_page=100 --field "page=$page"
        if ($LASTEXITCODE -ne 0) {
            throw "Could not look up nightly releases. Run 'gh auth login' or pass -Version."
        }
        $releases = @(($body -join "`n") | ConvertFrom-Json)
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
$prefix = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($HomeDirectory)
if (-not $NoPath -and -not $NoActivate -and $prefix -match '[;\r\n]') {
    throw 'Use -NoPath with a semicolon or newline in the installation path.'
}

$Repo = if ($env:AEVIA_GITHUB_REPO) { $env:AEVIA_GITHUB_REPO } else { 'Apich-Organization/aevia' }
if ($Repo -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9_][A-Za-z0-9_.-]*\z') {
    throw 'AEVIA_GITHUB_REPO must be OWNER/REPO.'
}
$gh = (Get-Command gh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $gh) { throw "Install GitHub CLI (gh) and run 'gh auth login' first." }
& $gh release verify-asset --help > $null
if ($LASTEXITCODE -ne 0) {
    throw "Update GitHub CLI to a version with 'gh release verify-asset'."
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
    & $gh release download $tag --repo "github.com/$Repo" --pattern $archive --pattern "$archive.sha256" --dir $tmpDir
    if ($LASTEXITCODE -ne 0) { throw "Could not download $archive and its checksum from $Repo/$tag." }

    $path = Join-Path $tmpDir $archive
    $checksum = Get-Content -LiteralPath "$path.sha256" -Raw
    $pattern = '\A([a-fA-F0-9]{64})[ \t]+\*?' + [regex]::Escape($archive) + '(?:\r?\n)?\z'
    if ($checksum -cnotmatch $pattern) { throw "Invalid checksum for $archive." }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash -ine $Matches[1]) {
        throw "SHA-256 mismatch for $archive."
    }
    & $gh release verify-asset $tag $path --repo "github.com/$Repo"
    if ($LASTEXITCODE -ne 0) { throw "GitHub release attestation verification failed for $archive." }

    $unpacked = Join-Path $tmpDir 'unpacked'
    Expand-Archive -LiteralPath $path -DestinationPath $unpacked
    $source = Get-Item -LiteralPath (Join-Path $unpacked $bundle) -Force
    if (-not $source.PSIsContainer -or ($source.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Invalid archive directory: $bundle."
    }
    foreach ($executable in 'aevia.exe', 'aevia-lsp.exe', 'aevia-pkg.exe') {
        $file = Get-Item -LiteralPath (Join-Path $source.FullName $executable) -Force
        if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $file.Length -eq 0) {
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
