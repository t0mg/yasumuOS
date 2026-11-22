#!/bin/sh
# yasumuOS Installer / Upgrader for muOS Handhelds
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
YASUMU_DIR="/opt/yasumu"
FRONTEND_SH="/opt/muos/script/mux/frontend.sh"
STARTUP_SH="/opt/muos/script/system/startup.sh"

mkdir -p "$YASUMU_DIR"
chmod 700 "$YASUMU_DIR"

# Atomic binary replacement avoids ETXTBSY if `yasumu daemon` is currently running
cp -f "${SCRIPT_DIR}/yasumu" "${YASUMU_DIR}/yasumu.new"
chmod +x "${YASUMU_DIR}/yasumu.new"
mv -f "${YASUMU_DIR}/yasumu.new" "${YASUMU_DIR}/yasumu"

cp -f "${SCRIPT_DIR}/retroarch-override.cfg" "${YASUMU_DIR}/retroarch-override.cfg"
cp -f "${SCRIPT_DIR}/ap-up.sh" "${YASUMU_DIR}/ap-up.sh"
cp -f "${SCRIPT_DIR}/ap-down.sh" "${YASUMU_DIR}/ap-down.sh"
cp -f "${SCRIPT_DIR}/frontend-wrapper.sh" "${YASUMU_DIR}/frontend-wrapper.sh"
cp -f "${SCRIPT_DIR}/uninstall.sh" "${YASUMU_DIR}/uninstall.sh"
if [ -f "${SCRIPT_DIR}/bootlogo.bmp" ]; then
    cp -f "${SCRIPT_DIR}/bootlogo.bmp" "${YASUMU_DIR}/bootlogo.bmp"
fi
if [ -f "${SCRIPT_DIR}/cjk-fallback.ttf" ]; then
    cp -f "${SCRIPT_DIR}/cjk-fallback.ttf" "${YASUMU_DIR}/cjk-fallback.ttf"
fi

chmod +x "${YASUMU_DIR}/yasumu" \
         "${YASUMU_DIR}/ap-up.sh" \
         "${YASUMU_DIR}/ap-down.sh" \
         "${YASUMU_DIR}/frontend-wrapper.sh" \
         "${YASUMU_DIR}/uninstall.sh"

# Back up stock muOS frontend.sh once and install yasumuOS wrapper
if [ -f "$FRONTEND_SH" ] && [ ! -f "${FRONTEND_SH}.orig" ]; then
    if ! grep -q "yasumuOS" "$FRONTEND_SH" 2>/dev/null; then
        cp -a "$FRONTEND_SH" "${FRONTEND_SH}.orig"
    fi
fi
if [ -d "$(dirname "$FRONTEND_SH")" ]; then
    cp -f "${YASUMU_DIR}/frontend-wrapper.sh" "$FRONTEND_SH"
    chmod +x "$FRONTEND_SH"
fi

# Reversibly install custom boot splash (backs up stock bootlogo.bmp once)
if [ -f "${YASUMU_DIR}/bootlogo.bmp" ]; then
    for TARGET in /mnt/boot/bootlogo.bmp /boot/bootlogo.bmp; do
        if [ -f "$TARGET" ]; then
            if [ ! -f "${TARGET}.yasumu.bak" ]; then
                cp -a "$TARGET" "${TARGET}.yasumu.bak" 2>/dev/null || true
            fi
            cp -f "${YASUMU_DIR}/bootlogo.bmp" "$TARGET" 2>/dev/null || true
        fi
    done
fi

# Hook `yasumu daemon` into /opt/muos/script/system/startup.sh if present
if [ -f "$STARTUP_SH" ]; then
    if [ ! -f "${STARTUP_SH}.orig" ]; then
        cp -a "$STARTUP_SH" "${STARTUP_SH}.orig"
    fi
    if ! grep -q "/opt/yasumu/yasumu daemon" "$STARTUP_SH"; then
        sed -i '2i # yasumuOS Watchdog Daemon\n/opt/yasumu/yasumu daemon >/dev/null 2>&1 &\n' "$STARTUP_SH"
    fi
fi

sync
echo "yasumuOS installed successfully to ${YASUMU_DIR}."
