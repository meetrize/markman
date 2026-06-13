#!/usr/bin/env bash
# Fast compile check without producing a binary.
#
# Usage:
#   ./scripts/check.sh
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Checking $MARKMAN_DISPLAY_NAME (cargo check)..."
cargo check
