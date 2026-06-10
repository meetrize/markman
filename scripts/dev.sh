#!/usr/bin/env bash
# Run Velotype in development mode (debug build, fast incremental compile).
#
# Usage:
#   ./scripts/dev.sh [cargo run args...]
#
# Examples:
#   ./scripts/dev.sh
#   ./scripts/dev.sh test.md
#   ./scripts/dev.sh -- --help
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Starting Velotype (dev / cargo run)..."
exec cargo run -- "$@"
