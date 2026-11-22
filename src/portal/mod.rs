//! Companion phone Web Portal HTTP API and embedded single-page UI.

mod media;
mod network;

pub use network::PortalHandle;

use crate::roms::{clean_rom_title, scan_roms, DiscoveredGame};
use media::{count_screenshots, handle_delete_screenshot, handle_get_screenshot, handle_list_screenshots, handle_upload_rom};
use crate::state::{BedtimeConfig, GameRule, LockStatus, StateStore, WeeklyQuotas};
use crate::time_provider::TimeProvider;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

const PORTAL_HTML: &str = include_str!("../portal_ui.html");
const PORTAL_CSS: &str = include_str!("../portal_ui.css");
const PORTAL_I18N: &str = include_str!("../portal_i18n.js");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryDayEntry {
    pub date: String,
    pub seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopGameEntry {
    pub id: String,
    pub title: String,
    pub system: String,
    pub seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotEntry {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotsPageResponse {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<ScreenshotEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalStateResponse {
    pub version: String,
    pub has_pin: bool,
    pub timezone: String,
    pub language: String,
    pub portal_mode: String,
    pub home_wifi_ssid: Option<String>,
    pub lock_status: LockStatus,
    pub played_seconds_today: u64,
    pub quota_remaining_seconds: u64,
    pub effective_remaining_seconds: u64,
    pub quota_enabled: bool,
    pub logging_enabled: bool,
    pub weekly_quotas: WeeklyQuotas,
    pub bedtime: BedtimeConfig,
    pub games: Vec<DiscoveredGame>,
    pub history_7d: Vec<HistoryDayEntry>,
    pub top_games: Vec<TopGameEntry>,
    pub screenshot_count: usize,
    pub exit_to_muos_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLanguageRequest {
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWifiRequest {
    pub ssid: Option<String>,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTimeRequest {
    pub timestamp_ms: i64,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameUpdateItem {
    pub id: String,
    pub allowed: bool,
    pub custom_title: Option<String>,
    pub core_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateGamesRequest {
    pub games: Vec<GameUpdateItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateScheduleRequest {
    #[serde(default)]
    pub quota_enabled: Option<bool>,
    pub weekly_quotas: WeeklyQuotas,
    pub bedtime: BedtimeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonusRequest {
    pub minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePinRequest {
    pub pin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLoggingRequest {
    pub enabled: bool,
}

pub struct SharedDaemonContext {
    pub store: StateStore,
    pub time: TimeProvider,
    pub rom_roots: Vec<PathBuf>,
    pub catalogue_roots: Vec<PathBuf>,
    pub dev_mode: bool,
    pub portal_mode: String,
    pub portal_url: String,
    pub last_portal_activity_monotonic: u64,
    pub exit_to_muos_requested: bool,
}

impl SharedDaemonContext {
    pub fn touch_activity(&mut self) {
        self.last_portal_activity_monotonic = self.time.monotonic_secs();
    }

    pub fn build_portal_state(&mut self) -> PortalStateResponse {
        let local = self.time.local_now(&self.store.config.timezone);
        let eval = self.store.evaluate(&local, false);
        let games = scan_roms(&self.rom_roots, &self.store.config, &self.catalogue_roots);
        let history_7d = self
            .store
            .last_7_days_history(&local)
            .into_iter()
            .map(|(date, seconds)| HistoryDayEntry { date, seconds })
            .collect();

        let mut top_games: Vec<TopGameEntry> = self
            .store
            .usage
            .game_play_seconds
            .iter()
            .filter(|(_, &secs)| secs > 0)
            .map(|(id, &seconds)| {
                if let Some(g) = games.iter().find(|g| g.id == *id) {
                    TopGameEntry {
                        id: id.clone(),
                        title: g.display_title.clone(),
                        system: g.system_badge.clone(),
                        seconds,
                    }
                } else {
                    let (sys, fname) = id.split_once('/').unwrap_or(("", id.as_str()));
                    TopGameEntry {
                        id: id.clone(),
                        title: clean_rom_title(fname),
                        system: sys.to_string(),
                        seconds,
                    }
                }
            })
            .collect();
        top_games.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.title.cmp(&b.title)));
        top_games.truncate(5);

        let screenshot_count = count_screenshots(&self.store.data_dir, self.dev_mode);

        PortalStateResponse {
            version: crate::APP_VERSION.to_string(),
            has_pin: self.store.config.has_pin(),
            timezone: self.store.config.timezone.clone(),
            language: self.store.config.language.clone(),
            portal_mode: self.portal_mode.clone(),
            home_wifi_ssid: self.store.config.home_wifi_ssid.clone(),
            lock_status: eval.lock_status,
            played_seconds_today: eval.played_seconds_today,
            quota_remaining_seconds: eval.quota_remaining_seconds,
            effective_remaining_seconds: eval.effective_remaining_seconds,
            quota_enabled: self.store.config.quota_enabled,
            logging_enabled: self.store.config.logging_enabled,
            weekly_quotas: self.store.config.weekly_quotas.clone(),
            bedtime: self.store.config.bedtime.clone(),
            games,
            history_7d,
            top_games,
            screenshot_count,
            exit_to_muos_requested: self.exit_to_muos_requested,
        }
    }
}

pub type SharedCtx = Arc<Mutex<SharedDaemonContext>>;

pub fn build_portal_router(ctx: SharedCtx) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/api/state", get(handle_get_state))
        .route("/api/sync-time", post(handle_sync_time))
        .route("/api/games", post(handle_update_games))
        .route("/api/schedule", post(handle_update_schedule))
        .route("/api/bonus", post(handle_grant_bonus))
        .route("/api/pin", post(handle_update_pin))
        .route("/api/language", post(handle_update_language))
        .route("/api/logging", post(handle_update_logging))
        .route("/api/wifi", post(handle_update_wifi))
        .route("/api/scrape-covers", post(handle_scrape_covers))
        .route(
            "/api/upload-rom",
            post(handle_upload_rom).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/api/screenshots", get(handle_list_screenshots))
        .route(
            "/api/screenshots/:name",
            get(handle_get_screenshot).delete(handle_delete_screenshot),
        )
        .route("/api/exit-muos", post(handle_exit_muos))
        .route("/api/poweroff", post(handle_poweroff))
        .route("/api/uninstall", post(handle_uninstall))
        .route("/api/dev/seed-stats", post(handle_dev_seed_stats))
        .fallback(handle_captive_redirect)
        .with_state(ctx)
}

async fn handle_index(State(ctx): State<SharedCtx>) -> impl IntoResponse {
    let initial_json = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        let st = guard.build_portal_state();
        serde_json::to_string(&st)
            .unwrap_or_else(|_| "null".to_string())
            .replace("</", "<\\/")
    };
    eprintln!("[yasumu-portal] GET / served");
    let html = PORTAL_HTML
        .replace("/*__PORTAL_CSS__*/", PORTAL_CSS)
        .replace("/*__PORTAL_I18N__*/", PORTAL_I18N)
        .replace("/*__INITIAL_STATE__*/null", &initial_json);
    Html(html)
}

async fn handle_captive_redirect(State(ctx): State<SharedCtx>) -> Response {
    let target = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        let base = guard.portal_url.trim_end_matches('/').to_string();
        format!("{}/", base)
    };

    (
        StatusCode::FOUND,
        [(header::LOCATION, target.as_str())],
        "Redirecting to yasumuOS Web Portal...",
    )
        .into_response()
}

async fn handle_get_state(State(ctx): State<SharedCtx>) -> Json<PortalStateResponse> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    Json(guard.build_portal_state())
}

async fn handle_sync_time(
    State(ctx): State<SharedCtx>,
    Json(req): Json<SyncTimeRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let new_epoch_secs = req.timestamp_ms / 1000;
    if new_epoch_secs <= 0 {
        return Err((StatusCode::BAD_REQUEST, "invalid timestamp_ms".to_string()));
    }

    let dev_mode = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        guard.time.set_wall_clock_secs(new_epoch_secs);
        guard
            .store
            .sync_time_and_clear_locks(new_epoch_secs, req.timezone.as_deref())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        guard.dev_mode
    };

    // On real RG35XXSP hardware (non-dev mode), also sync Linux kernel clock + RTC via hwclock -w
    if !dev_mode && unsafe { libc::geteuid() == 0 } {
        let _ = Command::new("date")
            .args(["-u", "-s", &format!("@{}", new_epoch_secs)])
            .status();
        let _ = Command::new("hwclock").arg("-w").status();
    }

    let mut guard = ctx.lock().unwrap();
    Ok(Json(guard.build_portal_state()))
}

async fn handle_update_games(
    State(ctx): State<SharedCtx>,
    Json(req): Json<UpdateGamesRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    for item in req.games {
        guard.store.config.games.insert(
            item.id,
            GameRule {
                allowed: item.allowed,
                custom_title: item.custom_title.filter(|s| !s.trim().is_empty()),
                core_override: item.core_override.filter(|s| !s.trim().is_empty()),
            },
        );
    }
    let now = guard.time.wall_clock_secs();
    guard
        .store
        .save_all(now)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_update_schedule(
    State(ctx): State<SharedCtx>,
    Json(req): Json<UpdateScheduleRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    if let Some(qe) = req.quota_enabled {
        guard.store.config.quota_enabled = qe;
    }
    guard.store.config.weekly_quotas = req.weekly_quotas.clamped();
    guard.store.config.bedtime = req.bedtime;
    let now = guard.time.wall_clock_secs();
    guard
        .store
        .save_all(now)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_grant_bonus(
    State(ctx): State<SharedCtx>,
    Json(req): Json<BonusRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    let local = guard.time.local_now(&guard.store.config.timezone);
    guard
        .store
        .grant_bonus_minutes(req.minutes.max(1).min(240), &local);
    guard
        .store
        .save_usage(local.epoch_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_update_pin(
    State(ctx): State<SharedCtx>,
    Json(req): Json<UpdatePinRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    guard
        .store
        .config
        .set_pin(req.pin.trim())
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    guard.store.usage.failed_pin_attempts = 0;
    guard.store.usage.pin_lockout_until = 0;
    guard.store.tamper_error = None;
    let now = guard.time.wall_clock_secs();
    guard
        .store
        .save_all(now)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_update_language(
    State(ctx): State<SharedCtx>,
    Json(req): Json<UpdateLanguageRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    let lang = crate::i18n::Language::from_code(&req.language);
    guard.store.config.language = lang.code().to_string();
    let now = guard.time.wall_clock_secs();
    guard
        .store
        .save_all(now)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_update_logging(
    State(ctx): State<SharedCtx>,
    Json(req): Json<UpdateLoggingRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    guard.store.config.logging_enabled = req.enabled;
    let flag_path = guard.store.data_dir.join("logging_enabled");
    if req.enabled {
        let _ = std::fs::write(&flag_path, "1\n");
    } else {
        let _ = std::fs::remove_file(&flag_path);
    }
    let now = guard.time.wall_clock_secs();
    guard
        .store
        .save_all(now)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_update_wifi(
    State(ctx): State<SharedCtx>,
    Json(req): Json<UpdateWifiRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    let ssid = req.ssid.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let pass = req
        .passphrase
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    guard.store.config.home_wifi_ssid = ssid;
    if guard.store.config.home_wifi_ssid.is_none() {
        guard.store.config.home_wifi_passphrase = None;
    } else if pass.is_some() {
        guard.store.config.home_wifi_passphrase = pass;
    }
    let now = guard.time.wall_clock_secs();
    guard
        .store
        .save_all(now)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_scrape_covers(
    State(ctx): State<SharedCtx>,
) -> Result<Json<crate::scraper::ScrapeReport>, (StatusCode, String)> {
    let games = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        scan_roms(&guard.rom_roots, &guard.store.config, &guard.catalogue_roots)
    };

    let report = tokio::task::spawn_blocking(move || {
        crate::scraper::scrape_missing_covers(&games)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
    }
    Ok(Json(report))
}

async fn handle_exit_muos(
    State(ctx): State<SharedCtx>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    guard.exit_to_muos_requested = true;
    Ok(Json(guard.build_portal_state()))
}

async fn handle_poweroff(
    State(ctx): State<SharedCtx>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let dev_mode = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        let now = guard.time.wall_clock_secs();
        let _ = guard.store.save_usage(now);
        // If PortalHandle isn't running AP scripts in dev_mode, skip real OS poweroff
        guard.portal_url.starts_with("http://127.0.0.1")
    };
    if !dev_mode {
        std::thread::spawn(|| {
            let _ = std::process::Command::new("sync").status();
            std::thread::sleep(std::time::Duration::from_millis(250));
            if std::path::Path::new("/opt/muos/script/system/halt.sh").is_file() {
                let _ = std::process::Command::new("/opt/muos/script/system/halt.sh")
                    .arg("poweroff")
                    .status();
            }
            let _ = std::process::Command::new("poweroff").status();
        });
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn handle_uninstall(
    State(ctx): State<SharedCtx>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let dev_mode = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        guard.portal_url.starts_with("http://127.0.0.1")
    };
    if !dev_mode {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(350));
            let script = std::path::Path::new("/opt/yasumu/uninstall.sh");
            let tmp_script = std::path::Path::new("/tmp/yasumu-uninstall.sh");
            if script.is_file() && std::fs::copy(script, tmp_script).is_ok() {
                let _ = std::process::Command::new("/bin/sh")
                    .arg(tmp_script)
                    .arg("--reboot")
                    .status();
            }
        });
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DevSeedStatsRequest {
    played_seconds_today: u64,
    history: std::collections::BTreeMap<String, u64>,
    game_play_seconds: std::collections::BTreeMap<String, u64>,
}

async fn handle_dev_seed_stats(
    State(ctx): State<SharedCtx>,
    Json(req): Json<DevSeedStatsRequest>,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    let mut guard = ctx.lock().unwrap();
    if !guard.dev_mode {
        return Err((StatusCode::FORBIDDEN, "dev mode only".to_string()));
    }
    let local = guard.time.local_now(&guard.store.config.timezone);
    guard.store.usage.current_date = local.date_str.clone();
    guard.store.usage.played_seconds_today = req.played_seconds_today;
    guard.store.usage.history = req.history;
    guard.store.usage.game_play_seconds = req.game_play_seconds;
    guard
        .store
        .save_usage(local.epoch_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

