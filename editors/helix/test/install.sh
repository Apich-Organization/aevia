#!/usr/bin/env sh
# Exercise the installer with a pre-existing grammar choice in a temporary profile.
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"
test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT

unset CARGO_MANIFEST_DIR HELIX_RUNTIME
export XDG_CONFIG_HOME="$test_dir/with spaces" XDG_CACHE_HOME="$test_dir/cache"
config="$XDG_CONFIG_HOME/helix"
mkdir -p "$config" "$test_dir/.helix"
cd "$test_dir"
selection='use-grammars = { only = ["rust"] }'
printf '%s\n' "$selection" > "$config/languages.toml"

sh "$here/scripts/install.sh" --build
test "$(grep '^use-grammars' "$config/languages.toml")" = "$selection"
test -s "$config/runtime/grammars/aevia.so"
cp "$config/languages.toml" "$test_dir/installed.toml"
sh "$here/scripts/install.sh" --build
cmp "$test_dir/installed.toml" "$config/languages.toml"

health="$(hx --health aevia)"
for component in 'Tree-sitter parser' 'Highlight queries' 'Textobject queries'; do
  printf '%s\n' "$health" | grep "^$component: .*✓"
done
