#!/usr/bin/env bash
# Build Velotype release binary.
#
# Usage:
#   ./scripts/build.sh [--locked]
#
# Output:
#   target/release/velotype (or velotype.exe on Windows)
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

LOCKED=()
if [[ "${1:-}" == "--locked" ]]; then
    LOCKED=(--locked)
fi

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Building Velotype (release)..."
cargo build --release "${LOCKED[@]}"

BINARY="$(velotype_binary_path release)"
velotype_info "Done: $BINARY"
