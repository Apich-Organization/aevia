#!/bin/sh
# Guards infra/lang-workspace.toml against drifting from the language
# repository.
#
# The registry's documentation feature compiles four crates from the language
# checkout (aevia-doc and its dependencies). Those crates inherit their version,
# edition and external dependencies from the language workspace root, which sits
# outside the container build context, so the Containerfile supplies a mirror of
# that root instead. A mirror can go stale — this script is how you find out
# before a build does.
#
# Usage:  sh infra/check-lang-workspace.sh [path-to-language-repo]
# Exit:   0 when the mirror matches, 1 when it does not, 2 when a file is missing.
set -eu

language_repo=${1:-..}
root="$language_repo/Cargo.toml"
mirror="$(dirname "$0")/lang-workspace.toml"

[ -f "$root" ] || { echo "check-lang-workspace: no $root; pass the language repository path" >&2; exit 2; }
[ -f "$mirror" ] || { echo "check-lang-workspace: no $mirror" >&2; exit 2; }

status=0

# 1. Every key a mirrored crate inherits must exist in the mirror with the same
#    value as in the real root.
for crate in aevia-doc aevia-syntax aevia-fmt aevia-dim; do
    manifest="$language_repo/crates/$crate/Cargo.toml"
    [ -f "$manifest" ] || { echo "check-lang-workspace: no $manifest" >&2; exit 2; }
    keys=$(sed -n 's/^\([a-zA-Z0-9_-]*\)\.workspace = true$/\1/p' "$manifest")
    for key in $keys; do
        real=$(grep -m1 "^$key = " "$root" || true)
        mine=$(grep -m1 "^$key = " "$mirror" || true)
        if [ -z "$real" ]; then
            echo "MISSING in $root: $key (inherited by $crate)" >&2
            status=1
        elif [ "$real" != "$mine" ]; then
            echo "DRIFT for $key (used by $crate):" >&2
            echo "  language: $real" >&2
            echo "  mirror:   ${mine:-<absent>}" >&2
            status=1
        fi
    done
done

# 2. The inherited package fields must match too: a different edition or
#    rust-version would build a different binary than the one tested locally.
for field in version edition rust-version license repository; do
    real=$(sed -n "/^\[workspace.package\]/,/^\[/p" "$root" | grep -m1 "^$field = " || true)
    mine=$(sed -n "/^\[workspace.package\]/,/^\[/p" "$mirror" | grep -m1 "^$field = " || true)
    if [ "$real" != "$mine" ]; then
        echo "DRIFT for [workspace.package] $field:" >&2
        echo "  language: ${real:-<absent>}" >&2
        echo "  mirror:   ${mine:-<absent>}" >&2
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "check-lang-workspace: $mirror matches $root"
fi
exit "$status"
