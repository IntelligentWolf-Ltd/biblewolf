#!/usr/bin/env bash
# Build (if needed) and run BibleWolf from this checkout.
#   ./run.sh          optimised release build (recommended)
#   ./run.sh --debug  faster to compile, slower to run
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust is not installed. Get it from https://rustup.rs and try again." >&2
    exit 1
fi

if [[ "${1:-}" == "--debug" ]]; then
    shift
    cargo run -- "$@"
else
    cargo run --release -- "$@"
fi
