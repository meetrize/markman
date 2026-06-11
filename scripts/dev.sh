#!/usr/bin/env bash
# Run Velotype in development mode (debug build, fast incremental compile).
#
# Usage:
#   ./scripts/dev.sh [velotype args...]
#
# Examples:
#   ./scripts/dev.sh
#   ./scripts/dev.sh test.md
#   ./scripts/dev.sh --help
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Starting $VELOTYPE_DISPLAY_NAME (dev build)..."
cargo build
exec "$(dirname "${BASH_SOURCE[0]}")/launch-dev-binary.sh" "$@"
