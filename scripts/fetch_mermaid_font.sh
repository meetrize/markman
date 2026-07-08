#!/usr/bin/env bash
# Download Source Han Sans SC (思源黑体) for embedded Mermaid diagram labels.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

FONT_DIR="$MARKMAN_PROJECT_ROOT/assets/fonts"
FONT_PATH="$FONT_DIR/SourceHanSansSC-Regular.otf"
FONT_URL="https://cdn.jsdelivr.net/gh/adobe-fonts/source-han-sans@release/OTF/SimplifiedChinese/SourceHanSansSC-Regular.otf"

mkdir -p "$FONT_DIR"

if [[ -f "$FONT_PATH" ]]; then
    markman_info "Mermaid font already present: $FONT_PATH"
    exit 0
fi

markman_info "Downloading Source Han Sans SC..."
curl -fsSL --connect-timeout 30 -o "$FONT_PATH" "$FONT_URL"
markman_info "Saved $FONT_PATH ($(du -h "$FONT_PATH" | awk '{print $1}'))"
