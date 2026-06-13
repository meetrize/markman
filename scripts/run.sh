#!/usr/bin/env bash
# Run the Markman release binary.
#
# Usage:
#   ./scripts/run.sh [markman args...]
#
# Examples:
#   ./scripts/run.sh
#   ./scripts/run.sh test.md
#   ./scripts/run.sh --help
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$MARKMAN_PROJECT_ROOT"

BINARY="$(markman_binary_path release)"

if [[ ! -x "$BINARY" && ! -f "$BINARY" ]]; then
    markman_warn "Release binary not found, building first..."
    "$MARKMAN_PROJECT_ROOT/scripts/build.sh"
    BINARY="$(markman_binary_path release)"
fi

markman_info "Running $BINARY"
markman_launch_binary "$BINARY" "$@"
