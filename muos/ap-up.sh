#!/bin/sh
# yasumuOS Smart Dual-Mode Parent Portal Network Script for MustardOS (RG35XXSP)
#
# Args: <AP_SSID> <AP_PASSPHRASE> [HOME_WIFI_SSID] [HOME_WIFI_PASSPHRASE]
# 1. If HOME_WIFI_SSID is configured, tries to join Home Wi-Fi via wpa_supplicant + udhcpc (~6s).
#    On success, writes {"mode":"home_wifi","ip":"<LAN_IP>","ssid":"<HOME_SSID>"} to /run/quos-ap/portal-status.json.
# 2. If HOME_WIFI_SSID is unset or unreachable, automatically falls back to Offline WPA2-PSK AP (192.168.4.1)
#    and writes {"mode":"ap","ip":"192.168.4.1","ssid":"<AP_SSID>"} to /run/quos-ap/portal-status.json.
set -eu

if [ -f /opt/yasumu/logging_enabled ]; then
    LOG_DIR="/mnt/mmc/yasumu-logs"
    mkdir -p "$LOG_DIR" 2>/dev/null || LOG_DIR="/var/log"
    exec >>"${LOG_DIR}/ap.log" 2>&1
else
    LOG_DIR="/tmp"
    exec >/dev/null 2>&1
fi
echo "[$(date)] Starting yasumuOS Portal Network..."

AP_SSID="${1:-Yasumu}"
AP_PASS="${2:-Yasumu1234}"
HOME_SSID="${3:-}"
HOME_PASS="${4:-}"
IFACE="${YASUMU_WIFI_IFACE:-${QUOS_WIFI_IFACE:-$(cat /opt/muos/device/config/network/iface 2>/dev/null || echo wlan0)}}"
NET_MOD="$(cat /opt/muos/device/config/network/name 2>/dev/null || echo 8821cs)"
AP_IP="192.168.4.1"
RUN_DIR="/run/yasumu-ap"
STATUS_FILE="${RUN_DIR}/portal-status.json"

mkdir -p "$RUN_DIR"
rm -f "$STATUS_FILE"

# Load Wi-Fi kernel driver if not already loaded (e.g. 8821cs on RG35XXSP)
modprobe -q "$NET_MOD" 2>/dev/null || true
rfkill unblock wifi 2>/dev/null || true

# Wait up to 3 seconds for wlan0 interface to appear
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
    if [ -d "/sys/class/net/${IFACE}" ]; then
        break
    fi
    sleep 0.25
done

stop_existing() {
    killall wpa_supplicant 2>/dev/null || true
    killall dhcpcd 2>/dev/null || true
    killall udhcpc 2>/dev/null || true
    killall udhcpd 2>/dev/null || true
    rm -rf "${RUN_DIR}/wpa_ctrl"
    sleep 0.25
    ip link set "$IFACE" down 2>/dev/null || true
    ip addr flush dev "$IFACE" 2>/dev/null || true
    ip link set "$IFACE" up
}

# --- Mode 1: Try Home Wi-Fi (Station Mode) if configured ---
if [ -n "$HOME_SSID" ]; then
    echo "[$(date)] Trying Home Wi-Fi SSID=${HOME_SSID}..."
    stop_existing

    if [ -n "$HOME_PASS" ]; then
        cat > "${RUN_DIR}/wpa_sta.conf" <<EOF
ctrl_interface=${RUN_DIR}/wpa_ctrl
update_config=0
country=US
network={
    ssid="${HOME_SSID}"
    psk="${HOME_PASS}"
}
EOF
    else
        cat > "${RUN_DIR}/wpa_sta.conf" <<EOF
