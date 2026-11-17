#!/bin/sh
# Aevia toolchain installer for macOS and Linux.
# Usage:
#   curl -fsSL https://aevia.apich.org/install.sh | sh
#   curl -fsSL https://aevia.apich.org/install.sh | sh -s -- --version 0.1.0

set -eu

REPO="${AEVIA_GITHUB_REPO:-Apich-Organization/aevia}"
INSTALL_DIR="${AEVIA_INSTALL_DIR:-${HOME}/.aevia}"
BIN_DIR="${INSTALL_DIR}/bin"
VERSION=""

usage() {
    cat <<'EOF'
Install the Aevia toolchain (aevia, aevia-lsp).

Usage:
  install.sh [--version <version>] [--help]

Options:
  --version <ver>   Install a specific release version (e.g. 0.1.0 or v0.1.0).
                    Defaults to the latest GitHub release.
  -h, --help        Show this help message.

Environment:
  AEVIA_INSTALL_DIR   Installation prefix (default: ~/.aevia)
  AEVIA_GITHUB_REPO   GitHub repository (default: Apich-Organization/aevia)
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            if [ "$#" -lt 2 ]; then
                echo "error: --version requires a value" >&2
                exit 1
            fi
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#--version=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

say() {
    printf '%s\n' "$*"
}

err() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "required command not found: $1"
    fi
}

fetch() {
    url="$1"
    dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -H "User-Agent: aevia-installer" "$url" -o "$dest"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$dest" --header="User-Agent: aevia-installer" "$url"
    else
        err "either curl or wget is required to download Aevia"
    fi
}

fetch_stdout() {
    url="$1"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -H "User-Agent: aevia-installer" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- --header="User-Agent: aevia-installer" "$url"
    else
        err "either curl or wget is required to download Aevia"
    fi
}

detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) os_name="linux" ;;
        Darwin) os_name="macos" ;;
        *) err "unsupported operating system: $os (use install.ps1 on Windows)" ;;
    esac

    case "$arch" in
        x86_64|amd64) arch_name="x86_64" ;;
        aarch64|arm64) arch_name="aarch64" ;;
        *) err "unsupported architecture: $arch" ;;
    esac

    printf '%s-%s' "$os_name" "$arch_name"
}

resolve_tag() {
    if [ -n "$VERSION" ]; then
        case "$VERSION" in
            v*) printf '%s' "$VERSION" ;;
            *) printf 'v%s' "$VERSION" ;;
        esac
        return
    fi

    api_url="https://api.github.com/repos/${REPO}/releases/latest"
    json="$(fetch_stdout "$api_url")" || err "failed to query latest release from ${api_url}"
    tag="$(printf '%s' "$json" | tr '\n' ' ' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    if [ -z "$tag" ]; then
        err "could not determine latest release tag from GitHub API"
    fi
    printf '%s' "$tag"
}

add_to_path_file() {
    profile="$1"
    line="$2"
    if [ -f "$profile" ] && grep -F "$BIN_DIR" "$profile" >/dev/null 2>&1; then
        return
    fi
    mkdir -p "$(dirname "$profile")"
    printf '\n# Added by Aevia installer\n%s\n' "$line" >> "$profile"
}

configure_path() {
    case ":${PATH}:" in
        *":${BIN_DIR}:"*)
            return
            ;;
    esac

    export_line="export PATH=\"${BIN_DIR}:\$PATH\""
    updated=0

    for rc in "${HOME}/.zshrc" "${HOME}/.bashrc" "${HOME}/.bash_profile"; do
        if [ -f "$rc" ]; then
            add_to_path_file "$rc" "$export_line"
            updated=1
        fi
    done

    if [ "$updated" -eq 0 ]; then
        add_to_path_file "${HOME}/.profile" "$export_line"
    fi

    if [ -d "${HOME}/.config/fish" ]; then
        add_to_path_file "${HOME}/.config/fish/config.fish" "fish_add_path \"${BIN_DIR}\""
    fi
}

install_completions() {
    aevia_bin="${BIN_DIR}/aevia"
    if [ ! -x "$aevia_bin" ]; then
        return
    fi

    if [ -n "${BASH_VERSION:-}" ] || [ -f "${HOME}/.bashrc" ]; then
        comp_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/bash-completion/completions"
        if mkdir -p "$comp_dir" 2>/dev/null; then
            "$aevia_bin" completions bash > "${comp_dir}/aevia" 2>/dev/null || rm -f "${comp_dir}/aevia"
        fi
    fi

    if [ -n "${ZSH_VERSION:-}" ] || [ -f "${HOME}/.zshrc" ]; then
        comp_dir="${HOME}/.zfunc"
        if mkdir -p "$comp_dir" 2>/dev/null; then
            "$aevia_bin" completions zsh > "${comp_dir}/_aevia" 2>/dev/null || rm -f "${comp_dir}/_aevia"
        fi
    fi

    if [ -d "${HOME}/.config/fish" ]; then
        comp_dir="${HOME}/.config/fish/completions"
        if mkdir -p "$comp_dir" 2>/dev/null; then
            "$aevia_bin" completions fish > "${comp_dir}/aevia.fish" 2>/dev/null || rm -f "${comp_dir}/aevia.fish"
        fi
    fi
}

main() {
    need_cmd tar
    need_cmd uname

    target="$(detect_target)"
    tag="$(resolve_tag)"
    ver="${tag#v}"

    tmp_dir="$(mktemp -d 2>/dev/null || mktemp -d -t aevia-install)"
    trap 'rm -rf "$tmp_dir"' EXIT INT TERM

    archive_name="aevia-${ver}-${target}.tar.gz"
    archive_url="https://github.com/${REPO}/releases/download/${tag}/${archive_name}"
    archive_path="${tmp_dir}/aevia.tar.gz"

    say "Downloading Aevia ${tag} (${target})..."
    if ! fetch "$archive_url" "$archive_path" 2>/dev/null; then
        # Fallback for macos-arm64 release asset naming if macos-aarch64 is absent.
        if [ "$target" = "macos-aarch64" ]; then
            fallback_url="https://github.com/${REPO}/releases/download/${tag}/aevia-${ver}-macos-arm64.tar.gz"
            fetch "$fallback_url" "$archive_path" || err "failed to download release archive from ${archive_url}"
        else
            err "failed to download release archive from ${archive_url}"
        fi
    fi

    extract_dir="${tmp_dir}/extract"
    mkdir -p "$extract_dir"
    tar -xzf "$archive_path" -C "$extract_dir"

    mkdir -p "$BIN_DIR"
    installed=0
    for bin_name in aevia aevia-lsp aevia-pkg; do
        found="$(find "$extract_dir" -type f -name "$bin_name" | head -n 1 || true)"
        if [ -n "$found" ]; then
            cp "$found" "${BIN_DIR}/${bin_name}"
            chmod +x "${BIN_DIR}/${bin_name}"
            installed=$((installed + 1))
        fi
    done

    if [ "$installed" -eq 0 ]; then
        err "no aevia binaries found in downloaded archive"
    fi

    configure_path
    install_completions

    say "Installed Aevia ${tag} to ${BIN_DIR}"
    case ":${PATH}:" in
        *":${BIN_DIR}:"*) ;;
        *) say "Restart your shell or run: export PATH=\"${BIN_DIR}:\$PATH\"" ;;
    esac
    say "Run 'aevia --version' to verify."
}

main
