#!/usr/bin/env sh
# Installs the Aevia language definition into the user's Helix configuration.
set -eu
build=false
if [ "${1:-}" = --build ]; then
  build=true
  shift
fi

here="$(cd "$(dirname "$0")/.." && pwd)"
config="${1:-${XDG_CONFIG_HOME:-$HOME/.config}/helix}"
grammar="$(cd "$here/../tree-sitter-aevia" && pwd)"
grammar_toml="$(printf '%s' "$grammar" | sed 's/[\\"]/\\&/g')"
grammar_replacement="$(printf '%s' "$grammar_toml" | sed 's/[\\&|]/\\&/g')"
grammar_selection='use-grammars = { only = ["aevia"] }'

render_languages() {
  sed "s|\.\./tree-sitter-aevia|$grammar_replacement|" "$here/languages.toml"
}

mkdir -p "$config/runtime/queries/aevia" "$config/runtime/grammars"
cp "$here"/runtime/queries/aevia/*.scm "$config/runtime/queries/aevia/"

target="$config/languages.toml"
# The prefix includes both legacy installer-specific markers.
marker_prefix='# --- aevia (managed by editors/helix/scripts/install'
if [ -f "$target" ] && grep -qF "$marker_prefix" "$target"; then
  echo "languages.toml already has the Aevia block; leaving it as is."
else
  if [ ! -f "$target" ]; then
    printf '# Only build the local aevia grammar (prebuilt system grammars stay active).\n%s\n' "$grammar_selection" > "$target"
  fi
  { echo; echo "$marker_prefix) ---"; render_languages; } >> "$target"
  echo "appended the Aevia block to $target"
fi

if "$build"; then
  grammar_dir="$(cd "$config/runtime/grammars" && pwd)"
  build_dir="$(mktemp -d "$grammar_dir/.aevia-build.XXXXXX")"
  trap 'rm -rf "$build_dir"' EXIT
  mkdir -p "$build_dir/.helix" "$build_dir/config/helix/runtime/grammars"
  { printf '%s\n' "$grammar_selection"; render_languages; } > "$build_dir/config/helix/languages.toml"
  (
    cd "$build_dir"
    unset CARGO_MANIFEST_DIR
    XDG_CONFIG_HOME="$build_dir/config" XDG_CACHE_HOME="$build_dir/cache" hx --grammar build
  )
  mv -f "$build_dir/config/helix/runtime/grammars/aevia.so" "$grammar_dir/aevia.so"
  echo "Next: hx --health aevia"
else
  echo "Run this installer with --build to build the Aevia parser (requires Git and a C/C++ toolchain)."
fi
