#!/usr/bin/env bash
# Shared helpers for Markman build scripts.
set -euo pipefail

MARKMAN_PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MARKMAN_BINARY_NAME="markman"
MARKMAN_APP_NAME="Markman"
# Must stay in sync with `APP_DISPLAY_NAME` in `src/app_identity.rs`.
MARKMAN_DISPLAY_NAME="Markman"

markman_binary_path() {
    local profile="${1:-release}"
    local dir="$MARKMAN_PROJECT_ROOT/target/$profile"
    local unix_path="$dir/$MARKMAN_BINARY_NAME"
    local windows_path="$dir/${MARKMAN_BINARY_NAME}.exe"

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

markman_info() {
    echo "==> $*"
}

markman_warn() {
    echo "==> ⚠️  $*" >&2
}

markman_die() {
    echo "==> ❌ $*" >&2
    exit 1
}

# Launch a built binary. On macOS, run via a `Markman` symlink so the process
# name matches the display name when not running from a .app bundle.
markman_launch_binary() {
    local binary="$1"
    shift

    if [[ "$(uname -s)" == "Darwin" ]]; then
        local dir real_binary display_binary
        local binary_key display_key
        dir="$(cd "$(dirname "$binary")" && pwd)"
        real_binary="$dir/$MARKMAN_BINARY_NAME"
        if [[ ! -x "$real_binary" ]]; then
            markman_die "Binary not found: $real_binary"
        fi

        binary_key="$(printf '%s' "$MARKMAN_BINARY_NAME" | tr '[:upper:]' '[:lower:]')"
        display_key="$(printf '%s' "$MARKMAN_DISPLAY_NAME" | tr '[:upper:]' '[:lower:]')"
        if [[ "$binary_key" == "$display_key" ]]; then
            # Case-insensitive volumes treat Markman/markman as one path — skip symlinks.
            exec "$real_binary" "$@"
        fi

        display_binary="$dir/$MARKMAN_DISPLAY_NAME"
        rm -f "$display_binary"
        ln -sf "$MARKMAN_BINARY_NAME" "$display_binary"
        exec "$display_binary" "$@"
    else
        exec "$binary" "$@"
    fi
}
