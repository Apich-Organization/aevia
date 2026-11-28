#!/bin/sh
# Compatibility launcher for the Bazel development stack.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
exec bazel run //:platform_dev -- "$@"
