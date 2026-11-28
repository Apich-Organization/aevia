#!/bin/sh
set -eu

if [ "$#" -ne 2 ] || [ ! -f "$1" ]; then
    printf 'Usage: sh %s <Linux aevia executable> <local image tag>\n' "$0" >&2
    exit 2
fi

infra=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
context=$(mktemp -d)
trap 'rm -rf -- "$context"' EXIT
trap 'exit 1' HUP INT TERM
install -m 0755 -- "$1" "$context/aevia"
podman build --file "$infra/Containerfile.playground" --tag "$2" "$context"
