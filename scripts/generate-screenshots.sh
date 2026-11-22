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
mkdir -p "${ROMS_DIR}/GBA/Imgs" "${ROMS_DIR}/SNES" "${ROMS_DIR}/GBC" "${ROMS_DIR}/GB"
printf "gba" > "${ROMS_DIR}/GBA/Astro Runner.gba"
printf "sfc" > "${ROMS_DIR}/SNES/Caverns of Sol.sfc"
printf "gbc" > "${ROMS_DIR}/GBC/Pocket Racers DX.gbc"
printf "gb" > "${ROMS_DIR}/GB/Pixel Odyssey.gb"

# Generate a custom retro synthwave sunset & starfield 256x256 PNG box art for Astro Runner
python3 - "${ROMS_DIR}/GBA/Imgs/Astro Runner.png" <<'PY'
import math, sys, struct, zlib

w, h = 256, 256
stars = {(27, 24), (58, 41), (92, 19), (144, 33), (198, 22), (221, 51), (42, 72), (176, 62), (210, 86), (78, 56), (120, 28), (235, 34)}
raw = bytearray()
for y in range(h):
    raw.append(0)
    for x in range(w):
        if y < 148:
            # Deep twilight sky gradient
            t = y / 148.0
            r = int(14 + 46 * (t ** 1.8))
            g = int(16 + 22 * (t ** 1.5))
            b = int(42 + 54 * t)
            # Glowing retro sun
            dx, dy = x - 128, y - 122
            dist = math.hypot(dx, dy)
            if dist < 44:
                st = max(0.0, min(1.0, (y - 78) / 70.0))
                # Retro horizontal scanline cutouts in lower sun
                cut = (y > 116) and ((y % 8) in (0, 1))
                if not cut:
                    r = int(255)
                    g = int(204 - 110 * st)
                    b = int(46 - 26 * st)
            elif dist < 66:
                glow = (1.0 - (dist - 44) / 22.0) ** 2
                r = min(255, int(r + 120 * glow))
                g = min(255, int(g + 55 * glow))
                b = min(255, int(b + 20 * glow))
            # Crisp 2x2 stars above horizon
            if any(abs(x - sx) <= 1 and abs(y - sy) <= 1 for sx, sy in stars) and dist > 48:
                r, g, b = 244, 244, 250
            # Mountain silhouette near horizon
            m_h = 148 - int(18 * max(0.0, 1.0 - abs(x - 62) / 52.0) + 22 * max(0.0, 1.0 - abs(x - 196) / 60.0))
            if y >= m_h:
                r, g, b = 15, 18, 32
        else:
            # Perspective grid horizon
            gt = (y - 148) / 108.0
            r = int(16 + 10 * gt)
            g = int(14 + 8 * gt)
            b = int(34 + 22 * gt)
            # Horizontal perspective lines
            row_band = int((gt ** 0.55) * 9.0 * 100) % 100
            # Vertical converging grid lines
            rel_x = (x - 128) / (0.18 + gt * 1.45)
            col_band = abs((rel_x % 26) - 13)
            if y == 148 or y == 149:
                r, g, b = 255, 102, 0
            elif row_band < 7 or col_band > 12.1:
                r, g, b = int(95 + 115 * (1 - gt)), int(52 + 40 * (1 - gt)), int(180 + 45 * gt)
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
            {"id":"GBA/Astro Runner.gba","allowed":true,"custom_title":"Astro Runner","core_override":null},
            {"id":"SNES/Caverns of Sol.sfc","allowed":true,"custom_title":"Caverns of Sol","core_override":null},
            {"id":"GBC/Pocket Racers DX.gbc","allowed":true,"custom_title":"Pocket Racers DX","core_override":null},
            {"id":"GB/Pixel Odyssey.gb","allowed":true,"custom_title":"Pixel Odyssey","core_override":null}
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
    start_daemon "$(date +%s)" "$data_dir" "$port"
    local cur_hour
    cur_hour="$(date +%-H)"
    local bed_json='{"enabled":true,"wake_time":"07:00","bed_time":"20:00"}'
    if [ "$cur_hour" -lt 7 ] || [ "$cur_hour" -ge 20 ]; then
        bed_json='{"enabled":true,"wake_time":"00:00","bed_time":"23:59"}'
    fi
    curl -fsS -X POST "http://127.0.0.1:${port}/api/schedule" \
        -H "Content-Type: application/json" \
        -d "{\"quota_enabled\":true,\"weekly_quotas\":{\"mon\":45,\"tue\":45,\"wed\":45,\"thu\":45,\"fri\":45,\"sat\":90,\"sun\":90},\"bedtime\":${bed_json}}" >/dev/null

    # Seed realistic 7-day activity and most-played game rankings for the Overview showcase
    local seed_payload
    seed_payload="$(python3 - <<'PY'
import datetime, json
today = datetime.date.today()
mins = [38, 45, 22, 45, 74, 62, 18]
hist = {
    (today - datetime.timedelta(days=6 - i)).isoformat(): m * 60
    for i, m in enumerate(mins)
}
print(json.dumps({
    "played_seconds_today": 18 * 60,
    "history": hist,
    "game_play_seconds": {
        "GBA/Astro Runner.gba": 135 * 60,
        "GBC/Pocket Racers DX.gbc": 80 * 60,
        "SNES/Caverns of Sol.sfc": 48 * 60,
        "GB/Pixel Odyssey.gb": 26 * 60,
    },
}))
PY
)"
    curl -fsS -X POST "http://127.0.0.1:${port}/api/dev/seed-stats" \
        -H "Content-Type: application/json" \
        -d "$seed_payload" >/dev/null

    if command -v firefox >/dev/null 2>&1; then
        local ff_prof="${TMP_DIR}/ff_profile_${lang}"
        mkdir -p "$ff_prof"
        for tab in overview games schedule system; do
            firefox --headless --profile "$ff_prof" \
                --screenshot "${OUT_DIR}/web/portal_${lang}_${tab}.png" \
                --window-size=430,900 \
                "http://127.0.0.1:${port}/#${tab}" >/dev/null 2>&1 || true
        done
    fi

    # 6. Shutdown Screen (confirming power off from game list)
    cat > "${TMP_DIR}/shutdown_${lang}.script" <<EOF
PRESS BACK
PRESS DOWN
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
