#!/usr/bin/env bash
# Create a macOS .app bundle for Markman.
#
# Usage:
#   ./scripts/create_macos_app_dist.sh
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

DIST_DIR="$MARKMAN_PROJECT_ROOT/dist"

markman_info "Clean old dist output."
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

markman_info "Build release binary."
cargo build --release

markman_info "Create $MARKMAN_APP_NAME.app bundle."
APP_DIR="$DIST_DIR/$MARKMAN_APP_NAME.app"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"
cp "$MARKMAN_PROJECT_ROOT/target/release/$MARKMAN_BINARY_NAME" \
    "$APP_DIR/Contents/MacOS/$MARKMAN_BINARY_NAME"
cp "$MARKMAN_PROJECT_ROOT/resources/macos/Info.plist" "$APP_DIR/Contents/"
cp "$MARKMAN_PROJECT_ROOT/resources/macos/$MARKMAN_BINARY_NAME.icns" \
    "$APP_DIR/Contents/Resources/$MARKMAN_BINARY_NAME.icns"

if [[ -f "$MARKMAN_PROJECT_ROOT/README.md" ]]; then
    cp "$MARKMAN_PROJECT_ROOT/README.md" "$APP_DIR/Contents/Resources/"
fi

markman_info "Done: $APP_DIR"
echo "       Use: open '$APP_DIR'"
