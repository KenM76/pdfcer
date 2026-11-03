#![allow(clippy::unwrap_used)] // a panic here is the test failing

//! The spec's own picture examples (pp.1167-1185).

use super::{PictureRounding, Shown, ValueKind};

fn format(picture: &str, value: &str, kind: ValueKind, english: bool) -> Option<Shown> {
    super::format(picture, value, kind, english, PictureRounding::HalfUp)
}

#[test]
fn each_rounding_mode_drops_excess_fraction_digits_its_own_way() {
    let r = |mode, v| {
        super::format("num{S9.9}", v, ValueKind::Num, true, mode).map(|s| s.text.trim().to_owned())
    };
    let (up, even, cut) = (
        PictureRounding::HalfUp,
        PictureRounding::HalfEven,
        PictureRounding::Truncate,
    );
    assert_eq!(r(up, "2.25").as_deref(), Some("2.3"));
    assert_eq!(r(up, "-2.25").as_deref(), Some("-2.3"));
    assert_eq!(r(even, "2.25").as_deref(), Some("2.2"));
    assert_eq!(r(even, "2.35").as_deref(), Some("2.4"));
    assert_eq!(r(even, "2.251").as_deref(), Some("2.3"));
    assert_eq!(r(cut, "2.29").as_deref(), Some("2.2"));
    let whole = |mode, v| super::format("num{9}", v, ValueKind::Num, true, mode).map(|s| s.text);
    assert_eq!(whole(even, "2.5").as_deref(), Some("2"));
    assert_eq!(whole(even, "3.5").as_deref(), Some("4"));
    assert_eq!(whole(even, ".5").as_deref(), Some("0"));
}

fn num(picture: &str, value: &str) -> Option<String> {
    format(picture, value, ValueKind::Num, true).map(|s| s.text)
}

fn date(picture: &str, value: &str) -> Option<String> {
    format(picture, value, ValueKind::Date, true).map(|s| s.text)
}

#[test]
fn num_examples_match_the_spec_tables() {
    assert_eq!(num("zz9.zzz", "123").as_deref(), Some("123"));
    assert_eq!(num("zz9.zzz", "123.").as_deref(), Some("123."));
    assert_eq!(num("zz9.zzz", "123.0").as_deref(), Some("123.0"));
    assert_eq!(num("ZZ9.ZZZ", "1.234").as_deref(), Some("  1.234"));
    assert_eq!(num("ZZ9.ZZZ", "1.2").as_deref(), Some("  1.2  "));
    assert_eq!(num("SZZ9.99", "12.3").as_deref(), Some("  12.30"));
    assert_eq!(num("szz9.99", "123").as_deref(), Some("123.00"));
    assert_eq!(num("S999v99", "-1.23").as_deref(), Some("-00123"));
    assert_eq!(num("zz9%", "0.12").as_deref(), Some("12%"));
    assert_eq!(num("$ZZ,ZZ9.99CR", "1234").as_deref(), Some("$ 1,234.00  "));
    assert_eq!(
        num("$ZZ,ZZ9.99CR", "-1234").as_deref(),
        Some("$ 1,234.00CR")
    );
    assert_eq!(num("$z,zz9.99db", "-1234").as_deref(), Some("$1,234.00DB"));
    assert_eq!(num("$z,zz9.99db", "12").as_deref(), Some("$12.00"));
    assert_eq!(num("99.888", "1.23456").as_deref(), Some("01.23456"));
}

#[test]
fn num_rounds_half_up_and_refuses_what_does_not_fit() {
    assert_eq!(num("z9.99", "1.995").as_deref(), Some("2.00"));
    assert_eq!(num("z9.9", "99.96").as_deref(), None);
    assert_eq!(num("z9", "-5").as_deref(), None);
    assert_eq!(num("z9", "abc").as_deref(), None);
    let e = format("99.999E", "12345", ValueKind::Num, true);
    assert_eq!(e.map(|s| s.approximated), Some(true));
}

#[test]
fn date_and_time_pictures() {
    assert_eq!(
        date("YYYY-MM-DD", "2020-01-05").as_deref(),
        Some("2020-01-05")
    );
    assert_eq!(date("D/M/YY", "20200105").as_deref(), Some("5/1/20"));
    assert_eq!(
        date("EEEE, MMMM D, YYYY", "2026-11-02").as_deref(),
        Some("Monday, November 2, 2026")
    );
    assert_eq!(
        date("date.medium{}", "2026-11-02").as_deref(),
        Some("Nov 2, 2026")
    );
    assert_eq!(date("JJJ", "2024-12-31").as_deref(), Some("366"));
    assert_eq!(date("WW", "2021-01-01").as_deref(), Some("53"));
    assert_eq!(date("WW", "2026-01-01").as_deref(), Some("01"));
    assert_eq!(date("YYYY-MM-DD", "2021-02-29"), None);
    let t = format("h:MM A", "13:05:00", ValueKind::Time, true).map(|s| s.text);
    assert_eq!(t.as_deref(), Some("1:05 PM"));
    let t = format("time{HH:MM:SS.FFF}", "07:08:09.5", ValueKind::Text, true);
    assert_eq!(t.map(|s| s.text).as_deref(), Some("07:08:09.500"));
}

#[test]
fn alternates_null_text_and_locale() {
    let p = "date{YYYY-MM-DD}|date{YYYYMMDD}";
    assert_eq!(date(p, "2020-01-05").as_deref(), Some("2020-01-05"));
    assert_eq!(
        format("null{'n/a'}|num{z9}", "", ValueKind::Num, true),
        Some(Shown {
            text: "n/a".to_owned(),
            approximated: false
        })
    );
    let t = format("text{A9A 9A9}", "K1S5K2", ValueKind::Text, true);
    assert_eq!(t.map(|s| s.text).as_deref(), Some("K1S 5K2"));
    assert_eq!(
        format("text{A9A 9A9}", "K1S5K", ValueKind::Text, true),
        None
    );
    let fr = format("MMMM", "2020-01-05", ValueKind::Date, false).unwrap();
    assert!(fr.approximated, "English names under another locale");
    let ok = format("YYYY", "2020-01-05", ValueKind::Date, false).unwrap();
    assert!(!ok.approximated);
}

#[test]
fn upper_v_shows_the_radix_and_lower_v_implies_it() {
    assert_eq!(num("S999V99", "1.23").as_deref(), Some(" 001.23"));
    assert_eq!(num("S999V99", "123").as_deref(), Some(" 123.00"));
    assert_eq!(num("SZZ9.99", "-12.3").as_deref(), Some("- 12.30"));
}
