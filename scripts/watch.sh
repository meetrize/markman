#!/usr/bin/env bash
# Watch source files and auto-rebuild + restart Markman on changes.
#
# Requires cargo-watch: cargo install cargo-watch
#
# Usage:
#   ./scripts/watch.sh [markman args...]
#
# Examples:
#   ./scripts/watch.sh
#   ./scripts/watch.sh test.md
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

if ! command -v cargo-watch >/dev/null 2>&1; then
    markman_die "cargo-watch not found. Install with: cargo install cargo-watch"
fi

cd "$MARKMAN_PROJECT_ROOT"

markman_info "Watching sources; $MARKMAN_DISPLAY_NAME restarts on change (cargo watch)..."
markman_warn "GPUI desktop apps do not hot-reload UI state — the process restarts after each rebuild."

if (($# > 0)); then
    exec cargo watch \
        -w src \
        -w assets \
        -w resources \
        -w build.rs \
        -w Cargo.toml \
        -x build \
        -s "./scripts/launch-dev-binary.sh $*"
else
    exec cargo watch \
        -w src \
        -w assets \
        -w resources \
        -w build.rs \
        -w Cargo.toml \
        -x build \
        -s "./scripts/launch-dev-binary.sh"
fi
