#!/usr/bin/env bash
# Packages a compiled `yasumu` binary and muOS integration hooks into:
#   1. dist/yasumuOS.muxapp           (Zero-warning muOS Application installer via Archive Manager)
#   2. dist/yasumuOS-installer.tar.gz (Standalone SSH/ADB installer bundle)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_PATH="${1:-${ROOT_DIR}/target/aarch64-unknown-linux-gnu/release/yasumu}"
if [ ! -f "$BIN_PATH" ]; then
    BIN_PATH="${ROOT_DIR}/target/release/yasumu"
fi
if [ ! -f "$BIN_PATH" ]; then
    echo "Error: yasumu binary not found at $BIN_PATH" >&2
    exit 1
fi

DIST_DIR="${ROOT_DIR}/dist"
STAGE_APP="${DIST_DIR}/stage-muxapp"
APP_DIR="${STAGE_APP}/yasumuOS"
APP_BIN="${APP_DIR}/bin"
STAGE_TAR="${DIST_DIR}/stage-installer/yasumuOS-installer"

rm -rf "$STAGE_APP" "${DIST_DIR}/stage-installer" \
       "${DIST_DIR}/yasumuOS.muxapp" \
       "${DIST_DIR}/yasumuOS-installer.tar.gz" \
       "${DIST_DIR}/yasumuOS-rg35xxsp.muxapp" \
       "${DIST_DIR}/yasumuOS-rg35xxsp-installer.tar.gz"
mkdir -p "$APP_BIN" "$STAGE_TAR"

# Populate muOS Application (.muxapp) layout (extracts cleanly to MUOS/application/yasumuOS/ with zero warnings)
cp -f "$BIN_PATH" "${APP_BIN}/yasumu"
cp -f "${ROOT_DIR}/muos/retroarch-override.cfg" "${APP_BIN}/retroarch-override.cfg"
cp -f "${ROOT_DIR}/muos/ap-up.sh" "${APP_BIN}/ap-up.sh"
cp -f "${ROOT_DIR}/muos/ap-down.sh" "${APP_BIN}/ap-down.sh"
cp -f "${ROOT_DIR}/muos/frontend-wrapper.sh" "${APP_BIN}/frontend-wrapper.sh"
cp -f "${ROOT_DIR}/muos/install.sh" "${APP_BIN}/install.sh"
cp -f "${ROOT_DIR}/muos/uninstall.sh" "${APP_BIN}/uninstall.sh"
if [ -f "${ROOT_DIR}/assets/bootlogo.bmp" ]; then
    cp -f "${ROOT_DIR}/assets/bootlogo.bmp" "${APP_BIN}/bootlogo.bmp"
fi
if [ -f "${ROOT_DIR}/assets/fonts/CJK-Fallback.ttf" ]; then
    cp -f "${ROOT_DIR}/assets/fonts/CJK-Fallback.ttf" "${APP_BIN}/cjk-fallback.ttf"
fi
if [ -f "${ROOT_DIR}/assets/app-icon.png" ]; then
    mkdir -p "${APP_DIR}/glyph"
    cp -f "${ROOT_DIR}/assets/app-icon.png" "${APP_DIR}/glyph/yasumuos.png"
    cp -f "${ROOT_DIR}/assets/app-icon.png" "${APP_DIR}/glyph/yasumuOS.png"
fi

# muOS Application entry point: installs/upgrades yasumuOS atomically and reboots into it
cat > "${APP_DIR}/mux_launch.sh" <<'EOF'
#!/bin/sh
# HELP: Install or upgrade yasumuOS and reboot into the launcher.
# ICON: yasumuOS

APP_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
"${APP_DIR}/bin/install.sh"
sync
if [ -x /opt/muos/script/system/halt.sh ]; then
    /opt/muos/script/system/halt.sh reboot
else
    reboot
fi
EOF

chmod +x "${APP_DIR}/mux_launch.sh" \
         "${APP_BIN}/yasumu" \
         "${APP_BIN}/ap-up.sh" \
         "${APP_BIN}/ap-down.sh" \
         "${APP_BIN}/frontend-wrapper.sh" \
         "${APP_BIN}/install.sh" \
         "${APP_BIN}/uninstall.sh"

# Create dist/yasumuOS.muxapp using zip or python3 zipfile fallback
if command -v zip >/dev/null 2>&1; then
    (cd "$STAGE_APP" && zip -qr "${DIST_DIR}/yasumuOS.muxapp" yasumuOS)
else
    python3 - "$STAGE_APP" "${DIST_DIR}/yasumuOS.muxapp" <<'PY'
import os, sys, zipfile
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as zf:
    for root, _, files in os.walk(src):
        for f in files:
            full = os.path.join(root, f)
            rel = os.path.relpath(full, src)
            zf.write(full, rel)
PY
fi

# Populate standalone installer tarball
cp -f "${APP_BIN}/yasumu" "${STAGE_TAR}/yasumu"
cp -f "${APP_BIN}/retroarch-override.cfg" "${STAGE_TAR}/retroarch-override.cfg"
cp -f "${APP_BIN}/ap-up.sh" "${STAGE_TAR}/ap-up.sh"
cp -f "${APP_BIN}/ap-down.sh" "${STAGE_TAR}/ap-down.sh"
cp -f "${APP_BIN}/frontend-wrapper.sh" "${STAGE_TAR}/frontend-wrapper.sh"
cp -f "${APP_BIN}/install.sh" "${STAGE_TAR}/install.sh"
cp -f "${APP_BIN}/uninstall.sh" "${STAGE_TAR}/uninstall.sh"
if [ -f "${APP_BIN}/bootlogo.bmp" ]; then
    cp -f "${APP_BIN}/bootlogo.bmp" "${STAGE_TAR}/bootlogo.bmp"
fi
if [ -f "${APP_BIN}/cjk-fallback.ttf" ]; then
    cp -f "${APP_BIN}/cjk-fallback.ttf" "${STAGE_TAR}/cjk-fallback.ttf"
fi

chmod +x "${STAGE_TAR}/yasumu" \
         "${STAGE_TAR}/ap-up.sh" \
         "${STAGE_TAR}/ap-down.sh" \
         "${STAGE_TAR}/frontend-wrapper.sh" \
         "${STAGE_TAR}/install.sh" \
         "${STAGE_TAR}/uninstall.sh"

tar -C "${DIST_DIR}/stage-installer" -czf "${DIST_DIR}/yasumuOS-installer.tar.gz" yasumuOS-installer
rm -rf "$STAGE_APP" "${DIST_DIR}/stage-installer"

echo "Packaged:"
echo "  - ${DIST_DIR}/yasumuOS.muxapp"
echo "  - ${DIST_DIR}/yasumuOS-installer.tar.gz"
