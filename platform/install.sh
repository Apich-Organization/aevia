#!/bin/sh
# Bootstrap a verified Aevia toolchain on Linux or macOS.
set -eu

usage() {
    cat <<'HELP'
Install an Aevia toolchain from an immutable GitHub release.

Usage: install.sh [--channel stable|dev|nightly | --version VERSION]
                  [--home DIRECTORY] [--no-path] [--no-activate]

The default channel is stable. dev and nightly select the newest published
daily build for this machine. VERSION can be a SemVer release (with or without
the v prefix), a prerelease such as 0.1.0-beta.1, or nightly-YYYY-MM-DD.

Downloads over HTTPS. Release metadata is read with python3, or with the
GitHub CLI when python3 is absent; with 'gh' installed the release attestation
is verified in addition to the SHA-256 checksum. AEVIA_HOME overrides the
default installation directory (~/.aevia).
--no-path skips shell PATH and completion changes.
--no-activate keeps the active toolchain and shell configuration as they are.
HELP
}

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

selector=
home_dir=${AEVIA_HOME:-${AEVIA_INSTALL_DIR:-}}
configure_shell=yes
activate=yes
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version=*|--channel=*)
            [ -z "$selector" ] || fail "specify one --channel or --version"
            selector=${1%%=*}
            selector=${selector#--}
            requested=${1#*=}
            [ -n "$requested" ] || fail "$selector requires a value"
            shift
            ;;
        --channel|--version)
            [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 requires a value"
            [ -z "$selector" ] || fail "specify one --channel or --version"
            selector=${1#--}
            requested=$2
            shift 2
            ;;
        --home)
            [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 requires a directory"
            home_dir=$2
            shift 2
            ;;
        --no-path) configure_shell=no; shift ;;
        --no-activate) activate=no; shift ;;
        --help|-h) usage; exit 0 ;;
        *) fail "unknown argument: $1 (see --help)" ;;
    esac
done

core='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
nightly='nightly-[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])'
if [ "${selector:-channel}" = version ]; then
    case "$requested" in
        ''|*[!A-Za-z0-9.-]*) fail "invalid release version: $requested" ;;
    esac
    if printf '%s\n' "$requested" | LC_ALL=C grep -Eq "^$nightly\$"; then
        tag=$requested
    else
        requested=${requested#v}
        # Numeric prerelease identifiers cannot have leading zeroes.
        id='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
        [ "${#requested}" -le 80 ] &&
            printf '%s\n' "$requested" | LC_ALL=C grep -Eq "^$core(-$id(\.$id)*)?\$" ||
            fail "invalid release version: $requested"
        tag=v$requested
    fi
    channel=exact
else
    case "${requested:-stable}" in
        stable) channel=stable ;;
        dev|nightly) channel=nightly ;;
        *) fail "unknown channel: $requested (expected stable, dev, or nightly)" ;;
    esac
fi

repo=${AEVIA_GITHUB_REPO:-Apich-Organization/aevia}
case "$repo" in
    ''|*[!A-Za-z0-9_./-]*) fail "AEVIA_GITHUB_REPO must be OWNER/REPO" ;;
esac
printf '%s\n' "$repo" | LC_ALL=C grep -Eq '^[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9_][A-Za-z0-9_.-]*$' ||
    fail "AEVIA_GITHUB_REPO must be OWNER/REPO"
export GH_PROMPT_DISABLED=1
# GitHub CLI is optional. The release metadata comes from the public REST API
# (through python3 when it is present, otherwise through `gh api`), and the
# archive from a plain HTTPS download. `gh`, when installed, adds attestation
# verification on top of the SHA-256 checksum.
have_gh=no
if command -v gh >/dev/null 2>&1 && gh release verify-asset --help >/dev/null 2>&1; then
    have_gh=yes
fi
have_python=no
if command -v python3 >/dev/null 2>&1; then
    have_python=yes
fi
have_curl=no
if command -v curl >/dev/null 2>&1; then
    have_curl=yes
fi
have_wget=no
if command -v wget >/dev/null 2>&1; then
    have_wget=yes
fi
if [ "$have_curl" = no ] && [ "$have_wget" = no ] && [ "$have_gh" = no ]; then
    fail "need curl, wget or the GitHub CLI to download the release"
fi

cpu=$(uname -m)
case "$cpu" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) fail "unsupported architecture: $cpu" ;;
esac
os=$(uname -s)
case "$os" in
    Linux) target=$arch-unknown-linux-gnu ;;
    Darwin) target=$arch-apple-darwin ;;
    *) fail "unsupported system: $os (use install.ps1 on Windows)" ;;
esac
suffix=-$target.tar.gz

