#!/usr/bin/env bash
# Fast compile check without producing a binary.
#
# Usage:
#   ./scripts/check.sh
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$VELOTYPE_PROJECT_ROOT"

velotype_info "Checking Velotype (cargo check)..."
cargo check
