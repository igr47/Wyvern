//! Calendar helpers (proleptic Gregorian, no time zones). Dates are days since
//! 1970-01-01, datetimes are microseconds since the epoch.

pub const MICROS_PER_DAY: i64 = 86_400_000_000;

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => if is_leap(y) { 29 } else { 28 },
    }
}

/// `YYYY-MM-DD`
pub fn parse_date(s: &str) -> Option<i32> {
    let s = s.trim();
    let mut it = s.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    i32::try_from(days_from_civil(y, m, d)).ok()
}

/// `YYYY-MM-DD[ T]HH:MM[:SS[.ffffff]][Z]` (a bare date means midnight)
pub fn parse_datetime(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = match s.find([' ', 'T']) {
        Some(i) => (&s[..i], Some(s[i + 1..].trim())),
        None => (s, None),
    };
    let days = parse_date(date)? as i64;
    let mut micros = 0i64;
    if let Some(t) = time {
        let mut p = t.splitn(3, ':');
        let h: i64 = p.next()?.parse().ok()?;
        let mi: i64 = p.next()?.parse().ok()?;
        let sec = p.next().unwrap_or("0");
        let (whole, frac) = match sec.split_once('.') {
            Some((w, f)) => (w, f),
            None => (sec, ""),
        };
        let se: i64 = whole.parse().ok()?;
        if h > 23 || mi > 59 || se > 59 {
            return None;
        }
        let mut f = frac.chars().take(6).collect::<String>();
        while f.len() < 6 {
            f.push('0');
        }
        let fm: i64 = f.parse().ok()?;
        micros = ((h * 60 + mi) * 60 + se) * 1_000_000 + fm;
    }
    Some(days * MICROS_PER_DAY + micros)
}

pub fn format_date(days: i32) -> String {
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn format_datetime(micros: i64) -> String {
    let days = micros.div_euclid(MICROS_PER_DAY);
    let rem = micros.rem_euclid(MICROS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let secs = rem / 1_000_000;
    let frac = rem % 1_000_000;
    let base = format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
    if frac == 0 { base } else { format!("{base}.{frac:06}") }
}

/// 0 = Sunday .. 6 = Saturday
pub fn weekday(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

