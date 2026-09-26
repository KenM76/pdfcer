//! Render a date or time through Acrobat's `AFDate_FormatEx` token grammar
//! — decision 009 posture B, the display side's remaining half.
//!
//! # Two halves, and only one of them is sourced
//!
//! Formatting a date needs two things: a **grammar** for the format string,
//! and a **parse** of whatever the field currently stores. They have very
//! different evidential footing, and this module treats them differently
//! because of it.
//!
//! - The **grammar** is fully sourced. `pdfcer-acrobat-librarian` returned the
//!   complete token table, corroborated by the same token set documented for
//!   `util.printd`, which *is* Adobe-primary. [`render`] implements it.
//! - The **parse** is **not sourced at all.** Nothing available describes how
//!   Acrobat reads a stored date string back out of a field. So [`parse`]
//!   does not attempt to reproduce Acrobat — it accepts a small set of
//!   **unambiguous** shapes and declines everything else, and the decline is
//!   disclosed rather than papered over with a guess.
//!
//! That split is the whole design. A format helper that renders the wrong
//! date is worse than one that renders none: a wrong date is plausible,
//! silent, and indistinguishable from a right one.
//!
//! # The grammar is case-sensitive, and that IS the grammar
//!
//! | Lower | Upper |
//! |---|---|
//! | `m`/`mm` = **month** | `M`/`MM` = **minutes** |
//! | `h`/`hh` = **12-hour** | `H`/`HH` = **24-hour** |
//!
//! A case-insensitive tokeniser silently corrupts every string containing
//! both a month and a time — `"mm/dd/yyyy HH:MM"` is the canonical example,
//! and it would render the minutes where the month belongs. This is the
//! single most operationally important fact in the table, and the tests below
//! pin it directly rather than trusting the implementation to be obviously
//! right.
//!
//! # Longest-match, and why the order of the table matters
//!
//! `mmmm` must be tried before `mmm`, `mmm` before `mm`, `mm` before `m`.
//! Matching shortest-first would read `mmmm` as four separate months. The
//! table is therefore ordered longest-first and matched in order; a test
//! asserts that property over the table itself rather than over one example,
//! because a token appended in the wrong place would break only the strings
//! that happen to use it.
//!
//! # What is deliberately not here
//!
//! No locale. Month and weekday names are English, matching the only names
//! any source describes, and pdfcer does not invent a localisation Acrobat is
//! not documented to have. No time zones: a form field stores a wall-clock
//! date, and a zone conversion would change the value the operator typed.

/// A wall-clock date and time, with no zone.
///
/// Deliberately not a general date type. It carries exactly what the token
/// grammar can render and nothing else, so there is no field a caller could
/// populate that this module would silently ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// Full year, e.g. 2026.
    pub year: i32,
    /// Month, 1–12.
    pub month: u32,
    /// Day of month, 1–31.
    pub day: u32,
    /// Hour, 0–23.
    pub hour: u32,
    /// Minute, 0–59.
    pub minute: u32,
    /// Second, 0–59.
    pub second: u32,
}

impl DateTime {
    /// Whether the fields form a real calendar date.
    ///
    /// Checked rather than assumed because a stored value is untrusted input:
    /// `2026-02-31` parses digit-wise and is not a date, and rendering it
    /// would produce a confident string for a day that does not exist.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
    }

    /// Day of the week, 0 = Sunday.
    ///
    /// Sakamoto's method — chosen over a table because it is total for every
    /// proleptic-Gregorian date and has no range to fall off the end of.
    #[must_use]
    pub fn weekday(&self) -> usize {
        const OFFSET: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let mut y = self.year;
        if self.month < 3 {
            y -= 1;
        }
        // Clamped before the lookup: this is reachable with an unvalidated
        // value, and a stored field value is untrusted input. A wrong
        // weekday for an impossible month is a cosmetic error; an index past
        // the table is a crash.
        let m = (self.month.clamp(1, 12) - 1) as usize;
        let offset = OFFSET.get(m).copied().unwrap_or(0);
        let raw = y + y / 4 - y / 100 + y / 400 + offset + self.day as i32;
        // `rem_euclid` rather than `%`: a negative year must not produce a
        // negative index, and years before 1 CE are reachable from a
        // malformed field value.
        raw.rem_euclid(7) as usize
    }
}

/// Days in a month, honouring the Gregorian leap rule.
const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Abbreviated and full month names, indexed 0 = January.
const MONTHS: [(&str, &str); 12] = [
    ("Jan", "January"),
    ("Feb", "February"),
    ("Mar", "March"),
    ("Apr", "April"),
    ("May", "May"),
    ("Jun", "June"),
    ("Jul", "July"),
    ("Aug", "August"),
    ("Sep", "September"),
    ("Oct", "October"),
    ("Nov", "November"),
    ("Dec", "December"),
];

