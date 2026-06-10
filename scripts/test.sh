#!/usr/bin/env bash
# Run unit and integration tests.
#
# Usage:
#   ./scripts/test.sh [cargo test args...]
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Running tests..."
exec cargo test "$@"
