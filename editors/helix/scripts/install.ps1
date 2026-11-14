# Installs the Aevia language definition into the user's Helix configuration.
param([string]$ConfigDir = (Join-Path $env:APPDATA 'helix'))
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $PSScriptRoot
$grammar = (Resolve-Path (Join-Path $here '..\tree-sitter-aevia')).Path -replace '\\', '/'

New-Item -ItemType Directory -Force (Join-Path $ConfigDir 'runtime\queries\aevia') | Out-Null
Copy-Item -Force (Join-Path $here 'runtime\queries\aevia\*.scm') (Join-Path $ConfigDir 'runtime\queries\aevia')

$target = Join-Path $ConfigDir 'languages.toml'
$snippet = (Get-Content -Raw (Join-Path $here 'languages.toml')) -replace '\.\./tree-sitter-aevia', $grammar
$marker = '# --- aevia (managed by editors/helix/scripts/install.ps1) ---'
$existing = if (Test-Path $target) { Get-Content -Raw $target } else { '' }
if ($existing -match [regex]::Escape($marker)) {
    Write-Host "languages.toml already has the Aevia block; leaving it as is."
} else {
    [IO.File]::AppendAllText($target, "`n$marker`n$snippet", (New-Object Text.UTF8Encoding $false))
    Write-Host "appended the Aevia block to $target"
}
Write-Host "Next: hx --grammar build   (needs a C compiler), then hx --health aevia"
