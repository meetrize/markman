#!/usr/bin/env bash
# Run Markman in development mode (debug build, fast incremental compile).
#
# Usage:
#   ./scripts/dev.sh [markman args...]
#
# Examples:
#   ./scripts/dev.sh
#   ./scripts/dev.sh test.md
#   ./scripts/dev.sh --help
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Starting $MARKMAN_DISPLAY_NAME (dev build)..."
cargo build
exec "$(dirname "${BASH_SOURCE[0]}")/launch-dev-binary.sh" "$@"
