#!/usr/bin/env bash
# Package Markman for the current platform.
#
# Usage:
#   ./scripts/package.sh [macos-app|macos-pkg <version>|linux|windows]
#
# Examples:
#   ./scripts/package.sh                  # auto-detect platform
#   ./scripts/package.sh macos-app
#   ./scripts/package.sh macos-pkg 0.5.7
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

usage() {
    cat <<EOF
Usage: $0 [target]

Targets:
  macos-app              Build release + create Markman.app
  macos-pkg <version>    Create PKG installer (requires existing .app in dist/)
  linux                  Build release + tarball with desktop entry
  windows                Build release + zip archive
  (default)              Pick target from current OS
EOF
}

package_macos_app() {
    "$VELOTYPE_PROJECT_ROOT/scripts/create_macos_app_dist.sh"
}

package_macos_pkg() {
    local version="${1:-}"
    if [[ -z "$version" ]]; then
        version="$(grep '^version' "$VELOTYPE_PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
        velotype_warn "No version given, using Cargo.toml version: $version"
    fi
    "$VELOTYPE_PROJECT_ROOT/scripts/create_macos_pkg_dist.sh" "$version"
}

package_linux() {
    cd "$VELOTYPE_PROJECT_ROOT"
    "$VELOTYPE_PROJECT_ROOT/scripts/build.sh" --locked

    local version tag archive package_dir
    version="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
    tag="v${version}"
    archive="markman-${tag}-linux-$(uname -m).tar.gz"
    package_dir="dist/package"

    rm -rf dist
    mkdir -p "$package_dir/share/applications"
    mkdir -p "$package_dir/share/icons/hicolor/256x256/apps"
    mkdir -p "$package_dir/share/icons/hicolor/512x512/apps"

    cp "target/release/$VELOTYPE_BINARY_NAME" "$package_dir/"
    cp README.md LICENSE-APACHE "$package_dir/"
    cp resources/linux/com.manyougz.Velotype.desktop "$package_dir/share/applications/"
    cp resources/linux/icons/hicolor/256x256/apps/com.manyougz.Velotype.png \
        "$package_dir/share/icons/hicolor/256x256/apps/"
    cp resources/linux/icons/hicolor/512x512/apps/com.manyougz.Velotype.png \
        "$package_dir/share/icons/hicolor/512x512/apps/"

    tar -C "$package_dir" -czf "dist/$archive" .
    velotype_info "Done: dist/$archive"
}

package_windows() {
    cd "$VELOTYPE_PROJECT_ROOT"
    "$VELOTYPE_PROJECT_ROOT/scripts/build.sh" --locked

    local version tag archive package_dir
    version="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
    tag="v${version}"
    archive="markman-${tag}-windows-$(uname -m).zip"
    package_dir="dist/package"

    rm -rf dist
    mkdir -p "$package_dir"

    cp "target/release/${VELOTYPE_BINARY_NAME}.exe" "$package_dir/"
    cp README.md LICENSE-APACHE "$package_dir/"

    if command -v powershell.exe >/dev/null 2>&1; then
        powershell.exe -NoProfile -Command \
            "Compress-Archive -Path 'dist/package/*' -DestinationPath 'dist/$archive' -Force"
    elif command -v zip >/dev/null 2>&1; then
        (cd dist/package && zip -r "../$archive" .)
    else
        velotype_die "Need powershell.exe or zip to create Windows archive"
    fi

    velotype_info "Done: dist/$archive"
}

TARGET="${1:-auto}"

case "$TARGET" in
    -h | --help | help)
        usage
        exit 0
        ;;
    macos-app)
        package_macos_app
        ;;
    macos-pkg)
        package_macos_pkg "${2:-}"
        ;;
    linux)
        package_linux
        ;;
    windows)
        package_windows
        ;;
    auto)
        case "$(uname -s)" in
            Darwin)
                package_macos_app
                ;;
            Linux)
                package_linux
                ;;
            MINGW* | MSYS* | CYGWIN*)
                package_windows
                ;;
            *)
                velotype_die "Unsupported platform: $(uname -s). Pass an explicit target."
                ;;
        esac
        ;;
    *)
        velotype_die "Unknown target: $TARGET"
        ;;
esac
