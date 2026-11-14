#!/bin/sh
# Regenerate both sides of the reviewed kernel/language API contract.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
kernel_root=$(CDPATH= cd -- "$root/../rssn" && pwd)
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"
for family in contracts structured bindings codecs; do
    cargo run --manifest-path "$kernel_root/tools/rssn-api-gen/Cargo.toml" -- "--$family-write"
done
cd "$root"
cargo run -p rssn-api-audit -- --write
cargo run -p aevia-stdlib-gen -- --write
AEVIA_BLESS=1 cargo test -p aevia-rssn --test domains
