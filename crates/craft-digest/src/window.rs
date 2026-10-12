//! Window math: pure functions on UTC `YYYY-MM-DD` timestamps.

use crate::error::{DigestError, bounded};

/// Windows never reach back further than this many days.
pub const MAX_WINDOW_DAYS: i64 = 30;
const FALLBACK_DAYS: i64 = 1;
const MARKER_PREFIX: &str = "<!-- digest-window: ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSource {
    Marker,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub since: String,
    pub until: String,
    pub source: WindowSource,
}

/// The digest's final line, the durable window state on the sticky issue.
pub fn marker(window: &Window) -> String {
    format!("{MARKER_PREFIX}{}..{} -->", window.since, window.until)
}

pub fn is_valid_date(text: &str) -> bool {
    parse_date(text).is_some()
}

/// Strict `YYYY-MM-DD`: digits, years 0001-9999, in-range month and day.
fn parse_date(text: &str) -> Option<(i64, i64, i64)> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = digits(&bytes[0..4])?;
    let month = digits(&bytes[5..7])?;
    let day = digits(&bytes[8..10])?;
    if year < 1 || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some((year, month, day))
}

fn digits(bytes: &[u8]) -> Option<i64> {
    let mut value: i64 = 0;
    for b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i64::from(b - b'0');
    }
    Some(value)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Serial day number (1970-01-01 = 0); years stay non-negative here.
fn serial((year, month, day): (i64, i64, i64)) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let m = month + i64::from(month > 2) * -3 + i64::from(month <= 2) * 9;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn deserial(serial_day: i64) -> (i64, i64, i64) {
    let z = serial_day + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + i64::from(mp < 10) * 3 + i64::from(mp >= 10) * -9;
    (y + i64::from(month <= 2), month, day)
}

fn fmt((year, month, day): (i64, i64, i64)) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

fn shift_days(date: (i64, i64, i64), delta: i64) -> (i64, i64, i64) {
    deserial(serial(date) + delta)
}

fn window_err(message: String) -> DigestError {
    DigestError::Window(bounded(&message))
}

/// Today + optional `--since` into a clamped, validated window.
pub fn resolve_window(today: &str, since: Option<&str>) -> Result<Window, DigestError> {
    let end = parse_date(today)
        .ok_or_else(|| window_err(format!("today {today:?} is not a YYYY-MM-DD date")))?;
    let floor = shift_days(end, -MAX_WINDOW_DAYS);
    let since_date = match since {
        None => shift_days(end, -FALLBACK_DAYS),
        Some(text) => {
            let date = parse_date(text)
                .ok_or_else(|| window_err(format!("--since {text:?} is not a YYYY-MM-DD date")))?;
            let clamped = if date < floor { floor } else { date };
            if clamped > end { end } else { clamped }
        }
    };
    let source = match since {
        None => WindowSource::Fallback,
        Some(_) => WindowSource::Marker,
    };
    Ok(Window {
        since: fmt(since_date),
        until: fmt(end),
        source,
    })
}

/// The UTC calendar date for a Unix epoch second count; time-of-day is
/// dropped and the day boundary is the UTC one.
pub fn date_from_epoch_secs(secs: u64) -> String {
    fmt(deserial((secs / 86_400) as i64))
}

fn window_epoch_err(message: &str) -> DigestError {
    window_err(format!("{message} (system clock before the Unix epoch)"))
}

/// The CLI look-back duration as a window: spans under a day fall back to
/// the last UTC day, longer spans reach back to the date `now - lookback`;
/// `resolve_window` applies the 30-day clamp and validation either way.
pub fn resolve_window_lookback(lookback: std::time::Duration) -> Result<Window, DigestError> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| window_epoch_err("clock"))?
        .as_secs();
    let today = date_from_epoch_secs(secs);
    if lookback.as_secs() < 86_400 {
        return resolve_window(&today, None);
    }
    let since = date_from_epoch_secs(secs.saturating_sub(lookback.as_secs()));
    resolve_window(&today, Some(&since))
}
