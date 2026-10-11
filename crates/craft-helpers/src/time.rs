//! Strict RFC 3339 timestamps: hostile input parses to None, never panics.

/// Days from 1970-01-01 for a validated civil date.
fn serial_day(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + i64::from(month > 2) * -3 + i64::from(month <= 2) * 9;
    let doy = (153 * mp + 2).div_euclid(5) + day - 1;
    let doe = yoe * 365 + yoe.div_euclid(4) - yoe.div_euclid(100) + doy;
    era * 146_097 + doe - 719_468
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
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// `YYYY-MM-DDTHH:MM:SS` plus `Z` or `±HH:MM` into Unix seconds; anything
/// else is None. GitHub emits the `Z` form; the offset form keeps parsing
/// total.
pub fn parse_rfc3339_secs(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let offset = match bytes.len() {
        20 if bytes.get(19) == Some(&b'Z') || bytes.get(19) == Some(&b'z') => 0i64,
        25 if bytes.get(19) == Some(&b'+') || bytes.get(19) == Some(&b'-') => {
            if bytes.get(22) != Some(&b':') {
                return None;
            }
            let hours = digits(bytes.get(20..22)?)?;
            let minutes = digits(bytes.get(23..25)?)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let magnitude = hours * 3600 + minutes * 60;
            if bytes.get(19) == Some(&b'-') {
                -magnitude
            } else {
                magnitude
            }
        }
        _ => return None,
    };
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = digits(bytes.get(0..4)?)?;
    let month = digits(bytes.get(5..7)?)?;
    let day = digits(bytes.get(8..10)?)?;
    let hour = digits(bytes.get(11..13)?)?;
    let minute = digits(bytes.get(14..16)?)?;
    let second = digits(bytes.get(17..19)?)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let local = serial_day(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    u64::try_from(local).ok()
}
