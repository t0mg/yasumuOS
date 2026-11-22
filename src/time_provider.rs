use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Provides both wall-clock time and `CLOCK_MONOTONIC` seconds,
/// supporting `MOCK_TIME` and `MOCK_MONOTONIC_SPEED` for deterministic testing.
#[derive(Clone)]
pub struct TimeProvider {
    inner: Arc<Mutex<TimeState>>,
}

struct TimeState {
    real_monotonic_start_ns: u128,
    real_wall_start_sec: i64,
    mock_wall_base_sec: Option<i64>,
    mock_monotonic_speed: f64,
    manual_monotonic_offset_sec: u64,
    manual_wall_offset_sec: i64,
}

fn raw_clock_monotonic_ns() -> u128 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc == 0 {
        (ts.tv_sec as u128) * 1_000_000_000u128 + (ts.tv_nsec as u128)
    } else {
        0
    }
}

fn raw_wall_clock_sec() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Default for TimeProvider {
    fn default() -> Self {
        Self::from_env()
    }
}

impl TimeProvider {
    pub fn from_env() -> Self {
        let mock_wall_base_sec = std::env::var("MOCK_TIME")
            .ok()
            .and_then(|v| v.parse::<i64>().ok());
        let mock_monotonic_speed = std::env::var("MOCK_MONOTONIC_SPEED")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0)
            .max(0.0);

        Self {
            inner: Arc::new(Mutex::new(TimeState {
                real_monotonic_start_ns: raw_clock_monotonic_ns(),
                real_wall_start_sec: raw_wall_clock_sec(),
                mock_wall_base_sec,
                mock_monotonic_speed,
                manual_monotonic_offset_sec: 0,
                manual_wall_offset_sec: 0,
            })),
        }
    }

    pub fn new_mock(initial_wall_sec: i64, monotonic_speed: f64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TimeState {
                real_monotonic_start_ns: raw_clock_monotonic_ns(),
                real_wall_start_sec: raw_wall_clock_sec(),
                mock_wall_base_sec: Some(initial_wall_sec),
                mock_monotonic_speed: monotonic_speed.max(0.0),
                manual_monotonic_offset_sec: 0,
                manual_wall_offset_sec: 0,
            })),
        }
    }

    /// Returns monotonic seconds (pauses during system suspend on Linux via `CLOCK_MONOTONIC`).
    pub fn monotonic_secs(&self) -> u64 {
        let st = self.inner.lock().unwrap();
        let now_ns = raw_clock_monotonic_ns();
        let delta_ns = now_ns.saturating_sub(st.real_monotonic_start_ns);
        let scaled_secs = ((delta_ns as f64) / 1_000_000_000.0 * st.mock_monotonic_speed) as u64;
        scaled_secs + st.manual_monotonic_offset_sec
    }

    /// Returns current Unix epoch timestamp in seconds.
    pub fn wall_clock_secs(&self) -> i64 {
        let st = self.inner.lock().unwrap();
        if let Some(base) = st.mock_wall_base_sec {
            let now_ns = raw_clock_monotonic_ns();
            let delta_ns = now_ns.saturating_sub(st.real_monotonic_start_ns);
            let scaled_secs =
                ((delta_ns as f64) / 1_000_000_000.0 * st.mock_monotonic_speed) as i64;
            base + scaled_secs + st.manual_wall_offset_sec
        } else {
            let _ = st.real_wall_start_sec;
            raw_wall_clock_sec() + st.manual_wall_offset_sec
        }
    }

    /// Advances both wall clock and monotonic clock in tests/dev mode.
    pub fn advance(&self, wall_delta_sec: i64, monotonic_delta_sec: u64) {
        let mut st = self.inner.lock().unwrap();
        st.manual_wall_offset_sec += wall_delta_sec;
        st.manual_monotonic_offset_sec += monotonic_delta_sec;
    }

    /// Sets the wall-clock time explicitly (used by `POST /api/sync-time` or tests).
    pub fn set_wall_clock_secs(&self, new_epoch_sec: i64) {
        let mut st = self.inner.lock().unwrap();
        st.real_monotonic_start_ns = raw_clock_monotonic_ns();
        st.real_wall_start_sec = raw_wall_clock_sec();
        st.mock_wall_base_sec = Some(new_epoch_sec);
        st.manual_wall_offset_sec = 0;
    }

    /// Converts current wall-clock time to local date/time components in the given IANA timezone.
    pub fn local_now(&self, tz_name: &str) -> LocalTimeInfo {
        let epoch = self.wall_clock_secs();
        local_time_from_epoch(epoch, tz_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTimeInfo {
    pub epoch_secs: i64,
    pub date_str: String,
    pub date: NaiveDate,
    pub weekday: Weekday,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub minutes_since_midnight: u32,
    pub seconds_since_midnight: u32,
}

pub fn parse_timezone(tz_name: &str) -> Tz {
    tz_name.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

pub fn local_time_from_epoch(epoch_secs: i64, tz_name: &str) -> LocalTimeInfo {
    let tz = parse_timezone(tz_name);
    let utc_dt = DateTime::<Utc>::from_timestamp(epoch_secs, 0)
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
    let local_dt = utc_dt.with_timezone(&tz);
    let date = local_dt.date_naive();
    let hour = local_dt.hour();
    let minute = local_dt.minute();
    let second = local_dt.second();

    LocalTimeInfo {
        epoch_secs,
        date_str: format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
        date,
        weekday: local_dt.weekday(),
        hour,
        minute,
        second,
        minutes_since_midnight: hour * 60 + minute,
        seconds_since_midnight: hour * 3600 + minute * 60 + second,
    }
}

/// Parses an `"HH:MM"` string into minutes since midnight (`0..1440`).
pub fn parse_hhmm_to_minutes(hhmm: &str) -> Option<u32> {
    let parts: Vec<&str> = hhmm.trim().split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h = parts[0].parse::<u32>().ok()?;
    let m = parts[1].parse::<u32>().ok()?;
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(h * 60 + m)
}
