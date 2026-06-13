#!/usr/bin/env bash
# Launch the debug Markman binary (used by dev.sh and watch.sh).
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

markman_launch_binary "$(markman_binary_path debug)" "$@"
