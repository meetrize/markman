#!/usr/bin/env bash
# Shared helpers for Velotype build scripts.
set -euo pipefail

VELOTYPE_PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VELOTYPE_BINARY_NAME="velotype"
VELOTYPE_APP_NAME="Velotype"

velotype_binary_path() {
    local profile="${1:-release}"
    local dir="$VELOTYPE_PROJECT_ROOT/target/$profile"
    local unix_path="$dir/$VELOTYPE_BINARY_NAME"
    local windows_path="$dir/${VELOTYPE_BINARY_NAME}.exe"

    if [[ -x "$windows_path" ]]; then
        echo "$windows_path"
    elif [[ -x "$unix_path" ]]; then
        echo "$unix_path"
    elif [[ -f "$windows_path" ]]; then
        echo "$windows_path"
    else
        echo "$unix_path"
    fi
}

velotype_info() {
    echo "==> $*"
}

velotype_warn() {
    echo "==> ⚠️  $*" >&2
}

velotype_die() {
    echo "==> ❌ $*" >&2
    exit 1
}
