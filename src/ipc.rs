use crate::roms::DiscoveredGame;
use crate::state::LockStatus;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub lock_status: LockStatus,
    pub quota_enabled: bool,
    pub bedtime_enabled: bool,
    pub effective_remaining_seconds: u64,
    pub quota_remaining_seconds: u64,
    pub played_seconds_today: u64,
    pub has_pin: bool,
    pub pin_lockout_remaining_seconds: u64,
    pub portal_active: bool,
    pub portal_mode: String,
    pub portal_url: String,
    pub wifi_ssid: String,
    pub wifi_passphrase: String,
    pub home_wifi_ssid: Option<String>,
    pub language: String,
    pub exit_to_muos_requested: bool,
    pub battery_percent: u8,
    pub battery_charging: bool,
    pub allowed_games: Vec<DiscoveredGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    GetStatus,
    VerifyPinAndStartPortal {
        pin: String,
    },
    StartPortalFirstBoot,
    ForceOfflineAp,
    StopPortal,
    PrepareGameLaunch {
        game_id: String,
    },
    NotifyGamePid {
        pid: Option<u32>,
    },
    RequestExitToMuos,
    RequestPowerOff,
    DevAdvanceTime {
        wall_delta_sec: i64,
        monotonic_delta_sec: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "resp", rename_all = "snake_case")]
pub enum IpcResponse {
    Status {
        status: DaemonStatus,
    },
    PinResult {
        unlocked: bool,
        lockout_seconds: u64,
        status: DaemonStatus,
    },
    LaunchApproved {
        game: DiscoveredGame,
    },
    Ok {
        status: DaemonStatus,
    },
    Error {
        message: String,
    },
}

/// Sends a synchronous JSON-line IPC request to `yasumu daemon` over the Unix domain socket.
pub fn send_ipc_request(socket_path: &Path, req: &IpcRequest) -> Result<IpcResponse> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connect to daemon socket {}", socket_path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut payload = serde_json::to_string(req)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let resp: IpcResponse =
        serde_json::from_str(line.trim()).context("parse IPC response JSON")?;
    Ok(resp)
}

/// Reads battery percentage and charging status from `/sys/class/power_supply/`
/// (specifically `axp2202-battery` on RG35XXSP, or `BAT0`/`BAT1` on laptops, with `100%` fallback).
pub fn read_battery_status() -> (u8, bool) {
    if let Ok(env_pct) = std::env::var("QUOS_MOCK_BATTERY") {
        if let Ok(pct) = env_pct.parse::<u8>() {
            let charging = std::env::var("QUOS_MOCK_CHARGING")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return (pct.min(100), charging);
        }
    }

    let power_dir = Path::new("/sys/class/power_supply");
    let candidates = ["axp2202-battery", "battery", "BAT0", "BAT1"];
    for name in candidates {
        let dir = power_dir.join(name);
        if dir.is_dir() {
            let pct = fs::read_to_string(dir.join("capacity"))
                .ok()
                .and_then(|s| s.trim().parse::<u8>().ok())
                .unwrap_or(100)
                .min(100);
            let charging = fs::read_to_string(dir.join("status"))
                .ok()
                .map(|s| {
                    let st = s.trim().to_ascii_lowercase();
                    st == "charging" || st == "full"
                })
                .unwrap_or(false);
            return (pct, charging);
        }
    }
    (100, false)
}