# Prints the newest tag for the channel, one per line, newest last.
release_tags() {
    channel=$1
    if [ "$have_python" = yes ]; then
        python3 - "$repo" "$channel" "$suffix" <<'PY'
import json
import os
import re
import sys
import urllib.error
import urllib.request

repo, channel, suffix = sys.argv[1], sys.argv[2], sys.argv[3]
nightly = re.compile(r"^nightly-\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$")
token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")

def fetch(url):
    request = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json"})
    if token:
        request.add_header("Authorization", "Bearer " + token)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)

base = "https://api.github.com/repos/" + repo
try:
    if channel == "stable":
        items = [fetch(base + "/releases/latest")]
    else:
        items = []
        for page in (1, 2, 3):
            batch = fetch(base + "/releases?per_page=100&page=" + str(page))
            if not batch:
                break
            items.extend(batch)
except (urllib.error.URLError, ValueError) as error:
    print("release lookup failed: " + str(error), file=sys.stderr)
    sys.exit(1)

tags = []
for item in items:
    if item.get("draft") or not item.get("immutable"):
        continue
    tag = item.get("tag_name") or ""
    if channel == "stable":
        if item.get("prerelease") or not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
            continue
        tags.append(tag)
    else:
        if not item.get("prerelease") or not nightly.match(tag):
            continue
        archive = "aevia-" + tag + suffix
        names = {a.get("name") for a in item.get("assets", []) if a.get("state") == "uploaded"}
        if archive in names and archive + ".sha256" in names:
            tags.append(tag)
print("\n".join(sorted(tags)))
PY
        return $?
    fi
    if [ "$have_gh" = yes ]; then
        if [ "$channel" = stable ]; then
            gh api --hostname github.com "repos/$repo/releases/latest" \
                --jq 'select(.draft == false and .prerelease == false and .immutable == true) | .tag_name'
        else
            gh api --hostname github.com --paginate "repos/$repo/releases?per_page=100" \
                --jq '.[] | select(.draft == false and .prerelease == true and .immutable == true) | select(.tag_name | test("^'"$nightly"'$")) | ("aevia-" + .tag_name + "'"$suffix"'") as $archive | select(any(.assets[]; .name == $archive and .state == "uploaded")) | select(any(.assets[]; .name == ($archive + ".sha256") and .state == "uploaded")) | .tag_name'
        fi
        return $?
    fi
    printf 'release lookup needs python3 or GitHub CLI\n' >&2
    return 1
}

if [ "$channel" = stable ]; then
    tags=$(release_tags stable) ||
        fail "could not look up the latest stable release; pass --version instead"
    tag=$(printf '%s\n' "$tags" | LC_ALL=C grep -E "^v$core\$" | LC_ALL=C sort | tail -n 1)
    [ -n "$tag" ] ||
        fail "no immutable stable release found; use --version or --channel nightly"
elif [ "$channel" = nightly ]; then
    tags=$(release_tags nightly) ||
        fail "could not look up nightly releases; pass --version instead"
    tag=$(printf '%s\n' "$tags" | LC_ALL=C grep -E "^$nightly\$" | LC_ALL=C sort | tail -n 1)
    [ -n "$tag" ] || fail "no immutable nightly release with an archive for $target"
fi

