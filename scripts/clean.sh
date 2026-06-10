#!/usr/bin/env bash
# Remove build artifacts and local dist output.
#
# Usage:
#   ./scripts/clean.sh
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Cleaning cargo target..."
cargo clean

if [[ -d "$VELOTYPE_PROJECT_ROOT/dist" ]]; then
    velotype_info "Removing dist/..."
    rm -rf "$VELOTYPE_PROJECT_ROOT/dist"
fi

velotype_info "Done."
