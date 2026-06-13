#!/usr/bin/env bash
# Run Markman unit and integration tests.
#
# Usage:
#   ./scripts/test.sh [cargo test args...]
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Running tests..."
cargo test "$@"