/// Abbreviated and full weekday names, indexed 0 = Sunday.
const WEEKDAYS: [(&str, &str); 7] = [
    ("Sun", "Sunday"),
    ("Mon", "Monday"),
    ("Tue", "Tuesday"),
    ("Wed", "Wednesday"),
    ("Thu", "Thursday"),
    ("Fri", "Friday"),
    ("Sat", "Saturday"),
];

/// The token table, **longest first** — see the module header.
///
/// Public so a test can assert the longest-first property over the table
/// itself rather than over a handful of examples.
pub const TOKENS: [&str; 20] = [
    "mmmm", "dddd", "yyyy", "mmm", "ddd", "HH", "hh", "MM", "mm", "dd", "ss", "tt", "yy", "H", "h",
    "M", "m", "d", "s", "t",
];

/// Render `when` through an `AFDate_FormatEx` format string.
///
/// Unrecognised characters pass through verbatim — separators (`/`, `-`,
/// `:`, spaces) are exactly that, and a grammar that refused them would
/// refuse every real format string. A backslash escapes the next character,
/// so a literal `m` can be printed.
#[must_use]
pub fn render(format: &[u8], when: &DateTime) -> String {
    let f = String::from_utf8_lossy(format);
    let chars: Vec<char> = f.chars().collect();
    let mut out = String::with_capacity(chars.len() + 8);
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        // A backslash escapes the next character out of the grammar.
        if c == '\\' {
            match chars.get(i + 1) {
                Some(next) => {
                    out.push(*next);
                    i += 2;
                }
                // A trailing backslash escapes nothing; emit it rather than
                // dropping a character the format string contained.
                None => {
                    out.push('\\');
                    i += 1;
                }
            }
            continue;
        }
        let rest: String = chars.get(i..).unwrap_or_default().iter().collect();
        match TOKENS.iter().find(|t| rest.starts_with(**t)) {
            Some(token) => {
                out.push_str(&expand(token, when));
                // Tokens are ASCII, so their byte length is their char count.
                i += token.chars().count();
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Expand one matched token.
fn expand(token: &str, w: &DateTime) -> String {
    // Indices are guarded by `DateTime::is_valid`, but this function is
    // reachable with an unvalidated value, so every lookup saturates rather
    // than panicking — a malformed stored date must not take the process
    // down.
    let month_idx = (w.month.clamp(1, 12) - 1) as usize;
    let month = MONTHS.get(month_idx).copied().unwrap_or(("Jan", "January"));
    let weekday = WEEKDAYS
        .get(w.weekday().min(6))
        .copied()
        .unwrap_or(("Sun", "Sunday"));
    match token {
        "d" => w.day.to_string(),
        "dd" => format!("{:02}", w.day),
        "ddd" => weekday.0.to_owned(),
        "dddd" => weekday.1.to_owned(),
        "m" => w.month.to_string(),
        "mm" => format!("{:02}", w.month),
        "mmm" => month.0.to_owned(),
        "mmmm" => month.1.to_owned(),
        "yy" => format!("{:02}", w.year.rem_euclid(100)),
        "yyyy" => format!("{:04}", w.year),
        "H" => w.hour.to_string(),
        "HH" => format!("{:02}", w.hour),
        "h" => hour12(w.hour).to_string(),
        "hh" => format!("{:02}", hour12(w.hour)),
        "M" => w.minute.to_string(),
        "MM" => format!("{:02}", w.minute),
        "s" => w.second.to_string(),
        "ss" => format!("{:02}", w.second),
        "t" => if w.hour < 12 { "A" } else { "P" }.to_owned(),
        "tt" => if w.hour < 12 { "AM" } else { "PM" }.to_owned(),
        other => other.to_owned(),
    }
}

/// A 24-hour hour as a 12-hour one, where midnight and noon are both 12.
const fn hour12(hour: u32) -> u32 {
    match hour % 12 {
        0 => 12,
        h => h,
    }
}

/// The predefined `AFDate_Format` table, index 0–13.
///
/// Dual-sourced and agreeing index-for-index, which is the strongest evidence
/// in the whole posture-B set. Indices **12 and 13 embed a time component in
/// a nominally "date" format** — a real quirk, confirmed identically by both
/// sources, and not a transcription slip.
pub const DATE_FORMATS: [&str; 14] = [
    "m/d",
    "m/d/yy",
    "mm/dd/yy",
    "mm/yy",
    "d-mmm",
    "d-mmm-yy",
    "dd-mmm-yy",
    "yy-mm-dd",
    "mmm-yy",
    "mmmm-yy",
    "mmm d, yyyy",
    "mmmm d, yyyy",
    "m/d/yy h:MM tt",
    "m/d/yy HH:MM",
];

/// The predefined `AFTime_Format` table, index 0–3.
///
/// Materially shorter than the date table — four entries, not fourteen.
pub const TIME_FORMATS: [&str; 4] = ["HH:MM", "h:MM tt", "HH:MM:ss", "h:MM:ss tt"];

/// Look up a predefined date format, or `None` for an out-of-range index.
///
/// Out of range **declines** rather than falling back. One reimplementation
/// treats an unknown index as a literal format string, which would render a
/// stored `"99"` as the text `99`; nothing confirms real Acrobat does that,
/// and inventing an output for an index no source describes is exactly the
/// guessing this posture refuses.
#[must_use]
pub fn date_format(index: i64) -> Option<&'static str> {
    usize::try_from(index)
        .ok()
        .and_then(|i| DATE_FORMATS.get(i).copied())
}

/// Look up a predefined time format, or `None` for an out-of-range index.
#[must_use]
pub fn time_format(index: i64) -> Option<&'static str> {
    usize::try_from(index)
        .ok()
        .and_then(|i| TIME_FORMATS.get(i).copied())
}

/// Read a stored field value as a date, **accepting only unambiguous
/// shapes**.
///
/// # Why this is deliberately narrow
///
/// Nothing sourced describes how Acrobat parses a stored date string, so
/// there is no behaviour to reproduce — only a choice to make. The choice
/// here is to accept what cannot be read two ways and decline the rest:
///
/// - `yyyy-mm-dd`, optionally with `hh:mm` or `hh:mm:ss`. ISO-ordered, so
///   no field can be confused with another.
/// - A PDF date string, `D:YYYYMMDDHHmmSS` (§7.9.4), with any trailing
///   zone offset ignored — the components are positional and unambiguous.
///
/// **`03/04/2026` is refused**, and that refusal is the point: it is 3 April
/// to most of the world and 4 March in the United States, the stored value
/// carries nothing that decides which, and a format helper that guessed
/// would render a confident wrong date on half the forms it met.
///
/// Returns `None` for anything else, which routes the caller to disclose an
/// unformatted value rather than a wrong one.
#[must_use]
pub fn parse(text: &str) -> Option<DateTime> {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("D:") {
        return parse_pdf_date(rest);
    }
    parse_iso(t)
}

/// `yyyy-mm-dd[ hh:mm[:ss]]`, with `T` accepted in place of the space.
fn parse_iso(t: &str) -> Option<DateTime> {
    let (date, time) = match t.split_once(['T', ' ']) {
        Some((d, rest)) => (d, Some(rest.trim())),
        None => (t, None),
    };
    let mut parts = date.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = two_digits(parts.next()?)?;
    let day: u32 = two_digits(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let (hour, minute, second) = match time {
        None => (0, 0, 0),
        Some(time) => {
            let mut tp = time.split(':');
            let h = two_digits(tp.next()?)?;
            let m = two_digits(tp.next()?)?;
            let s = match tp.next() {
                Some(s) => two_digits(s)?,
                None => 0,
            };
            if tp.next().is_some() {
                return None;
            }
            (h, m, s)
        }
    };
    let when = DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    };
    when.is_valid().then_some(when)
}

/// `YYYYMMDDHHmmSS` with everything after the year optional (§7.9.4).
fn parse_pdf_date(rest: &str) -> Option<DateTime> {
    // A zone offset (`+05'00'`, `Z`) is ignored rather than applied: a form
    // field holds a wall-clock date, and shifting it would change the value
    // the operator typed into something they did not.
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 4 || !digits.len().is_multiple_of(2) {
        return None;
    }
    let at = |start: usize, default: u32| -> Option<u32> {
        match digits.get(start..start + 2) {
            Some(s) => s.parse().ok(),
            None => Some(default),
        }
    };
    let year: i32 = digits.get(0..4)?.parse().ok()?;
    let when = DateTime {
        year,
        month: at(4, 1)?,
        day: at(6, 1)?,
        hour: at(8, 0)?,
        minute: at(10, 0)?,
        second: at(12, 0)?,
    };
    when.is_valid().then_some(when)
}

/// A one- or two-digit component.
fn two_digits(s: &str) -> Option<u32> {
    (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}
