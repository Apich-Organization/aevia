#!/usr/bin/env sh
# Installs the Aevia language definition into the user's Helix configuration.
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"
config="${1:-${XDG_CONFIG_HOME:-$HOME/.config}/helix}"
grammar="$(cd "$here/../tree-sitter-aevia" && pwd)"

mkdir -p "$config/runtime/queries/aevia" "$config/runtime/grammars"
cp "$here"/runtime/queries/aevia/*.scm "$config/runtime/queries/aevia/"

target="$config/languages.toml"
marker='# --- aevia (managed by editors/helix/scripts/install.sh) ---'
if [ -f "$target" ] && grep -qF "$marker" "$target"; then
  echo "languages.toml already has the Aevia block; leaving it as is."
else
  if [ ! -f "$target" ]; then
    printf '# Only build the local aevia grammar (prebuilt system grammars stay active).\nuse-grammars = { only = ["aevia"] }\n' > "$target"
  fi
  { echo; echo "$marker"; sed "s|../tree-sitter-aevia|$grammar|" "$here/languages.toml"; } >> "$target"
  echo "appended the Aevia block to $target"
fi
if ! grep -q 'use-grammars' "$target"; then
  echo "Note: if 'hx --grammar build' fails on un-fetched built-in grammars, put 'use-grammars = { only = [\"aevia\"] }' at the top of $target."
fi
echo "Next: hx --grammar build   (needs a C compiler), then hx --health aevia"
