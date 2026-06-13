#!/usr/bin/env bash
# Create a macOS PKG installer for Markman.
#
# Usage:
#   ./scripts/create_macos_pkg_dist.sh <version>
#
# Example:
#   ./scripts/create_macos_pkg_dist.sh 0.5.7
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

if [[ $# -eq 0 ]]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.5.7"
    exit 1
fi

VERSION="$1"

DIST_DIR="$MARKMAN_PROJECT_ROOT/dist"
RESOURCES_DIR="$MARKMAN_PROJECT_ROOT/resources/macos"
APP_BUNDLE="${MARKMAN_APP_NAME}.app"
BUNDLE_ID="com.manyougz.Markman"

PKG_DIR="$DIST_DIR/pkg"
PKG_NAME="${MARKMAN_APP_NAME}-${VERSION}.pkg"
COMPONENT_PKG="${MARKMAN_APP_NAME}-component.pkg"
CLI_LINK="/usr/local/bin/${MARKMAN_BINARY_NAME}"

if [[ ! -d "$DIST_DIR/$APP_BUNDLE" ]]; then
    markman_die "$APP_BUNDLE not found at $DIST_DIR — run ./scripts/create_macos_app_dist.sh first"
fi

if [[ ! -f "$RESOURCES_DIR/pkg/Distribution.xml" ]]; then
    markman_die "Distribution.xml not found at $RESOURCES_DIR/pkg/"
fi

if [[ ! -f "$RESOURCES_DIR/pkg/postinstall" ]]; then
    markman_die "postinstall script not found at $RESOURCES_DIR/pkg/"
fi

markman_info "Creating PKG installer for $MARKMAN_APP_NAME $VERSION"
echo "    Bundle ID: $BUNDLE_ID"
echo "    Install location: /Applications"
echo "    CLI tool: $CLI_LINK"

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/root/Applications"
mkdir -p "$PKG_DIR/scripts"

markman_info "Preparing installation payload..."
cp -R "$DIST_DIR/$APP_BUNDLE" "$PKG_DIR/root/Applications/"

markman_info "Copying installation scripts..."
cp "$RESOURCES_DIR/pkg/postinstall" "$PKG_DIR/scripts/"
cp "$RESOURCES_DIR/pkg/preuninstall" "$PKG_DIR/scripts/"
chmod +x "$PKG_DIR/scripts/"*

markman_info "Signing app bundle..."
xattr -cr "$PKG_DIR/root/Applications/$APP_BUNDLE" 2>/dev/null || true
codesign --force --deep --sign - "$PKG_DIR/root/Applications/$APP_BUNDLE" 2>&1 || {
    markman_warn "Code signing failed. PKG installation may require manual approval."
}

markman_info "Creating component package..."
pkgbuild --identifier "$BUNDLE_ID" \
    --version "$VERSION" \
    --scripts "$PKG_DIR/scripts" \
    --root "$PKG_DIR/root" \
    --install-location "/" \
    "$PKG_DIR/$COMPONENT_PKG"

markman_info "Creating distribution package..."
cp "$RESOURCES_DIR/pkg/Distribution.xml" "$PKG_DIR/"
sed -i '' "s/__MARKMAN_VERSION__/${VERSION}/g" "$PKG_DIR/Distribution.xml"

productbuild --distribution "$PKG_DIR/Distribution.xml" \
    --package-path "$PKG_DIR" \
    "$DIST_DIR/$PKG_NAME"

markman_info "Fixing package metadata..."
pkgutil --expand "$DIST_DIR/$PKG_NAME" "$PKG_DIR/expanded" || true
if [[ -f "$PKG_DIR/expanded/$COMPONENT_PKG/PackageInfo" ]]; then
    sed -i '' '/<relocate>/,/<\/relocate>/d' "$PKG_DIR/expanded/$COMPONENT_PKG/PackageInfo"
    pkgutil --flatten "$PKG_DIR/expanded" "$DIST_DIR/$PKG_NAME"
    rm -rf "$PKG_DIR/expanded"
fi

markman_info "Done: $DIST_DIR/$PKG_NAME"
echo "      Size: $(du -h "$DIST_DIR/$PKG_NAME" | cut -f1)"
