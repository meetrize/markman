#!/usr/bin/env bash
# Run Markman Criterion benchmarks.
#
# Usage:
#   ./scripts/bench.sh [bench name filter...]
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Running benchmarks..."
cargo bench "$@"
