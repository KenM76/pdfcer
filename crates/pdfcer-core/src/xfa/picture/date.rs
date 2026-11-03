//! `date{}` and `time{}` pictures on canonical values (pp.1167-1175):
//! dates `YYYY-MM-DD` or `YYYYMMDD`, times `HH:MM[:SS[.FFF]]`.

use super::{Out, Tok, is_literal, tokens};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// A canonical date's year, month and day; `None` when `v` is not one.
fn parse_date(v: &str) -> Option<(i64, u32, u32)> {
    let digits: String = v.chars().filter(|c| *c != '-').collect();
    let dashed =
        v.len() == 10 && v.as_bytes().get(4) == Some(&b'-') && v.as_bytes().get(7) == Some(&b'-');
    if !(digits.len() == 8 && (dashed || v.len() == 8))
        || !digits.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let y = digits.get(..4)?.parse().ok()?;
    let m: u32 = digits.get(4..6)?.parse().ok()?;
    let d: u32 = digits.get(6..)?.parse().ok()?;
    ((1..=12).contains(&m) && d >= 1 && d <= days_in(y, m)).then_some((y, m, d))
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in(y: i64, m: u32) -> u32 {
    match m {
        2 if leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Day of the year, 1-based.
fn ordinal(y: i64, m: u32, d: u32) -> u32 {
    (1..m).map(|k| days_in(y, k)).sum::<u32>() + d
}

/// Day of the week, 0 = Sunday (Sakamoto's method).
fn weekday(y: i64, m: u32, d: u32) -> usize {
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let t = T.get(m as usize - 1).copied().unwrap_or(0);
    usize::try_from((y + y / 4 - y / 100 + y / 400 + t + i64::from(d)).rem_euclid(7)).unwrap_or(0)
}

/// ISO 8601 week of the year: the week holding January 4 is week 1.
fn iso_week(y: i64, m: u32, d: u32) -> u32 {
    let iso_day = |y, m, d| (weekday(y, m, d) + 6) % 7; // 0 = Monday
    let day = i64::from(ordinal(y, m, d));
    let week = (day - iso_day(y, m, d) as i64 + 9) / 7;
    if week < 1 {
        return iso_week(y - 1, 12, 31);
    }
    let year_days = if leap(y) { 366 } else { 365 };
    if week == 53 && i64::from(year_days) - day < 3 - iso_day(y, m, d) as i64 {
        return 1;
    }
    u32::try_from(week).unwrap_or(1)
}

/// `value` by a date picture.
pub(super) fn format_date(body: &str, value: &str) -> Option<Out> {
    let (y, m, d) = parse_date(value)?;
    let (mut s, mut names) = (String::new(), false);
    let mi = m as usize - 1;
    for t in tokens(body) {
        let piece = match t {
            Tok::Lit(l) => l,
            Tok::Sym(c, n) if is_literal(c) => c.to_string().repeat(n),
            Tok::Sym('D', 1) => d.to_string(),
            Tok::Sym('D', 2) => format!("{d:02}"),
            Tok::Sym('J', 1) => ordinal(y, m, d).to_string(),
            Tok::Sym('J', 3) => format!("{:03}", ordinal(y, m, d)),
            Tok::Sym('M', 1) => m.to_string(),
            Tok::Sym('M', 2) => format!("{m:02}"),
            Tok::Sym('M', 3) => short(MONTHS.get(mi)?, &mut names),
            Tok::Sym('M', 4) => long(MONTHS.get(mi)?, &mut names),
            Tok::Sym('E', 1) => (weekday(y, m, d) + 1).to_string(),
            Tok::Sym('E', 3) => short(DAYS.get(weekday(y, m, d))?, &mut names),
            Tok::Sym('E', 4) => long(DAYS.get(weekday(y, m, d))?, &mut names),
            Tok::Sym('e', 1) => ((weekday(y, m, d) + 6) % 7 + 1).to_string(),
            Tok::Sym('Y', 2) => format!("{:02}", y.rem_euclid(100)),
            Tok::Sym('Y', 4) => format!("{y:04}"),
            Tok::Sym('W', 2) => format!("{:02}", iso_week(y, m, d)),
            Tok::Sym('G', 1) => long("AD", &mut names),
            Tok::Sym(..) => return Some(Out::Unsupported),
        };
        s.push_str(&piece);
    }
    Some(Out::Text(s, names))
}

fn short(name: &str, names: &mut bool) -> String {
    *names = true;
    name.chars().take(3).collect()
}

fn long(name: &str, names: &mut bool) -> String {
    *names = true;
    name.to_owned()
}

/// A canonical time's hour, minute, second and millisecond.
fn parse_time(v: &str) -> Option<(u32, u32, u32, u32)> {
    let v = v.split(['Z', '+']).next()?;
    let (hms, frac) = v.split_once('.').unwrap_or((v, "0"));
    let mut parts = hms.split(':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next().unwrap_or("0").parse().ok()?;
    let s: u32 = parts.next().unwrap_or("0").parse().ok()?;
    let ms: u32 = format!("{frac:0<3}").get(..3)?.parse().ok()?;
    (h < 24 && m < 60 && s < 60 && parts.next().is_none()).then_some((h, m, s, ms))
}

/// `value` by a time picture.
pub(super) fn format_time(body: &str, value: &str) -> Option<Out> {
    let (h, m, sec, ms) = parse_time(value)?;
    let (mut s, mut names) = (String::new(), false);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    for t in tokens(body) {
        let piece = match t {
            Tok::Lit(l) => l,
            Tok::Sym(c, n) if is_literal(c) => c.to_string().repeat(n),
            Tok::Sym('h', 1) => h12.to_string(),
            Tok::Sym('h', 2) => format!("{h12:02}"),
            Tok::Sym('k', 1) => (h % 12).to_string(),
            Tok::Sym('k', 2) => format!("{:02}", h % 12),
            Tok::Sym('H', 1) => h.to_string(),
            Tok::Sym('H', 2) => format!("{h:02}"),
            Tok::Sym('K', 1) => (h + 1).to_string(),
            Tok::Sym('K', 2) => format!("{:02}", h + 1),
            Tok::Sym('M', 1) => m.to_string(),
            Tok::Sym('M', 2) => format!("{m:02}"),
            Tok::Sym('S', 1) => sec.to_string(),
            Tok::Sym('S', 2) => format!("{sec:02}"),
            Tok::Sym('F', 3) => format!("{ms:03}"),
            Tok::Sym('A', 1) => long(if h < 12 { "AM" } else { "PM" }, &mut names),
            Tok::Sym(..) => return Some(Out::Unsupported),
        };
        s.push_str(&piece);
    }
    Some(Out::Text(s, names))
}
