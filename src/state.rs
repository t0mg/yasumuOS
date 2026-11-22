use crate::crypto::{
    generate_salt, generate_wifi_passphrase, hash_pin, sign_payload, verify_and_deserialize,
    verify_pin,
};
use crate::time_provider::{parse_hhmm_to_minutes, LocalTimeInfo};
use anyhow::{bail, Context, Result};
use chrono::{Duration, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Per-day play-time quota in minutes (Monday through Sunday).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyQuotas {
    pub mon: u32,
    pub tue: u32,
    pub wed: u32,
    pub thu: u32,
    pub fri: u32,
    pub sat: u32,
    pub sun: u32,
}

impl Default for WeeklyQuotas {
    fn default() -> Self {
        Self {
            mon: 60,
            tue: 60,
            wed: 60,
            thu: 60,
            fri: 60,
            sat: 90,
            sun: 90,
        }
    }
}

/// Maximum configurable daily play-time quota in minutes (4 hours).
pub const MAX_DAILY_QUOTA_MINUTES: u32 = 240;

impl WeeklyQuotas {
    pub fn clamped(&self) -> Self {
        Self {
            mon: self.mon.min(MAX_DAILY_QUOTA_MINUTES),
            tue: self.tue.min(MAX_DAILY_QUOTA_MINUTES),
            wed: self.wed.min(MAX_DAILY_QUOTA_MINUTES),
            thu: self.thu.min(MAX_DAILY_QUOTA_MINUTES),
            fri: self.fri.min(MAX_DAILY_QUOTA_MINUTES),
            sat: self.sat.min(MAX_DAILY_QUOTA_MINUTES),
            sun: self.sun.min(MAX_DAILY_QUOTA_MINUTES),
        }
    }

    pub fn quota_for_weekday(&self, weekday: Weekday) -> u32 {
        match weekday {
            Weekday::Mon => self.mon,
            Weekday::Tue => self.tue,
            Weekday::Wed => self.wed,
            Weekday::Thu => self.thu,
            Weekday::Fri => self.fri,
            Weekday::Sat => self.sat,
            Weekday::Sun => self.sun,
        }
    }
}

/// Global bedtime window (`wake_time` and `bed_time` in `"HH:MM"` format).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BedtimeConfig {
    pub enabled: bool,
    pub wake_time: String,
    pub bed_time: String,
}

impl Default for BedtimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            wake_time: "07:00".to_string(),
            bed_time: "20:00".to_string(),
        }
    }
}

/// Per-game allowlist and customization settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GameRule {
    pub allowed: bool,
    pub custom_title: Option<String>,
    pub core_override: Option<String>,
}

fn default_language() -> String {
    "en".to_string()
}

fn default_true() -> bool {
    true
}

/// Signed persistent parent configuration (`/opt/parental/config.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentalConfig {
    pub pin_salt: Option<String>,
    pub pin_hash: Option<String>,
    pub wifi_ssid: String,
    pub wifi_passphrase: String,
    #[serde(default)]
    pub home_wifi_ssid: Option<String>,
    #[serde(default)]
    pub home_wifi_passphrase: Option<String>,
    pub timezone: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_true")]
    pub quota_enabled: bool,
    #[serde(default)]
    pub logging_enabled: bool,
    pub weekly_quotas: WeeklyQuotas,
    pub bedtime: BedtimeConfig,
    pub games: BTreeMap<String, GameRule>,
}

impl Default for ParentalConfig {
    fn default() -> Self {
        Self {
            pin_salt: None,
            pin_hash: None,
            wifi_ssid: "Yasumu".to_string(),
            wifi_passphrase: generate_wifi_passphrase(),
            home_wifi_ssid: None,
            home_wifi_passphrase: None,
            timezone: "UTC".to_string(),
            language: default_language(),
            quota_enabled: true,
            logging_enabled: false,
            weekly_quotas: WeeklyQuotas::default(),
            bedtime: BedtimeConfig::default(),
            games: BTreeMap::new(),
        }
    }
}

impl ParentalConfig {
    pub fn has_pin(&self) -> bool {
        self.pin_salt.is_some() && self.pin_hash.is_some()
    }

