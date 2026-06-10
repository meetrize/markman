#!/usr/bin/env bash
# Run Criterion benchmarks.
#
# Usage:
#   ./scripts/bench.sh [bench name or cargo bench args...]
#
# Examples:
#   ./scripts/bench.sh
#   ./scripts/bench.sh render_loop
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Running benchmarks..."
exec cargo bench "$@"
