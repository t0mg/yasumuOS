#!/usr/bin/env bash
# Generates 640x480 PNG screenshots of every yasumuOS launcher UI screen in English (en/)
# and French (fr/), plus full-page screenshots of the Parent Portal Web UI (web/).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-${ROOT_DIR}/ui-review}"
QUOS_BIN="${ROOT_DIR}/target/release/yasumu"

cargo build --release --offline --manifest-path "${ROOT_DIR}/Cargo.toml"

mkdir -p "${OUT_DIR}/en" "${OUT_DIR}/fr" "${OUT_DIR}/web"
TMP_DIR="$(mktemp -d)"
cleanup() {
    if [ -n "${DAEMON_PID:-}" ]; then
        kill "$DAEMON_PID" 2>/dev/null || true
    fi
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

export SDL_VIDEODRIVER=dummy
export SDL_AUDIODRIVER=dummy
export QUOS_MOCK_BATTERY=88
export QUOS_MOCK_CHARGING=0

ROMS_DIR="${TMP_DIR}/ROMS"
mkdir -p "${ROMS_DIR}/GBA/Imgs" "${ROMS_DIR}/SNES" "${ROMS_DIR}/GBC"
printf "gba" > "${ROMS_DIR}/GBA/Pokemon Emerald (USA).gba"
printf "sfc" > "${ROMS_DIR}/SNES/Super Mario World (USA) [!].sfc"
printf "gbc" > "${ROMS_DIR}/GBC/Zelda - Link's Awakening DX (USA).gbc"

# Generate a sample 128x128 PNG box art for Pokemon Emerald
python3 - "${ROMS_DIR}/GBA/Imgs/Pokemon Emerald (USA).png" <<'PY'
import sys, struct, zlib

w, h = 128, 128
raw = bytearray()
for y in range(h):
    raw.append(0)
    for x in range(w):
        border = x < 6 or x >= w - 6 or y < 6 or y >= h - 6
        dx, dy = x - 64, y - 64
        diamond = (abs(dx) + abs(dy)) < 34
        inner = (abs(dx) + abs(dy)) < 18
        if border:
            r, g, b = (250, 204, 21)
        elif inner:
            r, g, b = (52, 211, 153)
        elif diamond:
            r, g, b = (245, 158, 11)
        else:
            r, g, b = (6, int(95 + (y / h) * 70), int(70 + (x / w) * 40))
        raw.extend((r, g, b, 255))

def chunk(tag, data):
    return struct.pack("!I", len(data)) + tag + data + struct.pack("!I", zlib.crc32(tag + data) & 0xffffffff)

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack("!IIBBBBB", w, h, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
png += chunk(b"IEND", b"")
with open(sys.argv[1], "wb") as f:
    f.write(png)
PY

start_daemon() {
    local mock_time="$1"
    local data_dir="$2"
    local http_port="$3"
    if [ -n "${DAEMON_PID:-}" ]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    MOCK_TIME="$mock_time" "$QUOS_BIN" --dev --data-dir "$data_dir" daemon \
        --roms-dir "$ROMS_DIR" \
        --http-addr "127.0.0.1:${http_port}" \
        --retroarch-udp "127.0.0.1:59999" \
        --start-portal >/dev/null 2>&1 &
    DAEMON_PID=$!
    for _ in $(seq 1 50); do
        if [ -S "${data_dir}/yasumu.sock" ]; then
            break
        fi
        sleep 0.05
    done
    sleep 0.15
}

capture_language_set() {
    local lang="$1"
    local lang_dir="${OUT_DIR}/${lang}"
    local data_dir="${TMP_DIR}/data_${lang}"
    local port="$2"

    # 1. Start daemon at 2026-11-18 15:00:00 UTC (Wednesday)
    start_daemon 1795014000 "$data_dir" "$port"

    curl -fsS -X POST "http://127.0.0.1:${port}/api/language" \
        -H "Content-Type: application/json" \
        -d "{\"language\":\"${lang}\"}" >/dev/null

    curl -fsS -X POST "http://127.0.0.1:${port}/api/pin" \
        -H "Content-Type: application/json" \
        -d '{"pin":"1234"}' >/dev/null

    curl -fsS -X POST "http://127.0.0.1:${port}/api/games" \
        -H "Content-Type: application/json" \
        -d '{"games":[
            {"id":"GBA/Pokemon Emerald (USA).gba","allowed":true,"custom_title":"Pokemon Emerald","core_override":null},
            {"id":"SNES/Super Mario World (USA) [!].sfc","allowed":true,"custom_title":null,"core_override":null},
            {"id":"GBC/Zelda - Link'\''s Awakening DX (USA).gbc","allowed":true,"custom_title":"Zelda DX","core_override":null}
        ]}' >/dev/null

    curl -fsS -X POST "http://127.0.0.1:${port}/api/schedule" \
        -H "Content-Type: application/json" \
        -d '{"weekly_quotas":{"mon":45,"tue":45,"wed":45,"thu":45,"fri":45,"sat":90,"sun":90},"bedtime":{"enabled":true,"wake_time":"07:00","bed_time":"20:00"}}' >/dev/null

    # Run launcher flow FIRST while daemon mock clock is 15:00 (before Firefox's syncClock uses real wall time)
    cat > "${TMP_DIR}/flow_${lang}.script" <<EOF
SCREENSHOT ${lang_dir}/05_parent_portal_ap_qr.png
PRESS BACK
SCREENSHOT ${lang_dir}/01_game_list_boxart.png
PRESS DOWN
SCREENSHOT ${lang_dir}/02_game_list_cartridge.png
PRESS DOWN
PRESS DOWN
PRESS CONFIRM
SCREENSHOT ${lang_dir}/09_power_off_confirm.png
PRESS BACK
PRESS SELECT
PRESS CONFIRM
SCREENSHOT ${lang_dir}/04_pin_error.png
PRESS UP
PRESS RIGHT
PRESS UP
PRESS UP
PRESS RIGHT
PRESS UP
PRESS UP
PRESS UP
PRESS RIGHT
PRESS UP
PRESS UP
PRESS UP
PRESS UP
SCREENSHOT ${lang_dir}/03_pin_tumbler.png
PRESS CONFIRM
EOF

    "$QUOS_BIN" --dev --data-dir "$data_dir" launcher --script "${TMP_DIR}/flow_${lang}.script"

    # 2. Quota Reached (15:00 UTC, 0 min daily quota)
    curl -fsS -X POST "http://127.0.0.1:${port}/api/schedule" \
        -H "Content-Type: application/json" \
        -d '{"weekly_quotas":{"mon":0,"tue":0,"wed":0,"thu":0,"fri":0,"sat":0,"sun":0},"bedtime":{"enabled":true,"wake_time":"07:00","bed_time":"20:00"}}' >/dev/null

    cat > "${TMP_DIR}/quota_${lang}.script" <<EOF
PRESS BACK
SCREENSHOT ${lang_dir}/06_lock_quota_reached.png
EOF
    "$QUOS_BIN" --dev --data-dir "$data_dir" launcher --script "${TMP_DIR}/quota_${lang}.script"

    # 3. Bedtime Active (21:00 UTC)
    start_daemon 1795035600 "$data_dir" "$port"
    cat > "${TMP_DIR}/bed_${lang}.script" <<EOF
PRESS BACK
SCREENSHOT ${lang_dir}/07_lock_bedtime.png
EOF
    "$QUOS_BIN" --dev --data-dir "$data_dir" launcher --script "${TMP_DIR}/bed_${lang}.script"

    # 4. Clock Sync Required (RTC reset to 1970)
    start_daemon 1000 "$data_dir" "$port"
    cat > "${TMP_DIR}/clock_${lang}.script" <<EOF
PRESS BACK
SCREENSHOT ${lang_dir}/08_lock_clock_sync.png
EOF
    "$QUOS_BIN" --dev --data-dir "$data_dir" launcher --script "${TMP_DIR}/clock_${lang}.script"

    # 5. Capture all 4 Web Portal tabs (mobile 430px) in both EN and FR last
    # (Firefox runs syncClock() with real wall time, so run after deterministic mock-clock launcher shots)
    start_daemon 1795014000 "$data_dir" "$port"
    local cur_hour
    cur_hour="$(date +%-H)"
    local bed_json='{"enabled":true,"wake_time":"07:00","bed_time":"20:00"}'
    if [ "$cur_hour" -lt 7 ] || [ "$cur_hour" -ge 20 ]; then
        bed_json='{"enabled":true,"wake_time":"00:00","bed_time":"23:59"}'
    fi
    curl -fsS -X POST "http://127.0.0.1:${port}/api/schedule" \
        -H "Content-Type: application/json" \
        -d "{\"quota_enabled\":true,\"weekly_quotas\":{\"mon\":45,\"tue\":45,\"wed\":45,\"thu\":45,\"fri\":45,\"sat\":90,\"sun\":90},\"bedtime\":${bed_json}}" >/dev/null

    if command -v firefox >/dev/null 2>&1; then
        local ff_prof="${TMP_DIR}/ff_profile_${lang}"
        mkdir -p "$ff_prof"
        for tab in overview games schedule system; do
            firefox --headless --profile "$ff_prof" \
                --screenshot "${OUT_DIR}/web/portal_${lang}_${tab}.png" \
                --window-size=430,860 \
                "http://127.0.0.1:${port}/#${tab}" >/dev/null 2>&1 || true
        done
    fi

    # 6. Shutdown Screen (confirming power off from game list)
    cat > "${TMP_DIR}/shutdown_${lang}.script" <<EOF
PRESS BACK
PRESS DOWN
PRESS DOWN
PRESS DOWN
PRESS CONFIRM
PRESS CONFIRM
SCREENSHOT ${lang_dir}/10_shutting_down.png
EOF
    "$QUOS_BIN" --dev --data-dir "$data_dir" launcher --script "${TMP_DIR}/shutdown_${lang}.script"
}

capture_language_set "en" 18081
capture_language_set "fr" 18082

echo "Generated UI review screenshots in ${OUT_DIR}:"
find "$OUT_DIR" -type f -name '*.png' | sort
