#!/usr/bin/env sh
# Installs the Aevia language definition into the user's Helix configuration.
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"
config="${1:-${XDG_CONFIG_HOME:-$HOME/.config}/helix}"
grammar="$(cd "$here/../tree-sitter-aevia" && pwd)"

mkdir -p "$config/runtime/queries/aevia"
cp "$here"/runtime/queries/aevia/*.scm "$config/runtime/queries/aevia/"

target="$config/languages.toml"
marker='# --- aevia (managed by editors/helix/scripts/install.sh) ---'
if [ -f "$target" ] && grep -qF "$marker" "$target"; then
  echo "languages.toml already has the Aevia block; leaving it as is."
else
  { echo; echo "$marker"; sed "s|../tree-sitter-aevia|$grammar|" "$here/languages.toml"; } >> "$target"
  echo "appended the Aevia block to $target"
fi
echo "Next: hx --grammar build   (needs a C compiler), then hx --health aevia"
