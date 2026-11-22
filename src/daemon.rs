use crate::ipc::{read_battery_status, DaemonStatus, IpcRequest, IpcResponse};
use crate::portal::{PortalHandle, SharedCtx, SharedDaemonContext};
use crate::retroarch::{play_warning_chime_async, ChimeKind, RetroArchController};
use crate::roms::scan_roms;
use crate::state::{LockStatus, StateStore};
use crate::time_provider::TimeProvider;
use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

pub struct DaemonConfig {
    pub data_dir: PathBuf,
    pub socket_path: PathBuf,
    pub rom_roots: Vec<PathBuf>,
    pub catalogue_roots: Vec<PathBuf>,
    pub retroarch_udp: SocketAddr,
    pub portal_bind: SocketAddr,
    pub dev_mode: bool,
    pub scan_proc_for_retroarch: bool,
    pub start_portal_on_boot: bool,
}

pub struct DaemonRuntime {
    pub ctx: SharedCtx,
    pub retroarch: RetroArchController,
    pub portal: Arc<Mutex<Option<PortalHandle>>>,
    pub portal_bind: SocketAddr,
    pub dev_mode: bool,
    pub last_tick_monotonic: Arc<Mutex<u64>>,
    pub last_flush_monotonic: Arc<Mutex<u64>>,
    pub was_retroarch_running: Arc<Mutex<bool>>,
    pub active_game_id: Arc<Mutex<Option<String>>>,
}

impl DaemonRuntime {
    pub fn new(cfg: &DaemonConfig, time: TimeProvider) -> Result<Self> {
        let now_epoch = time.wall_clock_secs();
        let store = StateStore::load_or_init(&cfg.data_dir, &cfg.rom_roots, now_epoch)?;
        let mono_now = time.monotonic_secs();

        let portal_url = if cfg.dev_mode {
            format!("http://{}", cfg.portal_bind)
        } else {
            "http://192.168.4.1".to_string()
        };

        let ctx = Arc::new(Mutex::new(SharedDaemonContext {
            store,
            time,
            rom_roots: cfg.rom_roots.clone(),
            catalogue_roots: cfg.catalogue_roots.clone(),
            dev_mode: cfg.dev_mode,
            portal_mode: "ap".to_string(),
            portal_url,
            last_portal_activity_monotonic: mono_now,
            exit_to_muos_requested: false,
        }));

        let retroarch = RetroArchController::new(cfg.retroarch_udp, cfg.scan_proc_for_retroarch);

        Ok(Self {
            ctx,
            retroarch,
            portal: Arc::new(Mutex::new(None)),
            portal_bind: cfg.portal_bind,
            dev_mode: cfg.dev_mode,
            last_tick_monotonic: Arc::new(Mutex::new(mono_now)),
            last_flush_monotonic: Arc::new(Mutex::new(mono_now)),
            was_retroarch_running: Arc::new(Mutex::new(false)),
            active_game_id: Arc::new(Mutex::new(None)),
        })
    }

