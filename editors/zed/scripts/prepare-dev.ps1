# Points extension.toml at a local git commit of ../tree-sitter-aevia so that
# "zed: install dev extension" can fetch the grammar without publishing it.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $PSScriptRoot
$grammarSrc = Resolve-Path (Join-Path $here '..\tree-sitter-aevia')
$work = Join-Path $env:TEMP 'tree-sitter-aevia-dev'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory $work | Out-Null
foreach ($item in 'grammar.js', 'package.json', 'tree-sitter.json', 'src', 'queries') {
    Copy-Item -Recurse (Join-Path $grammarSrc $item) $work
}
git -C $work init -q
git -C $work add -A
git -C $work -c user.name=dev -c user.email=dev@localhost commit -q -m grammar
$rev = (git -C $work rev-parse HEAD).Trim()
$url = 'file:///' + ($work -replace '\\', '/')
$toml = Join-Path $here 'extension.toml'
$text = Get-Content -Raw $toml
$text = $text -replace '(?ms)\[grammars\.aevia\].*', "[grammars.aevia]`nrepository = `"$url`"`nrev = `"$rev`"`n"
[IO.File]::WriteAllText($toml, $text, (New-Object Text.UTF8Encoding $false))
Write-Host "grammar commit $rev at $url"
Write-Host "Now: Zed > command palette > 'zed: install dev extension' > select $here"
Write-Host "(extension.toml was rewritten; do not commit that change)"
