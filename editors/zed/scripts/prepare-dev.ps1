# Stages a development extension with a checkout-local tree-sitter grammar.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $PSScriptRoot
$grammarSrc = (Resolve-Path (Join-Path $here '../tree-sitter-aevia')).Path
$tempRoot = [IO.Path]::GetTempPath()
$work = Join-Path $tempRoot ('aevia-zed-' + [Guid]::NewGuid().ToString('N'))
$grammar = Join-Path $work 'grammar'
$extension = Join-Path $work 'extension'

New-Item -ItemType Directory (Join-Path $grammar 'src') -Force | Out-Null
New-Item -ItemType Directory $extension -Force | Out-Null
foreach ($item in 'grammar.js', 'package.json', 'tree-sitter.json', 'queries') {
    Copy-Item -Recurse (Join-Path $grammarSrc $item) $grammar
}
foreach ($item in 'grammar.json', 'node-types.json', 'parser.c', 'scanner.c', 'tree_sitter') {
    Copy-Item -Recurse (Join-Path $grammarSrc "src/$item") (Join-Path $grammar 'src')
}
foreach ($item in 'Cargo.toml', 'extension.toml', 'src', 'languages', 'debug_adapter_schemas') {
    Copy-Item -Recurse (Join-Path $here $item) $extension
}

function Invoke-Git {
    & git -C $grammar @args
    if ($LASTEXITCODE -ne 0) { throw "git failed while staging the grammar (exit $LASTEXITCODE)" }
}

Invoke-Git init -q
Invoke-Git add -A
Invoke-Git -c user.name=Aevia -c user.email=dev@localhost commit --no-gpg-sign -q -m grammar
$rev = (Invoke-Git rev-parse HEAD).Trim()
$url = [uri]::new($grammar, [UriKind]::Absolute).AbsoluteUri
$toml = Join-Path $extension 'extension.toml'
$text = Get-Content -Raw $toml
$text = $text -replace '(?ms)^\[grammars\.aevia\].*?(?=^\[|\z)', "[grammars.aevia]`nrepository = `"$url`"`nrev = `"$rev`"`n"
[IO.File]::WriteAllText($toml, $text, [Text.UTF8Encoding]::new($false))
Write-Host "In Zed, choose 'zed: install dev extension' and select $extension"
