#!/bin/sh
# yasumuOS Replacement for /opt/muos/script/mux/frontend.sh
# - Applies any staged upgrade (/opt/yasumu/yasumu.new -> /opt/yasumu/yasumu) atomically
# - Installs the custom boot splash reversibly (backing up stock bootlogo.bmp once)
# - Launches `yasumu daemon` + `yasumu launcher`, or falls back to stock muOS
#   for the current boot session if "Exit to Standard muOS" was requested.

YASUMU_DIR="/opt/yasumu"
YASUMU_BIN="${YASUMU_DIR}/yasumu"
YASUMU_NEW="${YASUMU_DIR}/yasumu.new"
YASUMU_SPLASH="${YASUMU_DIR}/bootlogo.bmp"
STOCK_FRONTEND="/opt/muos/script/mux/frontend.sh.orig"
STOCK_MUXLAUNCH="/opt/muos/extra/muxlaunch"

if [ -f "${YASUMU_DIR}/logging_enabled" ]; then
    LOG_DIR="/mnt/mmc/yasumu-logs"
    mkdir -p "$LOG_DIR" 2>/dev/null || LOG_DIR="/var/log"
    DAEMON_LOG="${LOG_DIR}/daemon.log"
    LAUNCHER_LOG="${LOG_DIR}/launcher.log"
else
    DAEMON_LOG="/dev/null"
    LAUNCHER_LOG="/dev/null"
fi

# 1. Apply staged binary upgrade atomically
if [ -f "$YASUMU_NEW" ]; then
    killall yasumu 2>/dev/null || true
    sleep 0.2
    mv -f "$YASUMU_NEW" "$YASUMU_BIN"
    chmod +x "$YASUMU_BIN"
fi

# 2. Apply custom boot splash reversibly (backs up stock bootlogo.bmp once to .yasumu.bak)
if [ -f "$YASUMU_SPLASH" ]; then
    for TARGET in /mnt/boot/bootlogo.bmp /boot/bootlogo.bmp; do
        if [ -f "$TARGET" ]; then
            if [ ! -f "${TARGET}.yasumu.bak" ]; then
                cp -a "$TARGET" "${TARGET}.yasumu.bak" 2>/dev/null || true
            fi
            if ! cmp -s "$YASUMU_SPLASH" "$TARGET" 2>/dev/null; then
                cp -f "$YASUMU_SPLASH" "$TARGET" 2>/dev/null || true
            fi
        fi
    done
fi

# 3. Ensure yasumu daemon is running
if ! pidof yasumu >/dev/null 2>&1; then
    "$YASUMU_BIN" daemon >>"$DAEMON_LOG" 2>&1 &
    sleep 1
fi

launch_stock_muos() {
    if [ -x "$STOCK_FRONTEND" ]; then
        exec "$STOCK_FRONTEND" "$@"
    elif [ -x "$STOCK_MUXLAUNCH" ]; then
        exec "$STOCK_MUXLAUNCH" "$@"
    fi
}

while true; do
    if [ -f /tmp/yasumu_exit_to_muos ]; then
        launch_stock_muos "$@"
    fi

    "$YASUMU_BIN" launcher >>"$LAUNCHER_LOG" 2>&1

    if [ -f /tmp/yasumu_exit_to_muos ]; then
        launch_stock_muos "$@"
    fi
    sleep 1
done
