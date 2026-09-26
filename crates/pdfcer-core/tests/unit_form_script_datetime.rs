//! Tests for `pdfcer_core::form_script::datetime`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::form_script::datetime::*;

/// 2026-08-11, 14:05:09 — a Tuesday, afternoon, so every case-sensitive
/// pair renders differently.
fn when() -> DateTime {
    DateTime {
        year: 2026,
        month: 8,
        day: 11,
        hour: 14,
        minute: 5,
        second: 9,
    }
}

/// **Case decides month-versus-minutes and 12-versus-24 hour.**
///
/// The single most important property in the grammar. A case-insensitive
/// tokeniser would render the minutes where the month belongs in every
/// string carrying both — and `mm/dd/yyyy HH:MM` is the commonest such
/// string there is.
#[test]
fn case_decides_month_versus_minutes_and_twelve_versus_twenty_four_hour() {
    let w = when();
    assert_eq!(render(b"mm", &w), "08", "lowercase mm is the MONTH");
    assert_eq!(render(b"MM", &w), "05", "uppercase MM is the MINUTES");
    assert_eq!(render(b"hh", &w), "02", "lowercase hh is 12-hour");
    assert_eq!(render(b"HH", &w), "14", "uppercase HH is 24-hour");
    assert_eq!(
        render(b"mm/dd/yyyy HH:MM", &w),
        "08/11/2026 14:05",
        "and the canonical mixed string comes out right"
    );
}

/// Longest-match: `mmmm` is one token, not four.
#[test]
fn tokens_match_longest_first() {
    let w = when();
    assert_eq!(render(b"mmmm", &w), "August");
    assert_eq!(render(b"mmm", &w), "Aug");
    assert_eq!(render(b"mm", &w), "08");
    assert_eq!(render(b"m", &w), "8");
    assert_eq!(render(b"dddd", &w), "Tuesday");
    assert_eq!(render(b"ddd", &w), "Tue");

    // Asserted over the TABLE, not just these examples. Matching takes
    // the FIRST token that fits, so an earlier entry that is a prefix of
    // a later one makes the later one unreachable: with "m" ahead of
    // "mmmm", every month name would render as four digits.
    for (i, later) in TOKENS.iter().enumerate() {
        for earlier in &TOKENS[..i] {
            assert!(
                !later.starts_with(earlier),
                "{earlier:?} comes before {later:?} and is a prefix of it, so {later:?} can never match"
            );
        }
    }
}

/// Every predefined date format renders, and the two that secretly carry
/// a time render it.
#[test]
fn the_predefined_date_formats_render_including_the_two_with_times() {
    let w = when();
    assert_eq!(render(DATE_FORMATS[1].as_bytes(), &w), "8/11/26");
    assert_eq!(render(DATE_FORMATS[2].as_bytes(), &w), "08/11/26");
    assert_eq!(render(DATE_FORMATS[7].as_bytes(), &w), "26-08-11");
    assert_eq!(render(DATE_FORMATS[11].as_bytes(), &w), "August 11, 2026");
    assert_eq!(
        render(DATE_FORMATS[12].as_bytes(), &w),
        "8/11/26 2:05 PM",
        "index 12 embeds a time in a 'date' format — a real quirk"
    );
    assert_eq!(render(DATE_FORMATS[13].as_bytes(), &w), "8/11/26 14:05");
}

/// The four predefined time formats.
#[test]
fn the_predefined_time_formats_render() {
    let w = when();
    assert_eq!(render(TIME_FORMATS[0].as_bytes(), &w), "14:05");
    assert_eq!(render(TIME_FORMATS[1].as_bytes(), &w), "2:05 PM");
    assert_eq!(render(TIME_FORMATS[2].as_bytes(), &w), "14:05:09");
    assert_eq!(render(TIME_FORMATS[3].as_bytes(), &w), "2:05:09 PM");
}