    /// Runs a single watchdog tick:
    /// - Measures monotonic elapsed seconds if RetroArch is active
    /// - Evaluates quota & bedtime
    /// - Sends 5m / 1m UDP `SHOW_MSG` warnings or `QUIT` when time is up
    /// - Flushes `usage.json` every 60s or on game exit
    /// - Auto-closes Parent Portal after 300s of inactivity
    pub fn tick(&self) {
        let is_running = self.retroarch.is_retroarch_running();
        let mut was_running = self.was_retroarch_running.lock().unwrap();

        let (mono_now, wall_now) = {
            let guard = self.ctx.lock().unwrap();
            (guard.time.monotonic_secs(), guard.time.wall_clock_secs())
        };

        let mut last_tick = self.last_tick_monotonic.lock().unwrap();
        let delta_mono = mono_now.saturating_sub(*last_tick);
        *last_tick = mono_now;

        let portal_active = self.portal.lock().unwrap().is_some();
        let mut should_quit_retroarch = false;
        let mut msg_to_send: Option<(&'static str, ChimeKind)> = None;
        let mut should_stop_portal = false;

        {
            let mut guard = self.ctx.lock().unwrap();
            let local = guard.time.local_now(&guard.store.config.timezone);

            if is_running && delta_mono > 0 {
                guard.store.record_play_seconds(delta_mono, &local);
                if let Some(ref gid) = *self.active_game_id.lock().unwrap() {
                    guard.store.record_game_seconds(gid, delta_mono);
                }
            }

            let eval = guard.store.evaluate(&local, is_running);

            if is_running {
                let lang = crate::i18n::Language::from_code(&guard.store.config.language);
                let tr = crate::i18n::strings(lang);
                if eval.lock_status != LockStatus::Unlocked || eval.effective_remaining_seconds == 0
                {
                    should_quit_retroarch = true;
                } else if eval.trigger_1m_warning {
                    msg_to_send = Some((tr.osd_1m_left, ChimeKind::Warn1m));
                } else if eval.trigger_5m_warning {
                    msg_to_send = Some((tr.osd_5m_left, ChimeKind::Warn5m));
                }
            }

            let mut last_flush = self.last_flush_monotonic.lock().unwrap();
            let transitioned_off = *was_running && !is_running;
            let periodic_due = mono_now.saturating_sub(*last_flush) >= 60;

            if (is_running && periodic_due)
                || transitioned_off
                || should_quit_retroarch
                || msg_to_send.is_some()
            {
                let _ = guard.store.save_usage(wall_now);
                *last_flush = mono_now;
            }

            // Check 5-minute (300s) Parent Portal inactivity timeout
            if portal_active
                && mono_now.saturating_sub(guard.last_portal_activity_monotonic) >= 300
            {
                should_stop_portal = true;
            }
        }

        *was_running = is_running;

        if let Some((msg, chime)) = msg_to_send {
            let _ = self.retroarch.show_message(msg);
            if !self.dev_mode {
                play_warning_chime_async(chime);
            }
        }

        if should_quit_retroarch {
            let _ = self.retroarch.quit_retroarch(true);
        }

        if should_stop_portal {
            self.stop_portal();
        }
    }

    pub fn start_portal(&self) -> Result<()> {
        self.start_portal_with_mode(false)
    }

    pub fn start_portal_with_mode(&self, force_ap: bool) -> Result<()> {
        if !force_ap && self.portal.lock().unwrap().is_some() {
            self.ctx.lock().unwrap().touch_activity();
            return Ok(());
        }
        if force_ap {
            self.stop_portal();
        }
        eprintln!(
            "[yasumu-daemon] starting Parent Portal on {} (force_ap={})",
            self.portal_bind, force_ap
        );
        let handle =
            PortalHandle::start(self.ctx.clone(), self.portal_bind, self.dev_mode, force_ap)?;
        let mut guard = self.portal.lock().unwrap();
        if guard.is_none() {
            *guard = Some(handle);
        }
        eprintln!("[yasumu-daemon] Parent Portal started successfully");
        Ok(())
    }

    pub fn stop_portal(&self) {
        let taken = self.portal.lock().unwrap().take();
        if let Some(mut handle) = taken {
            eprintln!("[yasumu-daemon] stopping Parent Portal");
            handle.stop();
        }
    }

    pub fn build_status(&self) -> DaemonStatus {
        let portal_active = self.portal.lock().unwrap().is_some();
        let mut guard = self.ctx.lock().unwrap();
        let local = guard.time.local_now(&guard.store.config.timezone);
        let eval = guard.store.evaluate(&local, false);
        let all_games = scan_roms(&guard.rom_roots, &guard.store.config, &guard.catalogue_roots);
        let allowed_games = all_games.into_iter().filter(|g| g.allowed).collect();
        let (battery_percent, battery_charging) = read_battery_status();

        DaemonStatus {
            lock_status: eval.lock_status,
            quota_enabled: guard.store.config.quota_enabled,
            bedtime_enabled: guard.store.config.bedtime.enabled,
            effective_remaining_seconds: eval.effective_remaining_seconds,
            quota_remaining_seconds: eval.quota_remaining_seconds,
            played_seconds_today: eval.played_seconds_today,
            has_pin: guard.store.config.has_pin(),
            pin_lockout_remaining_seconds: guard.store.pin_lockout_remaining(local.epoch_secs),
            portal_active,
            portal_mode: guard.portal_mode.clone(),
            portal_url: guard.portal_url.clone(),
            wifi_ssid: guard.store.config.wifi_ssid.clone(),
            wifi_passphrase: guard.store.config.wifi_passphrase.clone(),
            home_wifi_ssid: guard.store.config.home_wifi_ssid.clone(),
            language: guard.store.config.language.clone(),
            exit_to_muos_requested: guard.exit_to_muos_requested,
            battery_percent,
            battery_charging,
            allowed_games,
        }
    }

    pub fn handle_ipc_request(&self, req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::GetStatus => IpcResponse::Status {
                status: self.build_status(),
            },
            IpcRequest::VerifyPinAndStartPortal { pin } => {
                let res = {
                    let mut guard = self.ctx.lock().unwrap();
                    let now = guard.time.wall_clock_secs();
                    if !guard.store.config.has_pin() {
                        Ok(true)
                    } else {
                        guard.store.attempt_pin(pin.trim(), now)
                    }
                };

                match res {
                    Ok(true) => {
                        if let Err(e) = self.start_portal() {
                            return IpcResponse::Error {
                                message: format!("failed to start portal: {}", e),
                            };
                        }
                        IpcResponse::PinResult {
                            unlocked: true,
                            lockout_seconds: 0,
                            status: self.build_status(),
                        }
                    }
                    Ok(false) => IpcResponse::PinResult {
                        unlocked: false,
                        lockout_seconds: 0,
                        status: self.build_status(),
                    },
                    Err(lockout_seconds) => IpcResponse::PinResult {
                        unlocked: false,
                        lockout_seconds,
                        status: self.build_status(),
                    },
                }
            }
            IpcRequest::StartPortalFirstBoot => {
                let has_pin = self.ctx.lock().unwrap().store.config.has_pin();
                if has_pin {
                    return IpcResponse::Error {
                        message: "PIN is already set; use VerifyPinAndStartPortal".to_string(),
                    };
                }
                if let Err(e) = self.start_portal() {
                    return IpcResponse::Error {
                        message: format!("failed to start portal: {}", e),
                    };
                }
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
            IpcRequest::ForceOfflineAp => {
                if let Err(e) = self.start_portal_with_mode(true) {
                    return IpcResponse::Error {
                        message: format!("failed to start offline AP: {}", e),
                    };
                }
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
            IpcRequest::StopPortal => {
                self.stop_portal();
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
            IpcRequest::PrepareGameLaunch { game_id } => {
                let status = self.build_status();
                if status.lock_status != LockStatus::Unlocked {
                    return IpcResponse::Error {
                        message: "Device is currently locked".to_string(),
                    };
                }
                match status.allowed_games.into_iter().find(|g| g.id == game_id) {
                    Some(game) => {
                        *self.active_game_id.lock().unwrap() = Some(game.id.clone());
                        IpcResponse::LaunchApproved { game }
                    }
                    None => IpcResponse::Error {
                        message: format!("Game '{}' is not on the allowlist", game_id),
                    },
                }
            }
            IpcRequest::NotifyGamePid { pid } => {
                self.retroarch.set_tracked_pid(pid);
                if pid.is_none() {
                    *self.active_game_id.lock().unwrap() = None;
                }
                self.tick();
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
            IpcRequest::RequestExitToMuos => {
                {
                    let mut guard = self.ctx.lock().unwrap();
                    guard.exit_to_muos_requested = true;
                }
                self.stop_portal();
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
            IpcRequest::RequestPowerOff => {
                {
                    let mut guard = self.ctx.lock().unwrap();
                    let now = guard.time.wall_clock_secs();
                    let _ = guard.store.save_usage(now);
                }
                self.stop_portal();
                if !self.dev_mode {
                    std::thread::spawn(|| {
                        let _ = std::process::Command::new("sync").status();
                        std::thread::sleep(Duration::from_millis(200));
                        if std::path::Path::new("/opt/muos/script/system/halt.sh").is_file() {
                            let _ = std::process::Command::new("/opt/muos/script/system/halt.sh")
                                .arg("poweroff")
                                .status();
                        }
                        let _ = std::process::Command::new("poweroff").status();
                    });
                }
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
            IpcRequest::DevAdvanceTime {
                wall_delta_sec,
                monotonic_delta_sec,
            } => {
                {
                    let guard = self.ctx.lock().unwrap();
                    guard.time.advance(wall_delta_sec, monotonic_delta_sec);
                }
                self.tick();
                IpcResponse::Ok {
                    status: self.build_status(),
                }
            }
        }
    }
}

pub fn run_daemon(cfg: DaemonConfig, stop_flag: Option<Arc<AtomicBool>>) -> Result<()> {
    let time = TimeProvider::from_env();
    let runtime = Arc::new(DaemonRuntime::new(&cfg, time)?);
    if cfg.start_portal_on_boot {
        runtime.start_portal()?;
    }

    if cfg.socket_path.exists() {
        let _ = fs::remove_file(&cfg.socket_path);
    }
    if let Some(parent) = cfg.socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&cfg.socket_path)
        .with_context(|| format!("bind Unix socket {}", cfg.socket_path.display()))?;
    listener.set_nonblocking(true)?;

    let stop = stop_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let stop_watchdog = stop.clone();
    let rt_watchdog = runtime.clone();

    // Spawn 2-second watchdog thread
    let watchdog_handle = thread::spawn(move || {
        while !stop_watchdog.load(Ordering::Relaxed) {
            rt_watchdog.tick();
            for _ in 0..20 {
                if stop_watchdog.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    });

    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let rt_client = runtime.clone();
                thread::spawn(move || {
                    let _ = handle_client_stream(stream, &rt_client);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    runtime.stop_portal();
    let _ = watchdog_handle.join();
    let _ = fs::remove_file(&cfg.socket_path);
    Ok(())
}

fn handle_client_stream(mut stream: UnixStream, runtime: &DaemonRuntime) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(());
    }

    let resp = match serde_json::from_str::<IpcRequest>(line.trim()) {
        Ok(req) => runtime.handle_ipc_request(req),
        Err(e) => IpcResponse::Error {
            message: format!("invalid IPC JSON: {}", e),
        },
    };

    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    stream.write_all(out.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retroarch::run_mock_retroarch;
    use std::net::UdpSocket;

    #[test]
    fn test_daemon_watchdog_warnings_and_quit_over_udp() {
        let tmp = tempfile::tempdir().unwrap();
        let roms_dir = tmp.path().join("ROMS/GBA");
        fs::create_dir_all(&roms_dir).unwrap();
        fs::write(roms_dir.join(" Kirby (USA).gba"), b"rom").unwrap();

        // Find free UDP port for mock RetroArch
        let udp_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_addr = udp_sock.local_addr().unwrap();
        drop(udp_sock);

        let log_file = tmp.path().join("udp.log");
        let log_clone = log_file.clone();
        let mock_thread = thread::spawn(move || {
            run_mock_retroarch(udp_addr, Some(&log_clone)).unwrap();
        });
        thread::sleep(Duration::from_millis(30));

        // 2026-11-18 14:00:00 UTC (Wednesday, 60m default quota)
        let t0 = 1795010400i64;
        let time = TimeProvider::new_mock(t0, 1.0);

        let cfg = DaemonConfig {
            data_dir: tmp.path().join("parental"),
            socket_path: tmp.path().join("yasumu.sock"),
            rom_roots: vec![tmp.path().join("ROMS")],
            catalogue_roots: vec![],
            retroarch_udp: udp_addr,
            portal_bind: "127.0.0.1:0".parse().unwrap(),
            dev_mode: true,
            scan_proc_for_retroarch: false,
            start_portal_on_boot: false,
        };

        let rt = DaemonRuntime::new(&cfg, time).unwrap();
        // Register our own process PID as the active game so `is_retroarch_running()` returns true
        rt.retroarch.set_tracked_pid(Some(std::process::id()));

        // Advance 55 minutes -> 5m remaining -> should send "5 minutes left!"
        rt.handle_ipc_request(IpcRequest::DevAdvanceTime {
            wall_delta_sec: 55 * 60,
            monotonic_delta_sec: 55 * 60,
        });

        // Advance 4 minutes -> 1m remaining -> should send "1 minute left!"
        rt.handle_ipc_request(IpcRequest::DevAdvanceTime {
            wall_delta_sec: 4 * 60,
            monotonic_delta_sec: 4 * 60,
        });

        // Clear tracked PID before the final 0m QUIT so we don't SIGTERM our own test runner process!
        // Instead, use a child sleep process for the final tick.
        let mut child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .unwrap();
        rt.retroarch.set_tracked_pid(Some(child.id()));

        // Advance final 60 seconds -> 0m remaining -> sends QUIT over UDP and SIGTERMs child
        rt.handle_ipc_request(IpcRequest::DevAdvanceTime {
            wall_delta_sec: 60,
            monotonic_delta_sec: 60,
        });
        let _ = child.wait();

        mock_thread.join().unwrap();
        let udp_log = fs::read_to_string(&log_file).unwrap();
        let lines: Vec<&str> = udp_log.lines().collect();
        assert_eq!(
            lines,
            vec![
                "SHOW_MSG 5 minutes left!",
                "SHOW_MSG 1 minute left!",
                "QUIT"
            ]
        );

        let st = rt.build_status();
        assert_eq!(st.lock_status, LockStatus::QuotaReached);
        assert_eq!(st.effective_remaining_seconds, 0);
    }
}
