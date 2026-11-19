# Installs the Aevia language definition into the user's Helix configuration.
param([string]$ConfigDir)
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
$marker = '# --- aevia (managed by editors/helix/scripts/install.ps1) ---'
$hadTarget = Test-Path $target
$existing = if ($hadTarget) { Get-Content -Raw $target } else { '' }
if ($existing -match [regex]::Escape($marker)) {
    Write-Host "languages.toml already has the Aevia block; leaving it as is."
} else {
    $prefix = if (-not $hadTarget) {
        "# Only build the local aevia grammar (prebuilt system grammars stay active).`nuse-grammars = { only = [`"aevia`"] }`n"
    } else { '' }
    [IO.File]::AppendAllText($target, "$prefix`n$marker`n$snippet", (New-Object Text.UTF8Encoding $false))
    Write-Host "appended the Aevia block to $target"
}
if ((Get-Content -Raw $target) -notmatch 'use-grammars') {
    Write-Host "Note: if 'hx --grammar build' fails on un-fetched built-in grammars, put 'use-grammars = { only = [`"aevia`"] }' at the top of $target."
}
Write-Host "Next: hx --grammar build   (needs a C compiler), then hx --health aevia"
