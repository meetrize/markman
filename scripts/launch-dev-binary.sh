#!/usr/bin/env bash
# Launch the debug build with the macOS display name (Markman).
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

velotype_launch_binary "$(velotype_binary_path debug)" "$@"