[ -n "$home_dir" ] || home_dir=${HOME:?HOME is required unless --home is supplied}/.aevia
case "$home_dir" in
    /*) ;;
    *) home_dir=$(pwd -P)/$home_dir ;;
esac
# Colons are PATH separators, so an installation there cannot configure PATH.
case "$home_dir" in
    *:*|*'
'*) [ "$configure_shell" = no ] || [ "$activate" = no ] || fail "use --no-path with a colon or newline in the installation path" ;;
esac

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' 0
trap 'exit 1' 1 2 15
archive=aevia-${tag#v}$suffix
printf 'Downloading Aevia %s (%s)\n' "$tag" "$target"
download_asset() {
    name=$1
    if [ "$have_curl" = yes ]; then
        curl -fsSL --retry 3 --retry-delay 2 -o "$tmp_dir/$name" \
            "https://github.com/$repo/releases/download/$tag/$name"
    else
        gh release download "$tag" --repo "github.com/$repo" --pattern "$name" --dir "$tmp_dir"
    fi
}
download_asset "$archive" && download_asset "$archive.sha256" ||
    fail "could not download $archive and its checksum from $repo/$tag"

expected=$(LC_ALL=C awk -v name="$archive" '
    { sub(/\r$/, "") }
    NR == 1 && NF == 2 && length($1) == 64 && $1 ~ /^[0-9a-fA-F]+$/ &&
        ($2 == name || $2 == "*" name) { digest = tolower($1) }
    END { if (NR != 1 || !digest) exit 1; print digest }
' "$tmp_dir/$archive.sha256") || fail "invalid checksum for $archive"
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum < "$tmp_dir/$archive")
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 < "$tmp_dir/$archive")
else
    fail "required command not found: sha256sum or shasum"
fi
[ "${actual%% *}" = "$expected" ] || fail "SHA-256 mismatch for $archive"
if [ "$have_gh" = yes ]; then
    gh release verify-asset "$tag" "$tmp_dir/$archive" --repo "github.com/$repo" ||
        fail "GitHub release attestation verification failed for $archive"
else
    printf 'note: checksum verified; install GitHub CLI to also verify the release attestation\n' >&2
fi

mkdir "$tmp_dir/unpacked"
tar -xzf "$tmp_dir/$archive" -C "$tmp_dir/unpacked"
toolchain=$tmp_dir/unpacked/${archive%.tar.gz}
[ -d "$toolchain" ] && [ ! -L "$toolchain" ] || fail "missing archive directory: ${archive%.tar.gz}"
for executable in aevia aevia-lsp aevia-pkg; do
    file=$toolchain/$executable
    [ -f "$file" ] && [ ! -L "$file" ] && [ -s "$file" ] && [ -x "$file" ] ||
        fail "invalid or missing executable: $executable"
done
set -- "$tag" "$toolchain" --home "$home_dir" --channel "$channel"
if [ "$activate" = no ]; then
    set -- "$@" --no-activate
fi
"$toolchain/aevia" toolchain register "$@"

quote_sh() {
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

add_profile_line() {
    if ! grep -Fqx -e "$2" "$1" 2>/dev/null; then
        mkdir -p "$(dirname "$1")"
        printf '\n%s\n' "$2" >> "$1"
    fi
}

save_completion() {
    # Write in the destination directory so that an old working completion is
    # never replaced by an unsuccessful generation, including on another drive.
    mkdir -p "$(dirname "$2")" || return
    completion=$(mktemp "$(dirname "$2")/.aevia-completion.XXXXXX") || return
    if "$toolchain/aevia" completions "$1" > "$completion" && [ -s "$completion" ]; then
        mv -f "$completion" "$2"
    else
        rm -f "$completion"
        return 1
    fi
}

configure_unix_shells() {
    bin=$home_dir/bin
    path_line='case ":${PATH}:" in *'$(quote_sh ":$bin:")'*) ;; *) export PATH='$(quote_sh "$bin")':"$PATH";; esac'
    add_profile_line "$HOME/.profile" "$path_line"

    if [ -f "$HOME/.bashrc" ] || [ "${SHELL##*/}" = bash ]; then
        bash_completion=$home_dir/completions/aevia.bash
        if save_completion bash "$bash_completion"; then
            quoted_completion=$(quote_sh "$bash_completion")
            load_bash='case "$-" in *i*) if [ -r '$quoted_completion' ]; then . '$quoted_completion'; fi;; esac'
            add_profile_line "$HOME/.bashrc" "$load_bash"
            for bash_profile in "$HOME/.bash_profile" "$HOME/.bash_login"; do
                if [ -f "$bash_profile" ]; then
                    add_profile_line "$bash_profile" "$load_bash"
                fi
            done
        fi
        add_profile_line "$HOME/.bashrc" "$path_line"
    fi
    for bash_profile in "$HOME/.bash_profile" "$HOME/.bash_login"; do
        if [ -f "$bash_profile" ]; then
            add_profile_line "$bash_profile" "$path_line"
        fi
    done

    zsh_profile=${ZDOTDIR:-$HOME}/.zshrc
    if [ -f "$zsh_profile" ] || [ "${SHELL##*/}" = zsh ]; then
        add_profile_line "$zsh_profile" "$path_line"
        zsh_completions=$home_dir/completions/zsh
        if save_completion zsh "$zsh_completions/_aevia"; then
            add_profile_line "$zsh_profile" 'fpath=('$(quote_sh "$zsh_completions")' "$fpath[@]"); autoload -Uz _aevia; if (( $+functions[compdef] )); then compdef _aevia aevia; fi'
        fi
    fi

    fish_root=${XDG_CONFIG_HOME:-$HOME/.config}/fish
    if [ -d "$fish_root" ] || [ "${SHELL##*/}" = fish ]; then
        fish_bin=$(printf '%s' "$bin" | sed "s/\\\\/\\\\\\\\/g; s/'/\\\\'/g")
        add_profile_line "$fish_root/config.fish" "fish_add_path '$fish_bin'"
        if ! save_completion fish "$fish_root/completions/aevia.fish"; then
            printf '%s\n' 'warning: could not install Fish completions' >&2
        fi
    fi
}

if [ "$configure_shell" = yes ] && [ "$activate" = yes ]; then
    # SHELL can be unset in a provisioning environment.
    SHELL=${SHELL:-}
    configure_unix_shells
    printf 'Restart your shell, or run: export PATH=%s:"$PATH"\n' "$(quote_sh "$home_dir/bin")"
fi
