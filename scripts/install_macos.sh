#!/usr/bin/env bash
# Build, package, and install Markman to /Applications on macOS.
#
# Usage:
#   ./scripts/install_macos.sh [options]
#
# Options:
#   --no-build    Skip compile; install existing dist/Markman.app
#   --no-cli      Do not create /usr/local/bin/markman symlink
#   --open        Launch Markman after installation
#   -h, --help    Show help
#
# Examples:
#   ./scripts/install_macos.sh
#   ./scripts/install_macos.sh --no-build --open
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

INSTALL_DIR="/Applications"
APP_BUNDLE="${MARKMAN_APP_NAME}.app"
SOURCE_APP="$MARKMAN_PROJECT_ROOT/dist/$APP_BUNDLE"
DEST_APP="$INSTALL_DIR/$APP_BUNDLE"
BINARY_PATH="$DEST_APP/Contents/MacOS/$MARKMAN_BINARY_NAME"
CLI_LINK="/usr/local/bin/$MARKMAN_BINARY_NAME"
LEGACY_CLI_LINK="/usr/local/bin/velotype"

DO_BUILD=1
DO_CLI=1
DO_OPEN=0

usage() {
    cat <<EOF
Usage: $0 [options]

Build a release .app bundle (unless --no-build) and install it to:
  $DEST_APP

Options:
  --no-build    Install dist/$APP_BUNDLE without rebuilding
  --no-cli      Skip /usr/local/bin/$MARKMAN_BINARY_NAME symlink
  --open        Open $MARKMAN_APP_NAME after installation
  -h, --help    Show this help

Related scripts:
  ./scripts/build.sh                 Release binary only
  ./scripts/create_macos_app_dist.sh Create dist/$APP_BUNDLE
  ./scripts/package.sh macos-pkg     Create PKG installer for distribution
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-build)
            DO_BUILD=0
            ;;
        --no-cli)
            DO_CLI=0
            ;;
        --open)
            DO_OPEN=1
            ;;
        -h | --help | help)
            usage
            exit 0
            ;;
        *)
            markman_die "Unknown option: $1 (try --help)"
            ;;
    esac
    shift
done

if [[ "$(uname -s)" != "Darwin" ]]; then
    markman_die "This script only supports macOS."
fi

if ((DO_BUILD)); then
    "$MARKMAN_PROJECT_ROOT/scripts/create_macos_app_dist.sh"
elif [[ ! -d "$SOURCE_APP" ]]; then
    markman_die "Missing $SOURCE_APP — run without --no-build or ./scripts/create_macos_app_dist.sh first"
fi

if [[ ! -x "$SOURCE_APP/Contents/MacOS/$MARKMAN_BINARY_NAME" ]]; then
    markman_die "Invalid app bundle: $SOURCE_APP"
fi

markman_info "Ad-hoc signing app bundle..."
xattr -cr "$SOURCE_APP" 2>/dev/null || true
codesign --force --deep --sign - "$SOURCE_APP" 2>&1 || {
    markman_warn "Code signing failed. Gatekeeper may block the installed app on first launch."
}

install_with_privilege() {
  local src="$1"
  local dest="$2"

  if [[ -e "$dest" ]]; then
    markman_info "Removing existing installation: $dest"
    rm -rf "$dest"
  fi

  markman_info "Installing to $dest"
  ditto "$src" "$dest"
}

if [[ -w "$INSTALL_DIR" ]]; then
  install_with_privilege "$SOURCE_APP" "$DEST_APP"
else
  markman_info "Requesting administrator privileges to write to $INSTALL_DIR"
  if [[ -e "$DEST_APP" ]]; then
    sudo rm -rf "$DEST_APP"
  fi
  markman_info "Installing to $DEST_APP"
  sudo ditto "$SOURCE_APP" "$DEST_APP"
fi

if ((DO_CLI)); then
  markman_info "Installing CLI symlink at $CLI_LINK"
  if [[ -w "$(dirname "$CLI_LINK")" ]]; then
    rm -f "$CLI_LINK" "$LEGACY_CLI_LINK"
    ln -sf "$BINARY_PATH" "$CLI_LINK"
  else
    sudo rm -f "$CLI_LINK" "$LEGACY_CLI_LINK"
    sudo ln -sf "$BINARY_PATH" "$CLI_LINK"
  fi
fi

markman_info "Installed: $DEST_APP"
if ((DO_CLI)) && [[ -L "$CLI_LINK" ]]; then
  echo "       CLI: $CLI_LINK -> $(readlink "$CLI_LINK")"
fi

if ((DO_OPEN)); then
  markman_info "Launching $MARKMAN_APP_NAME"
  open "$DEST_APP"
fi
