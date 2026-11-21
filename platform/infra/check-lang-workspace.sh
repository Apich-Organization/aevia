#!/bin/sh
# Guards infra/lang-workspace.toml against drifting from the language
# repository.
#
# The registry uses five language crates for documentation and dependency
# resolution. They inherit settings from the repository's workspace manifest.
# The container copies the relevant settings from infra/lang-workspace.toml
# because the language build context only contains the crates/ directory.
#
# Usage:  sh platform/infra/check-lang-workspace.sh [path-to-language-repo]
# The default language repository is located relative to this script.
# Exit:   0 when the mirror matches, 1 when it does not, 2 when a file is missing.
set -eu

infra_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
language_repo=${1:-$(CDPATH= cd -- "$infra_dir/../.." && pwd)}
root="$language_repo/Cargo.toml"
mirror="$infra_dir/lang-workspace.toml"
crates="aevia-dim aevia-syntax aevia-fmt aevia-doc aevia-resolve"

[ -f "$root" ] || { echo "check-lang-workspace: no $root; pass the language repository path" >&2; exit 2; }
[ -f "$mirror" ] || { echo "check-lang-workspace: no $mirror" >&2; exit 2; }

status=0

table_value() {
    awk -v table="[$2]" -v key="$3" '
        /^[[:blank:]]*\[/ {
            heading = $0
            sub(/^[[:blank:]]*/, "", heading)
            sub(/\][[:blank:]]*(#.*)?$/, "]", heading)
            active = (heading == table)
        }
        active && $0 ~ ("^[[:blank:]]*" key "[[:blank:]]*=") {
            sub(/^[^=]*=[[:blank:]]*/, "")
            sub(/[[:blank:]]*$/, "")
            print
            exit
        }
    ' "$1"
}

compare_field() {
    table=$1
    field=$2
    usage=${3:+ (used by $3)}
    real=$(table_value "$root" "$table" "$field")
    mine=$(table_value "$mirror" "$table" "$field")
    if [ -z "$real" ] || [ "$real" != "$mine" ]; then
        printf 'DRIFT for [%s] %s%s:\n' "$table" "$field" "$usage" >&2
        printf '  language: %s\n  mirror:   %s\n' "${real:-<absent>}" "${mine:-<absent>}" >&2
        status=1
    fi
}

inherited_settings() {
    awk '
        /^[[:blank:]]*\[/ {
            heading = $0
            sub(/^[[:blank:]]*/, "", heading)
            sub(/\][[:blank:]]*(#.*)?$/, "]", heading)
            section = ""
            dependency = ""
            if (heading == "[package]") {
                section = "workspace.package"
            } else if (heading ~ /(\[|\.)((build|dev)-)?dependencies(\.[A-Za-z0-9_-]+)?\]$/) {
                section = "workspace.dependencies"
                if (heading ~ /dependencies\.[A-Za-z0-9_-]+\]$/) {
                    dependency = heading
                    sub(/^.*dependencies\./, "", dependency)
                    sub(/\]$/, "", dependency)
                }
            }
            next
        }
        !section { next }
        dependency && /^[[:blank:]]*workspace[[:blank:]]*=[[:blank:]]*true([[:blank:]]*(#.*)?)$/ {
            print section ":" dependency
            next
        }
        /^[[:blank:]]*[A-Za-z0-9_-]+\.workspace[[:blank:]]*=[[:blank:]]*true([[:blank:]]*(#.*)?)$/ ||
        (section == "workspace.dependencies" &&
            /^[[:blank:]]*[A-Za-z0-9_-]+[[:blank:]]*=[[:blank:]]*\{([^}]*,)?[[:blank:]]*workspace[[:blank:]]*=[[:blank:]]*true[[:blank:]]*[,}]/) {
            sub(/^[[:blank:]]*/, "")
            sub(/[^A-Za-z0-9_-].*/, "")
            print section ":" $0
        }
    ' "$1"
}

# Compare the settings inherited by the language crates.
for crate in $crates; do
    manifest="$language_repo/crates/$crate/Cargo.toml"
    [ -f "$manifest" ] || { echo "check-lang-workspace: no $manifest" >&2; exit 2; }
    settings=$(inherited_settings "$manifest")
    for setting in $settings; do
        compare_field "${setting%:*}" "${setting#*:}" "$crate"
    done
done

compare_field workspace resolver

workspace_members() {
    awk '
        /^[[:blank:]]*\[/ {
            workspace = ($0 ~ /^[[:blank:]]*\[workspace\][[:blank:]]*(#.*)?$/)
        }
        workspace && /^[[:blank:]]*members[[:blank:]]*=[[:blank:]]*\[/ {
            sub(/^[^[]*\[/, "")
            members = 1
        }
        members {
            sub(/#.*/, "")
            line = $0
            while (match(line, /"[^"]+"/)) {
                print substr(line, RSTART + 1, RLENGTH - 2)
                line = substr(line, RSTART + RLENGTH)
            }
            if ($0 ~ /\]/) exit
        }
    ' "$1" | LC_ALL=C sort
}

expected_members=$(for crate in $crates; do printf 'crates/%s\n' "$crate"; done | LC_ALL=C sort)
if [ "$(workspace_members "$mirror")" != "$expected_members" ]; then
    echo "DRIFT for [workspace] members: $mirror must list $crates" >&2
    status=1
fi

lint_tables() {
    awk '/^\[/ { active = ($0 ~ /^\[workspace\.lints\./) } active && NF { print }' "$1"
}
if [ "$(lint_tables "$root")" != "$(lint_tables "$mirror")" ]; then
    echo "DRIFT for workspace lints: update $mirror from $root" >&2
    status=1
fi

if [ "$status" -eq 0 ]; then
    echo "check-lang-workspace: $mirror matches $root"
fi
exit "$status"