/// Midnight and noon are both `12` on a 12-hour clock, and the meridiem
/// flips at noon exactly.
#[test]
fn midnight_and_noon_are_both_twelve() {
    let at = |hour| DateTime { hour, ..when() };
    assert_eq!(render(b"h tt", &at(0)), "12 AM");
    assert_eq!(render(b"h tt", &at(11)), "11 AM");
    assert_eq!(render(b"h tt", &at(12)), "12 PM");
    assert_eq!(render(b"h tt", &at(23)), "11 PM");
}

/// Separators pass through, and a backslash escapes a token character so
/// a literal can be printed.
#[test]
fn separators_pass_through_and_a_backslash_escapes() {
    let w = when();
    assert_eq!(render(b"yyyy-mm-dd", &w), "2026-08-11");
    assert_eq!(
        render(br"\m\m mm", &w),
        "mm 08",
        "an escaped m is a letter, an unescaped mm is the month"
    );
}

/// **An ambiguous stored value is REFUSED, not guessed at.**
///
/// `03/04/2026` is 3 April to most of the world and 4 March in the
/// United States. The stored value carries nothing that decides which,
/// so a helper that picked would render a confident wrong date on half
/// the forms it met.
#[test]
fn an_ambiguous_stored_date_is_refused() {
    assert_eq!(parse("03/04/2026"), None);
    assert_eq!(parse("4/3/26"), None);
    assert_eq!(parse("March 4, 2026"), None, "no month-name parsing either");
    assert_eq!(parse(""), None);
    assert_eq!(parse("not a date"), None);
}

/// The unambiguous shapes parse.
#[test]
fn iso_ordered_and_pdf_date_strings_parse() {
    assert_eq!(
        parse("2026-08-11"),
        Some(DateTime {
            year: 2026,
            month: 8,
            day: 11,
            hour: 0,
            minute: 0,
            second: 0
        })
    );
    assert_eq!(parse("2026-08-11 14:05:09"), Some(when()));
    assert_eq!(parse("2026-08-11T14:05:09"), Some(when()));
    assert_eq!(parse("D:20260811140509"), Some(when()));
    assert_eq!(
        parse("D:20260811140509+05'00'"),
        Some(when()),
        "a zone offset is ignored, not applied — a form field holds a \
         wall-clock date and shifting it would change what was typed"
    );
}

/// A value that is digits but not a date is refused, so no impossible
/// day is ever rendered.
#[test]
fn an_impossible_date_is_refused() {
    assert_eq!(parse("2026-02-31"), None, "February has no 31st");
    assert_eq!(parse("2026-13-01"), None, "no thirteenth month");
    assert_eq!(parse("2026-08-11 25:00"), None, "no twenty-fifth hour");
    assert!(parse("2024-02-29").is_some(), "but a real leap day is fine");
    assert_eq!(parse("2026-02-29"), None, "and a fake one is not");
}

/// An out-of-range predefined index declines rather than inventing an
/// output for a mode no source describes.
#[test]
fn an_out_of_range_predefined_index_declines() {
    assert_eq!(date_format(0), Some("m/d"));
    assert_eq!(date_format(13), Some("m/d/yy HH:MM"));
    assert_eq!(date_format(14), None);
    assert_eq!(date_format(-1), None);
    assert_eq!(time_format(3), Some("h:MM:ss tt"));
    assert_eq!(time_format(4), None);
}

/// The weekday calculation is right across a century boundary and a leap
/// year, because an off-by-one there renders the wrong day name with no
/// other symptom.
#[test]
fn weekdays_are_right_across_leap_years_and_centuries() {
    let day = |year, month, day| {
        DateTime {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
        }
        .weekday()
    };
    assert_eq!(day(2000, 1, 1), 6, "2000-01-01 was a Saturday");
    assert_eq!(day(1900, 1, 1), 1, "1900-01-01 was a Monday");
    assert_eq!(day(2024, 2, 29), 4, "the 2024 leap day was a Thursday");
    assert_eq!(day(2026, 8, 11), 2, "the fixture date is a Tuesday");
}
