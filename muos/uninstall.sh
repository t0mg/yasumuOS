#!/bin/sh
# yasumuOS Uninstaller for muOS Handhelds
set -eu

REBOOT_AFTER=0
if [ "${1:-}" = "--reboot" ]; then
    REBOOT_AFTER=1
fi

FRONTEND_SH="/opt/muos/script/mux/frontend.sh"
STARTUP_SH="/opt/muos/script/system/startup.sh"

if [ -f "${FRONTEND_SH}.orig" ]; then
    mv -f "${FRONTEND_SH}.orig" "$FRONTEND_SH"
    chmod +x "$FRONTEND_SH"
fi

if [ -f "${STARTUP_SH}.orig" ]; then
    mv -f "${STARTUP_SH}.orig" "$STARTUP_SH"
    chmod +x "$STARTUP_SH"
fi

# Restore stock muOS boot splash if backed up
for TARGET in /mnt/boot/bootlogo.bmp /boot/bootlogo.bmp; do
    if [ -f "${TARGET}.yasumu.bak" ]; then
        mv -f "${TARGET}.yasumu.bak" "$TARGET" 2>/dev/null || true
    fi
done

/opt/yasumu/ap-down.sh 2>/dev/null || true
rm -f /tmp/yasumu_exit_to_muos /run/yasumu.sock
rm -rf /mnt/mmc/MUOS/application/yasumuOS /opt/yasumu
sync

if [ "$REBOOT_AFTER" -eq 1 ]; then
    if [ -x /opt/muos/script/system/halt.sh ]; then
        /opt/muos/script/system/halt.sh reboot || reboot
    else
        reboot
    fi
else
    killall yasumu 2>/dev/null || true
    echo "yasumuOS removed and stock muOS frontend + splash restored."
fi