ctrl_interface=${RUN_DIR}/wpa_ctrl
update_config=0
country=US
network={
    ssid="${HOME_SSID}"
    key_mgmt=NONE
}
EOF
    fi

    wpa_supplicant -B -D nl80211 -i "$IFACE" -P "${RUN_DIR}/wpa_ap.pid" -c "${RUN_DIR}/wpa_sta.conf" -f "${LOG_DIR}/wpa_supplicant.log" || true

    # Wait up to 6 seconds for WPA association
    CONNECTED=0
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24; do
        if wpa_cli -p "${RUN_DIR}/wpa_ctrl" -i "$IFACE" status 2>/dev/null | grep -q "wpa_state=COMPLETED"; then
            CONNECTED=1
            break
        fi
        sleep 0.25
    done

    if [ "$CONNECTED" -eq 1 ]; then
        iw dev "$IFACE" set power_save off 2>/dev/null || true
        echo "[$(date)] Associated with Home Wi-Fi, requesting DHCP lease..."
        if command -v dhcpcd >/dev/null 2>&1; then
            dhcpcd -4 -t 8 "$IFACE" >/dev/null 2>&1 || true
        else
            udhcpc -i "$IFACE" -n -q -t 6 -T 1 >/dev/null 2>&1 || true
        fi
        LAN_IP="$(ip -4 addr show dev "$IFACE" | awk '/inet /{print $2}' | cut -d/ -f1 | head -n 1)"
        if [ -n "$LAN_IP" ]; then
            echo "[$(date)] Connected to Home Wi-Fi (${HOME_SSID}) with IP ${LAN_IP}"
            ip addr show dev "$IFACE" 2>&1 || true
            ip route 2>&1 || true
            cat /etc/resolv.conf 2>&1 || true
            # Keep 8821cs Wi-Fi awake and router ARP table fresh while portal is open
            GW_IP="$(ip route | awk '/default/{print $3; exit}')"
            if [ -n "$GW_IP" ]; then
                (
                    while true; do
                        ping -c 1 -W 1 "$GW_IP" >/dev/null 2>&1 || true
                        sleep 2
                    done
                ) &
                echo $! > "${RUN_DIR}/keepalive.pid"
            fi
            printf '{"mode":"home_wifi","ip":"%s","ssid":"%s"}\n' "$LAN_IP" "$HOME_SSID" > "$STATUS_FILE"
            exit 0
        fi
    fi

    echo "[$(date)] Home Wi-Fi unavailable or DHCP timed out; falling back to Offline AP..."
fi

# --- Mode 2: Offline WPA2-PSK Access Point Fallback ---
stop_existing

cat > "${RUN_DIR}/wpa_ap.conf" <<EOF
ctrl_interface=${RUN_DIR}/wpa_ctrl
ap_scan=1
country=US
network={
    ssid="${AP_SSID}"
    mode=2
    frequency=2437
    key_mgmt=WPA-PSK
    proto=RSN
    pairwise=CCMP
    group=CCMP
    psk="${AP_PASS}"
}
EOF

wpa_supplicant -B -D nl80211 -i "$IFACE" -P "${RUN_DIR}/wpa_ap.pid" -c "${RUN_DIR}/wpa_ap.conf" -f "${LOG_DIR}/wpa_supplicant.log"

for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    if wpa_cli -p "${RUN_DIR}/wpa_ctrl" -i "$IFACE" status 2>/dev/null | grep -q "wpa_state=COMPLETED"; then
        break
    fi
    sleep 0.25
done

ip addr replace "${AP_IP}/24" dev "$IFACE"
ip link set "$IFACE" up

touch "${RUN_DIR}/udhcpd.leases"
cat > "${RUN_DIR}/udhcpd.conf" <<EOF
start 192.168.4.10
end 192.168.4.50
interface ${IFACE}
lease_file ${RUN_DIR}/udhcpd.leases
opt subnet 255.255.255.0
opt router ${AP_IP}
opt dns ${AP_IP}
opt lease 3600
EOF

udhcpd -f "${RUN_DIR}/udhcpd.conf" >>"${LOG_DIR}/udhcpd.log" 2>&1 &
echo $! > "${RUN_DIR}/udhcpd.pid"

printf '{"mode":"ap","ip":"%s","ssid":"%s"}\n' "$AP_IP" "$AP_SSID" > "$STATUS_FILE"
echo "[$(date)] yasumuOS AP started on ${IFACE} (${AP_IP}), SSID=${AP_SSID}"