    pub fn set_pin(&mut self, pin: &str) -> Result<()> {
        if pin.len() != 4 || !pin.chars().all(|c| c.is_ascii_digit()) {
            bail!("PIN must be exactly 4 numeric digits");
        }
        let salt = generate_salt();
        let hashed = hash_pin(pin, &salt);
        self.pin_salt = Some(salt);
        self.pin_hash = Some(hashed);
        Ok(())
    }

    pub fn clear_pin(&mut self) {
        self.pin_salt = None;
        self.pin_hash = None;
    }

    pub fn check_pin(&self, candidate: &str) -> bool {
        match (&self.pin_salt, &self.pin_hash) {
            (Some(salt), Some(hash)) => verify_pin(candidate, salt, hash),
            _ => false,
        }
    }
}

/// Signed persistent usage and tamper state (`/opt/parental/usage.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageState {
    pub last_seen_timestamp: i64,
    pub current_date: String,
    pub played_seconds_today: u64,
    pub bonus_minutes_today: u32,
    pub bedtime_snooze_until_epoch: Option<i64>,
    pub warned_5m_today: bool,
    pub warned_1m_today: bool,
    pub failed_pin_attempts: u32,
    pub pin_lockout_until: i64,
    pub history: BTreeMap<String, u64>,
    #[serde(default)]
    pub game_play_seconds: BTreeMap<String, u64>,
}

impl Default for UsageState {
    fn default() -> Self {
        Self {
            last_seen_timestamp: 0,
            current_date: String::new(),
            played_seconds_today: 0,
            bonus_minutes_today: 0,
            bedtime_snooze_until_epoch: None,
            warned_5m_today: false,
            warned_1m_today: false,
            failed_pin_attempts: 0,
            pin_lockout_until: 0,
            history: BTreeMap::new(),
            game_play_seconds: BTreeMap::new(),
        }
    }
}

/// High-level lock status displayed by `yasumu launcher` and enforced by `yasumu daemon`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LockStatus {
    Unlocked,
    QuotaReached,
    BedtimeActive { wake_time: String },
    ClockSyncRequired,
    TamperLocked { reason: String },
}

/// Evaluation of current quota, bedtime, and warnings at a specific wall-clock timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayEvaluation {
    pub lock_status: LockStatus,
    pub quota_total_seconds: u64,
    pub played_seconds_today: u64,
    pub quota_remaining_seconds: u64,
    pub seconds_until_bedtime: Option<u64>,
    pub effective_remaining_seconds: u64,
    pub trigger_5m_warning: bool,
    pub trigger_1m_warning: bool,
}

/// Manages `config.json` and `usage.json` on disk with HMAC-SHA256 verification.
pub struct StateStore {
    pub data_dir: PathBuf,
    pub config: ParentalConfig,
    pub usage: UsageState,
    pub tamper_error: Option<String>,
    pub clock_rollback_detected: bool,
}

