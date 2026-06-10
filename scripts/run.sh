#!/usr/bin/env bash
# Run the release binary. Builds first if the artifact is missing.
#
# Usage:
#   ./scripts/run.sh [velotype args...]
#
# Examples:
#   ./scripts/run.sh
#   ./scripts/run.sh test.md
#   ./scripts/run.sh --detach
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

BINARY="$(velotype_binary_path release)"

if [[ ! -x "$BINARY" ]]; then
    velotype_warn "Release binary not found, building first..."
    "$VELOTYPE_PROJECT_ROOT/scripts/build.sh"
fi

velotype_info "Running $BINARY"
exec "$BINARY" "$@"
