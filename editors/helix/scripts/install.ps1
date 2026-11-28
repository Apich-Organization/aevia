# Installs the Aevia language definition into the user's Helix configuration.
param([string]$ConfigDir, [switch]$Build)
$ErrorActionPreference = 'Stop'
if (-not $ConfigDir) {
    $baseConfig = if ($env:APPDATA) { $env:APPDATA }
        elseif ($env:XDG_CONFIG_HOME) { $env:XDG_CONFIG_HOME }
        else { Join-Path $HOME '.config' }
    $ConfigDir = Join-Path $baseConfig 'helix'
}
$here = Split-Path -Parent $PSScriptRoot
$grammar = (Resolve-Path (Join-Path $here '..\tree-sitter-aevia')).Path -replace '\\', '/'

New-Item -ItemType Directory -Force (Join-Path $ConfigDir 'runtime\queries\aevia') | Out-Null
New-Item -ItemType Directory -Force (Join-Path $ConfigDir 'runtime\grammars') | Out-Null
Copy-Item -Force (Join-Path $here 'runtime\queries\aevia\*.scm') (Join-Path $ConfigDir 'runtime\queries\aevia')

$target = Join-Path $ConfigDir 'languages.toml'
$snippet = (Get-Content -Raw (Join-Path $here 'languages.toml')).Replace('../tree-sitter-aevia', $grammar.Replace('"', '\"'))
$grammarSelection = 'use-grammars = { only = ["aevia"] }'
# The prefix includes both legacy installer-specific markers.
$markerPrefix = '# --- aevia (managed by editors/helix/scripts/install'
$hadTarget = Test-Path $target
$existing = if ($hadTarget) { Get-Content -Raw $target } else { '' }
if ($existing -match [regex]::Escape($markerPrefix)) {
    Write-Host "languages.toml already has the Aevia block; leaving it as is."
} else {
    $prefix = if (-not $hadTarget) {
        "# Only build the local aevia grammar (prebuilt system grammars stay active).`n$grammarSelection`n"
    } else { '' }
    [IO.File]::AppendAllText($target, "$prefix`n${markerPrefix}) ---`n$snippet", (New-Object Text.UTF8Encoding $false))
    Write-Host "appended the Aevia block to $target"
}

if ($Build) {
    $grammarDir = (Resolve-Path -LiteralPath (Join-Path $ConfigDir 'runtime/grammars')).Path
    $buildRoot = Join-Path $grammarDir ('.aevia-build-' + [guid]::NewGuid())
    $buildConfig = Join-Path $buildRoot 'config'
    $buildHelix = Join-Path $buildConfig 'helix'
    try {
        New-Item -ItemType Directory -Force -Path (Join-Path $buildRoot '.helix'), (Join-Path $buildHelix 'runtime/grammars') | Out-Null
        [IO.File]::WriteAllText((Join-Path $buildHelix 'languages.toml'), "$grammarSelection`n$snippet")

        $start = New-Object Diagnostics.ProcessStartInfo
        $start.FileName = (Get-Command hx -CommandType Application | Select-Object -First 1).Source
        $start.Arguments = '--grammar build'
        $start.WorkingDirectory = $buildRoot
        $start.UseShellExecute = $false
        $start.EnvironmentVariables.Remove('CARGO_MANIFEST_DIR')
        $cache = Join-Path $buildRoot 'cache'
        if ($IsWindows) {
            $start.EnvironmentVariables['APPDATA'] = $buildConfig
            $start.EnvironmentVariables['LOCALAPPDATA'] = $cache
            $parser = 'aevia.dll'
        } else {
            $start.EnvironmentVariables['XDG_CONFIG_HOME'] = $buildConfig
            $start.EnvironmentVariables['XDG_CACHE_HOME'] = $cache
            $parser = 'aevia.so'
        }
        $process = [Diagnostics.Process]::Start($start)
        try {
            $process.WaitForExit()
            if ($process.ExitCode -ne 0) { throw "Helix grammar build failed with exit code $($process.ExitCode)." }
        } finally {
            $process.Dispose()
        }
        [IO.File]::Move((Join-Path $buildHelix "runtime/grammars/$parser"), (Join-Path $grammarDir $parser), $true)
        Write-Host 'Next: hx --health aevia'
    } finally {
        if (Test-Path -LiteralPath $buildRoot) { Remove-Item -Recurse -Force -LiteralPath $buildRoot }
    }
} else {
    Write-Host 'Run this installer with -Build to build the Aevia parser (requires Git and a C/C++ toolchain).'
}