impl StateStore {
    pub fn config_path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.json")
    }

    pub fn usage_path(data_dir: &Path) -> PathBuf {
        data_dir.join("usage.json")
    }

    /// Loads or initializes `config.json` and `usage.json` from `data_dir`.
    /// Fails closed (`tamper_error = Some(...)`) if files are modified or if `usage.json` is deleted after `config.json` exists.
    pub fn load_or_init(data_dir: &Path, rom_roots: &[PathBuf], now_epoch: i64) -> Result<Self> {
        fs::create_dir_all(data_dir)
            .with_context(|| format!("create data dir {}", data_dir.display()))?;

        let cfg_path = Self::config_path(data_dir);
        let usg_path = Self::usage_path(data_dir);

        let mut tamper_error: Option<String> = None;
        let cfg_exists = cfg_path.exists();
        let usg_exists = usg_path.exists();

        let mut config = if cfg_exists {
            match fs::read_to_string(&cfg_path)
                .map_err(anyhow::Error::from)
                .and_then(|raw| verify_and_deserialize::<ParentalConfig>(&raw))
            {
                Ok(c) => c,
                Err(e) => {
                    tamper_error = Some(format!("config.json signature invalid: {}", e));
                    ParentalConfig::default()
                }
            }
        } else {
            ParentalConfig::default()
        };

        let mut usage = if usg_exists {
            match fs::read_to_string(&usg_path)
                .map_err(anyhow::Error::from)
                .and_then(|raw| verify_and_deserialize::<UsageState>(&raw))
            {
                Ok(u) => u,
                Err(e) => {
                    tamper_error = Some(format!("usage.json signature invalid: {}", e));
                    UsageState::default()
                }
            }
        } else if cfg_exists && config.has_pin() {
            // Fail-closed: config.json exists with a PIN, but usage.json was deleted!
            tamper_error = Some("usage.json missing (deleted or corrupted)".to_string());
            UsageState::default()
        } else {
            let local = crate::time_provider::local_time_from_epoch(now_epoch, &config.timezone);
            UsageState {
                last_seen_timestamp: now_epoch,
                current_date: local.date_str,
                ..UsageState::default()
            }
        };

        // Check Forgot-PIN escape hatch (`RESET_PIN` on any ROMs root or parent SD mount)
        if Self::check_and_consume_reset_pin(rom_roots) {
            config.clear_pin();
            usage.failed_pin_attempts = 0;
            usage.pin_lockout_until = 0;
        }

        // Check clock rollback (`now_epoch < usage.last_seen_timestamp`)
        // Allow up to 5 seconds of minor drift; any larger backwards jump triggers ClockSyncRequired.
        let clock_rollback_detected =
            usage.last_seen_timestamp > 0 && (now_epoch + 5) < usage.last_seen_timestamp;

        let mut store = Self {
            data_dir: data_dir.to_path_buf(),
            config,
            usage,
            tamper_error,
            clock_rollback_detected,
        };

        // On clean first boot, persist the initial signed files right away.
        if !cfg_exists && !usg_exists && store.tamper_error.is_none() {
            store.save_all(now_epoch)?;
        } else if cfg_exists && store.tamper_error.is_none() {
            // Persist if RESET_PIN cleared the PIN
            store.save_config()?;
        }

        Ok(store)
    }

    fn check_and_consume_reset_pin(rom_roots: &[PathBuf]) -> bool {
        let mut found = false;
        let mut candidates: Vec<PathBuf> = Vec::new();
        for root in rom_roots {
            candidates.push(root.join("RESET_PIN"));
            if let Some(parent) = root.parent() {
                candidates.push(parent.join("RESET_PIN"));
            }
        }
        candidates.push(PathBuf::from("/mnt/mmc/RESET_PIN"));
        candidates.push(PathBuf::from("/mnt/sdcard/RESET_PIN"));

        for path in candidates {
            if path.is_file() {
                let _ = fs::remove_file(&path);
                found = true;
            }
        }
        found
    }

    pub fn save_config(&self) -> Result<()> {
        let signed = sign_payload(&self.config)?;
        let json = serde_json::to_string_pretty(&signed)?;
        let tmp_path = self.data_dir.join("config.json.tmp");
        fs::write(&tmp_path, json)?;
        fs::rename(&tmp_path, Self::config_path(&self.data_dir))?;
        Ok(())
    }

    pub fn save_usage(&mut self, now_epoch: i64) -> Result<()> {
        if !self.clock_rollback_detected && now_epoch > self.usage.last_seen_timestamp {
            self.usage.last_seen_timestamp = now_epoch;
        }
        let signed = sign_payload(&self.usage)?;
        let json = serde_json::to_string_pretty(&signed)?;
        let tmp_path = self.data_dir.join("usage.json.tmp");
        fs::write(&tmp_path, json)?;
        fs::rename(&tmp_path, Self::usage_path(&self.data_dir))?;
        Ok(())
    }

    pub fn save_all(&mut self, now_epoch: i64) -> Result<()> {
        self.save_config()?;
        self.save_usage(now_epoch)?;
        Ok(())
    }

    /// Clears tamper and clock-rollback flags after parent authenticates or syncs time via portal.
    pub fn sync_time_and_clear_locks(
        &mut self,
        new_epoch_secs: i64,
        new_timezone: Option<&str>,
    ) -> Result<()> {
        if let Some(tz) = new_timezone {
            if !tz.trim().is_empty() && tz.parse::<chrono_tz::Tz>().is_ok() {
                self.config.timezone = tz.trim().to_string();
            }
        }
        self.usage.last_seen_timestamp = new_epoch_secs;
        self.clock_rollback_detected = false;
        self.tamper_error = None;
        let local =
            crate::time_provider::local_time_from_epoch(new_epoch_secs, &self.config.timezone);
        self.rollover_date_if_needed(&local);
        self.save_all(new_epoch_secs)?;
        Ok(())
    }

    /// Rolls over `current_date` when local midnight passes, archiving previous day's play time into `history`.
    pub fn rollover_date_if_needed(&mut self, local: &LocalTimeInfo) -> bool {
        if self.usage.current_date == local.date_str {
            // Keep today's entry in history synced
            self.usage
                .history
                .insert(local.date_str.clone(), self.usage.played_seconds_today);
            return false;
        }

        if !self.usage.current_date.is_empty() {
            self.usage.history.insert(
                self.usage.current_date.clone(),
                self.usage.played_seconds_today,
            );
        }

        self.usage.current_date = local.date_str.clone();
        self.usage.played_seconds_today = *self.usage.history.get(&local.date_str).unwrap_or(&0);
        self.usage.bonus_minutes_today = 0;
        self.usage.bedtime_snooze_until_epoch = None;
        self.usage.warned_5m_today = false;
        self.usage.warned_1m_today = false;
        self.usage
            .history
            .insert(local.date_str.clone(), self.usage.played_seconds_today);

        // Trim history to at most 14 days
        while self.usage.history.len() > 14 {
            if let Some(first_key) = self.usage.history.keys().next().cloned() {
                self.usage.history.remove(&first_key);
            }
        }

        true
    }

    /// Adds active gameplay seconds and updates today's history entry.
    pub fn record_play_seconds(&mut self, delta_secs: u64, local: &LocalTimeInfo) {
        if self.clock_rollback_detected || self.tamper_error.is_some() {
            return;
        }
        self.rollover_date_if_needed(local);
        self.usage.played_seconds_today = self.usage.played_seconds_today.saturating_add(delta_secs);
        self.usage
            .history
            .insert(local.date_str.clone(), self.usage.played_seconds_today);
        if local.epoch_secs > self.usage.last_seen_timestamp {
            self.usage.last_seen_timestamp = local.epoch_secs;
        }
    }

    /// Credits cumulative playtime seconds to a specific game ID.
    pub fn record_game_seconds(&mut self, game_id: &str, delta_secs: u64) {
        if delta_secs == 0 || game_id.is_empty() {
            return;
        }
        let entry = self
            .usage
            .game_play_seconds
            .entry(game_id.to_string())
            .or_insert(0);
        *entry = entry.saturating_add(delta_secs);
    }

    /// Grants `+minutes` bonus play time for today.
    /// If currently within `minutes` of bedtime (or during bedtime), also snoozes bedtime by `minutes` from `now`.
    pub fn grant_bonus_minutes(&mut self, minutes: u32, local: &LocalTimeInfo) {
        self.rollover_date_if_needed(local);
        self.usage.bonus_minutes_today = self.usage.bonus_minutes_today.saturating_add(minutes);
        // Reset warning flags so the child gets fresh 5m/1m warnings for the newly extended time
        self.usage.warned_5m_today = false;
        self.usage.warned_1m_today = false;

        let bonus_secs = (minutes as i64) * 60;
        if self.config.bedtime.enabled {
            let (_, secs_until_bed) = self.compute_bedtime_state(local);
            match secs_until_bed {
                Some(rem) if rem <= (minutes as u64) * 60 => {
                    let base = self
                        .usage
                        .bedtime_snooze_until_epoch
                        .unwrap_or(local.epoch_secs + (rem as i64))
                        .max(local.epoch_secs);
                    self.usage.bedtime_snooze_until_epoch = Some(base + bonus_secs);
                }
                None => {
                    // Currently in bedtime; push bedtime cutoff to at least `now + bonus_secs`
                    let base = self
                        .usage
                        .bedtime_snooze_until_epoch
                        .unwrap_or(local.epoch_secs)
                        .max(local.epoch_secs);
                    self.usage.bedtime_snooze_until_epoch = Some(base + bonus_secs);
                }
                _ => {}
            }
        }
    }

    /// Returns `(is_bedtime_active, seconds_until_bedtime_if_awake)`.
    pub fn compute_bedtime_state(&self, local: &LocalTimeInfo) -> (bool, Option<u64>) {
        if !self.config.bedtime.enabled {
            return (false, None);
        }

        // Check if parent snoozed bedtime via bonus time
        if let Some(snooze_until) = self.usage.bedtime_snooze_until_epoch {
            if local.epoch_secs < snooze_until {
                let rem = (snooze_until - local.epoch_secs) as u64;
                return (false, Some(rem));
            }
        }

        let wake_min = parse_hhmm_to_minutes(&self.config.bedtime.wake_time).unwrap_or(7 * 60);
        let bed_min = parse_hhmm_to_minutes(&self.config.bedtime.bed_time).unwrap_or(20 * 60);
        let wake_sec = wake_min * 60;
        let bed_sec = bed_min * 60;
        let now_sec = local.seconds_since_midnight;

        if wake_sec < bed_sec {
            // Normal daytime allowed window: [wake_sec, bed_sec)
            if now_sec < wake_sec || now_sec >= bed_sec {
                (true, None)
            } else {
                (false, Some((bed_sec - now_sec) as u64))
            }
        } else if wake_sec > bed_sec {
            // Overnight allowed window (e.g. wake 10:00, bed 01:00 -> bedtime is [01:00, 10:00))
            if now_sec >= bed_sec && now_sec < wake_sec {
                (true, None)
            } else if now_sec < bed_sec {
                (false, Some((bed_sec - now_sec) as u64))
            } else {
                let until_midnight = 86_400 - now_sec;
                (false, Some((until_midnight + bed_sec) as u64))
            }
        } else {
            (false, None)
        }
    }

    /// Evaluates current lock status, remaining quota/bedtime seconds, and whether 5m/1m OSD warnings should fire.
    pub fn evaluate(&mut self, local: &LocalTimeInfo, consume_warnings: bool) -> PlayEvaluation {
        if let Some(reason) = &self.tamper_error {
            return PlayEvaluation {
                lock_status: LockStatus::TamperLocked {
                    reason: reason.clone(),
                },
                quota_total_seconds: 0,
                played_seconds_today: self.usage.played_seconds_today,
                quota_remaining_seconds: 0,
                seconds_until_bedtime: Some(0),
                effective_remaining_seconds: 0,
                trigger_5m_warning: false,
                trigger_1m_warning: false,
            };
        }

        if (self.usage.last_seen_timestamp > 0
            && (local.epoch_secs + 5) < self.usage.last_seen_timestamp)
            || self.clock_rollback_detected
        {
            self.clock_rollback_detected = true;
            return PlayEvaluation {
                lock_status: LockStatus::ClockSyncRequired,
                quota_total_seconds: 0,
                played_seconds_today: self.usage.played_seconds_today,
                quota_remaining_seconds: 0,
                seconds_until_bedtime: Some(0),
                effective_remaining_seconds: 0,
                trigger_5m_warning: false,
                trigger_1m_warning: false,
            };
        }

        self.rollover_date_if_needed(local);

        let base_quota_min = self.config.weekly_quotas.quota_for_weekday(local.weekday);
        let total_quota_min = base_quota_min.saturating_add(self.usage.bonus_minutes_today);
        let quota_total_seconds = (total_quota_min as u64) * 60;
        let quota_remaining_seconds =
            quota_total_seconds.saturating_sub(self.usage.played_seconds_today);

        let (bedtime_active, seconds_until_bedtime) = self.compute_bedtime_state(local);

        if bedtime_active {
            return PlayEvaluation {
                lock_status: LockStatus::BedtimeActive {
                    wake_time: self.config.bedtime.wake_time.clone(),
                },
                quota_total_seconds,
                played_seconds_today: self.usage.played_seconds_today,
                quota_remaining_seconds,
                seconds_until_bedtime: Some(0),
                effective_remaining_seconds: 0,
                trigger_5m_warning: false,
                trigger_1m_warning: false,
            };
        }

        if self.config.quota_enabled && quota_remaining_seconds == 0 {
            return PlayEvaluation {
                lock_status: LockStatus::QuotaReached,
                quota_total_seconds,
                played_seconds_today: self.usage.played_seconds_today,
                quota_remaining_seconds: 0,
                seconds_until_bedtime,
                effective_remaining_seconds: 0,
                trigger_5m_warning: false,
                trigger_1m_warning: false,
            };
        }

        let effective_remaining_seconds = match (self.config.quota_enabled, seconds_until_bedtime) {
            (true, Some(until_bed)) => quota_remaining_seconds.min(until_bed),
            (true, None) => quota_remaining_seconds,
            (false, Some(until_bed)) => until_bed,
            (false, None) => u64::MAX,
        };

        let mut trigger_5m_warning = false;
        let mut trigger_1m_warning = false;

        if effective_remaining_seconds <= 60 && effective_remaining_seconds > 0 {
            if !self.usage.warned_1m_today {
                trigger_1m_warning = true;
                if consume_warnings {
                    self.usage.warned_1m_today = true;
                    self.usage.warned_5m_today = true;
                }
            }
        } else if effective_remaining_seconds <= 300 && effective_remaining_seconds > 60 {
            if !self.usage.warned_5m_today {
                trigger_5m_warning = true;
                if consume_warnings {
                    self.usage.warned_5m_today = true;
                }
            }
        }

        PlayEvaluation {
            lock_status: LockStatus::Unlocked,
            quota_total_seconds,
            played_seconds_today: self.usage.played_seconds_today,
            quota_remaining_seconds,
            seconds_until_bedtime,
            effective_remaining_seconds,
            trigger_5m_warning,
            trigger_1m_warning,
        }
    }

    /// Returns remaining cooldown seconds if PIN verification is currently locked out.
    pub fn pin_lockout_remaining(&self, now_epoch: i64) -> u64 {
        if self.usage.pin_lockout_until > now_epoch {
            (self.usage.pin_lockout_until - now_epoch) as u64
        } else {
            0
        }
    }

    /// Verifies the parent PIN with persistent brute-force protection:
    /// - 3 failed attempts -> 60s cooldown
    /// - 5+ failed attempts -> 600s (10m) cooldown
    pub fn attempt_pin(&mut self, candidate_pin: &str, now_epoch: i64) -> Result<bool, u64> {
        let rem = self.pin_lockout_remaining(now_epoch);
        if rem > 0 {
            return Err(rem);
        }

        if self.config.check_pin(candidate_pin) {
            self.usage.failed_pin_attempts = 0;
            self.usage.pin_lockout_until = 0;
            let _ = self.save_usage(now_epoch);
            Ok(true)
        } else {
            self.usage.failed_pin_attempts = self.usage.failed_pin_attempts.saturating_add(1);
            let cooldown = if self.usage.failed_pin_attempts >= 5 {
                600
            } else if self.usage.failed_pin_attempts == 3 {
                60
            } else {
                0
            };
            if cooldown > 0 {
                self.usage.pin_lockout_until = now_epoch + cooldown;
            }
            let _ = self.save_usage(now_epoch);
            if cooldown > 0 {
                Err(cooldown as u64)
            } else {
                Ok(false)
            }
        }
    }

    /// Returns the last 7 calendar days (`YYYY-MM-DD`, played_seconds) ending on `local.date`.
    pub fn last_7_days_history(&self, local: &LocalTimeInfo) -> Vec<(String, u64)> {
        let mut out = Vec::with_capacity(7);
        for days_ago in (0..7).rev() {
            let d = local.date - Duration::days(days_ago);
            let key = format!("{:04}-{:02}-{:02}", d.
year(), d.month(), d.day());
            let secs = if key == self.usage.current_date {
                self.usage.played_seconds_today
            } else {
                *self.usage.history.get(&key).unwrap_or(&0)
            };
            out.push((key, secs));
        }
        out
    }
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_provider::local_time_from_epoch;
    use tempfile::tempdir;

    #[test]
    fn test_quota_bedtime_and_warnings() {
        let dir = tempdir().unwrap();
        // 2026-11-18 14:00:00 UTC (Wednesday)
        let t0 = 1795010400i64;
        let mut store = StateStore::load_or_init(dir.path(), &[], t0).unwrap();
        store.config.set_pin("1234").unwrap();
        store.config.weekly_quotas.wed = 30; // 30 minutes on Wednesday
        store.config.bedtime = BedtimeConfig {
            enabled: true,
            wake_time: "07:00".to_string(),
            bed_time: "20:00".to_string(),
        };
        store.save_all(t0).unwrap();

        let local0 = local_time_from_epoch(t0, "UTC");
        let eval0 = store.evaluate(&local0, true);
        assert_eq!(eval0.lock_status, LockStatus::Unlocked);
        assert_eq!(eval0.effective_remaining_seconds, 30 * 60);
        assert!(!eval0.trigger_5m_warning);

        // Play 25 minutes -> 5 minutes remaining -> should fire 5m warning once
        let t1 = t0 + 25 * 60;
        let local1 = local_time_from_epoch(t1, "UTC");
        store.record_play_seconds(25 * 60, &local1);
        let eval1 = store.evaluate(&local1, true);
        assert_eq!(eval1.effective_remaining_seconds, 5 * 60);
        assert!(eval1.trigger_5m_warning);
        assert!(!eval1.trigger_1m_warning);

        // Next tick at same time does not re-trigger 5m warning
        let eval1_repeat = store.evaluate(&local1, true);
        assert!(!eval1_repeat.trigger_5m_warning);

        // Play 4 more minutes -> 1 minute remaining -> should fire 1m warning
        let t2 = t1 + 4 * 60;
        let local2 = local_time_from_epoch(t2, "UTC");
        store.record_play_seconds(4 * 60, &local2);
        let eval2 = store.evaluate(&local2, true);
        assert_eq!(eval2.effective_remaining_seconds, 60);
        assert!(eval2.trigger_1m_warning);

        // Play 1 more minute -> 0 remaining -> QuotaReached
        let t3 = t2 + 60;
        let local3 = local_time_from_epoch(t3, "UTC");
        store.record_play_seconds(60, &local3);
        let eval3 = store.evaluate(&local3, true);
        assert_eq!(eval3.lock_status, LockStatus::QuotaReached);
        assert_eq!(eval3.effective_remaining_seconds, 0);

        // Parent grants +15 bonus minutes -> Unlocked with 15m left
        store.grant_bonus_minutes(15, &local3);
        let eval4 = store.evaluate(&local3, true);
        assert_eq!(eval4.lock_status, LockStatus::Unlocked);
        assert_eq!(eval4.effective_remaining_seconds, 15 * 60);
    }

    #[test]
    fn test_bedtime_enforcement_and_bonus_snooze() {
        let dir = tempdir().unwrap();
        // 2026-11-18 20:30:00 UTC (after 20:00 bedtime)
        let t_bed = 1795033800i64;
        let mut store = StateStore::load_or_init(dir.path(), &[], t_bed).unwrap();
        let local_bed = local_time_from_epoch(t_bed, "UTC");

        let eval = store.evaluate(&local_bed, true);
        assert_eq!(
            eval.lock_status,
            LockStatus::BedtimeActive {
                wake_time: "07:00".to_string()
            }
        );

        // Granting +15m bonus during bedtime snoozes bedtime by 15m
        store.grant_bonus_minutes(15, &local_bed);
        let eval_snoozed = store.evaluate(&local_bed, true);
        assert_eq!(eval_snoozed.lock_status, LockStatus::Unlocked);
        assert_eq!(eval_snoozed.effective_remaining_seconds, 15 * 60);
    }

    #[test]
    fn test_clock_rollback_and_deleted_usage_fail_closed() {
        let dir = tempdir().unwrap();
        let t0 = 1795010400i64;
        let mut store = StateStore::load_or_init(dir.path(), &[], t0).unwrap();
        store.config.set_pin("4321").unwrap();
        store.save_all(t0).unwrap();

        // Simulate RTC battery drain back to 1970 (epoch 100)
        let local_1970 = local_time_from_epoch(100, "UTC");
        let eval = store.evaluate(&local_1970, true);
        assert_eq!(eval.lock_status, LockStatus::ClockSyncRequired);

        // Syncing time via portal restores normal state
        store.sync_time_and_clear_locks(t0 + 120, Some("UTC")).unwrap();
        let local_restored = local_time_from_epoch(t0 + 120, "UTC");
        assert_eq!(
            store.evaluate(&local_restored, true).lock_status,
            LockStatus::Unlocked
        );

        // Simulate kid deleting usage.json while config.json has a PIN
        fs::remove_file(StateStore::usage_path(dir.path())).unwrap();
        let mut reloaded = StateStore::load_or_init(dir.path(), &[], t0 + 180).unwrap();
        let eval_tamper = reloaded.evaluate(&local_restored, true);
        assert!(matches!(
            eval_tamper.lock_status,
            LockStatus::TamperLocked { .. }
        ));
    }

    #[test]
    fn test_pin_rate_limiting_and_reset_pin_escape_hatch() {
        let dir = tempdir().unwrap();
        let rom_dir = tempdir().unwrap();
        let t0 = 1795010400i64;
        let mut store = StateStore::load_or_init(dir.path(), &[rom_dir.path().to_path_buf()], t0).unwrap();
        store.config.set_pin("2580").unwrap();
        store.save_all(t0).unwrap();

        // 2 wrong attempts -> no cooldown yet
        assert_eq!(store.attempt_pin("0000", t0), Ok(false));
        assert_eq!(store.attempt_pin("1111", t0), Ok(false));
        // 3rd wrong attempt -> 60s cooldown
        assert_eq!(store.attempt_pin("2222", t0), Err(60));
        // Even correct PIN is rejected while cooldown active
        assert_eq!(store.attempt_pin("2580", t0 + 10), Err(50));

        // After 60s cooldown expires, try 2 more wrong attempts -> 5th failure triggers 600s (10m) cooldown
        assert_eq!(store.attempt_pin("3333", t0 + 61), Ok(false));
        assert_eq!(store.attempt_pin("4444", t0 + 62), Err(600));

        // Create RESET_PIN file on ROMs partition and reload
        let reset_file = rom_dir.path().join("RESET_PIN");
        fs::write(&reset_file, "").unwrap();
        let reloaded =
            StateStore::load_or_init(dir.path(), &[rom_dir.path().to_path_buf()], t0 + 700)
                .unwrap();
        assert!(!reloaded.config.has_pin());
        assert!(!reset_file.exists());
    }

    #[test]
    fn test_quota_disabled_toggle() {
        let dir = tempdir().unwrap();
        // 2026-11-18 14:00:00 UTC (Wednesday)
        let t0 = 1795010400i64;
        let mut store = StateStore::load_or_init(dir.path(), &[], t0).unwrap();
        store.config.quota_enabled = false;
        store.config.weekly_quotas.wed = 10; // 10 minutes
        store.config.bedtime = BedtimeConfig {
            enabled: true,
            wake_time: "07:00".to_string(),
            bed_time: "20:00".to_string(),
        };

        let local0 = local_time_from_epoch(t0, "UTC");
        // Exceed daily quota -> still unlocked because quota_enabled is false, limited only by bedtime (6h away)
        store.record_play_seconds(60 * 60, &local0);
        let eval_bed_only = store.evaluate(&local0, true);
        assert_eq!(eval_bed_only.lock_status, LockStatus::Unlocked);
        assert_eq!(eval_bed_only.effective_remaining_seconds, 6 * 3600);

        // Bedtime still locks after 20:00
        let local_night = local_time_from_epoch(t0 + 6 * 3600 + 60, "UTC");
        let eval_night = store.evaluate(&local_night, true);
        assert!(matches!(
            eval_night.lock_status,
            LockStatus::BedtimeActive { .. }
        ));

        // Disabling bedtime too makes playtime completely unrestricted
        store.config.bedtime.enabled = false;
        let eval_unrestricted = store.evaluate(&local_night, true);
        assert_eq!(eval_unrestricted.lock_status, LockStatus::Unlocked);
        assert_eq!(eval_unrestricted.effective_remaining_seconds, u64::MAX);
    }
}
