#!/usr/bin/env bash
# Remove Cargo build artifacts and local dist/ output.
#
# Usage:
#   ./scripts/clean.sh
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Cleaning cargo target..."
cargo clean

if [[ -d "$MARKMAN_PROJECT_ROOT/dist" ]]; then
    markman_info "Removing dist/..."
    rm -rf "$MARKMAN_PROJECT_ROOT/dist"
fi

markman_info "Done."
