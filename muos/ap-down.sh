#!/bin/sh
# yasumuOS On-Demand Wi-Fi Teardown Script for MustardOS
set -eu

IFACE="${YASUMU_WIFI_IFACE:-${QUOS_WIFI_IFACE:-$(cat /opt/muos/device/config/network/iface 2>/dev/null || echo wlan0)}}"
RUN_DIR="/run/yasumu-ap"

if [ -f "${RUN_DIR}/keepalive.pid" ]; then
    kill "$(cat "${RUN_DIR}/keepalive.pid")" 2>/dev/null || true
fi
if [ -f "${RUN_DIR}/wpa_ap.pid" ]; then
    kill "$(cat "${RUN_DIR}/wpa_ap.pid")" 2>/dev/null || true
fi
if [ -f "${RUN_DIR}/udhcpd.pid" ]; then
    kill "$(cat "${RUN_DIR}/udhcpd.pid")" 2>/dev/null || true
fi

killall udhcpd 2>/dev/null || true
killall udhcpc 2>/dev/null || true
killall dhcpcd 2>/dev/null || true
killall wpa_supplicant 2>/dev/null || true

ip addr flush dev "$IFACE" 2>/dev/null || true
ip link set "$IFACE" down 2>/dev/null || true
rm -rf "$RUN_DIR"
