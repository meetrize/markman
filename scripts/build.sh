#!/usr/bin/env bash
# Build Markman release binary.
#
# Usage:
#   ./scripts/build.sh [--locked]
#
# Output:
#   target/release/markman (or markman.exe on Windows)
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

LOCKED=()
if [[ "${1:-}" == "--locked" ]]; then
    LOCKED=(--locked)
fi

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Building $MARKMAN_DISPLAY_NAME (release)..."
if ((${#LOCKED[@]})); then
    cargo build --release "${LOCKED[@]}"
else
    cargo build --release
fi

BINARY="$(markman_binary_path release)"
markman_info "Done: $BINARY"
