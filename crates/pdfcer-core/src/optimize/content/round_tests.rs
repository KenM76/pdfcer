#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::rewrite::{Compacted, compact};
use super::*;
use crate::content::ContentStream;
use crate::optimize::tests::{build, id, session};
use crate::optimize::{OptimizeSettings, SkipReason};
use crate::space_audit::tests::stream;

fn within(units: f64, page_start: bool) -> ResolvedContent {
    let mut s = ResolvedContent::base(StreamAction::Leave);
    s.round = Tolerance::from_units(units);
    s.page_start = page_start;
    s
}

fn rounded(src: &[u8], units: f64) -> (String, Compacted) {
    let out = compact(src, within(units, true)).unwrap();
    (String::from_utf8(out.bytes.clone()).unwrap(), out)
}

#[test]
fn round_parses_prints_and_is_lossy() {
    let p = ContentPatch::parse("content(round=0.01)").unwrap();
    let t = Tolerance::from_units(0.01).unwrap();
    assert_eq!(p.round, Some(Rounding::Within(t)));
    assert_eq!(p.to_string(), "content(round=0.01)");
    assert!(StreamAction::Content(p).is_lossy());
    let off = ContentPatch::parse("content(round=off)").unwrap();
    assert_eq!(off.round, Some(Rounding::Off));
    assert!(!StreamAction::Content(off).is_lossy());
    for bad in ["0", "-1", "73", "x", "nan", "0.0000001"] {
        let spec = format!("content(round={bad})");
        assert!(ContentPatch::parse(&spec).is_err(), "{spec}");
    }
}

#[test]
fn the_fewest_decimals_within_tolerance_are_measured_through_cm() {
    let (text, out) = rounded(b"1.23456 2.5 m q 10 0 0 10 0 0 cm 1.23456 2.5 m Q", 0.01);
    assert!(text.starts_with("1.23 2.5 m\n"), "{text}");
    assert!(text.contains("cm\n1.235 2.5 m"), "{text}");
    assert_eq!(out.rounded.count, 2);
    assert!(out.rounded.max <= 0.01 && out.rounded.max > 0.0);
    assert_eq!(out.rounded.unrounded, None);
}

#[test]
fn every_corner_of_a_rectangle_stays_within_tolerance() {
    // Each operand alone moves 0.006 at zero decimals; the far corners
    // move 0.012.
    let (text, _) = rounded(b"0.006 0 10.006 5 re f", 0.01);
    assert!(text.starts_with(".01 0 10.01 5 re"), "{text}");
}

#[test]
fn only_the_first_stream_of_a_page_rounds() {
    let out = compact(b"1.23456 2 m", within(0.01, false)).unwrap();
    assert_eq!(out.rounded.count, 0);
    assert_eq!(out.rounded.unrounded, Some(Unrounded::NotPageStart));
}

#[test]
fn an_unknown_matrix_stops_rounding() {
    for src in [
        &b"1.23456 2 m Q 1.23456 2 m"[..],
        b"1.23456 2 m /M cm 1.23456 2 m",
    ] {
        let (text, out) = rounded(src, 0.01);
        assert_eq!(out.rounded.count, 1, "{text}");
        assert_eq!(out.rounded.unrounded, Some(Unrounded::UnknownMatrix));
        assert!(text.ends_with("1.23456 2 m"), "{text}");
    }
}

#[test]
fn the_check_compares_a_rounded_operand_with_its_rounded_value() {
    let original = ContentStream::parse(b"1.23456 2 m".to_vec()).unwrap();
    let keep = vec![true; original.tokens.len()];
    let mut text = vec![None; original.tokens.len()];
    text[0] = Some(b"1.23".to_vec());
    assert!(super::check::same(&original, (&keep, &text), b"1.23 2 m"));
    assert!(!super::check::same(&original, (&keep, &text), b"1.24 2 m"));
    assert!(!super::check::same(
        &original,
        (&keep, &text),
        b"1.23456 2 m"
    ));
}

/// Pages `[4 5]` and `[7 4]`: only 7 opens every page that draws it.
fn two_page_doc() -> Vec<u8> {
    let body = b"0.123456 0.123456 m 100.123456 100.123456 l S\n".repeat(50);
    build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [4 0 R 5 0 R] >>".to_vec(),
        stream("", &body),
        stream("", &body),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [7 0 R 4 0 R] >>".to_vec(),
        stream("", &body),
    ])
}

#[test]
fn a_plan_rounds_page_openers_and_marks_them_lossy() {
    let mut settings = OptimizeSettings::new();
    settings.content = Some(ContentPatch::parse("content(round=0.01)").unwrap());
    let mut s = session(two_page_doc());
    let plan = s.plan_optimize(&settings).unwrap();
    let change = |n| plan.changes.iter().find(|c| c.id == id(n)).unwrap();
    let opener = change(7);
    assert!(opener.lossy);
    assert_eq!(opener.content.unwrap().numbers_rounded, 50 * 4);
    for n in [4, 5] {
        let c = change(n);
        assert!(!c.lossy, "{n}");
        assert_eq!(c.content.unwrap().numbers_rounded, 0);
        assert_eq!(c.content.unwrap().unrounded, Some(Unrounded::NotPageStart));
    }
    assert!(plan.is_lossy());
    assert!(
        !plan
            .skipped
            .iter()
            .any(|k| matches!(k.reason, SkipReason::Content(_)))
    );
    s.apply_optimize(&plan).unwrap();
}

#[test]
fn a_td_carries_the_error_of_the_lines_before_it() {
    // Rounded alone, each would write 10 and the third line would sit 0.012
    // from where it belongs.
    let (text, out) = rounded(b"BT 10.004 0 Td 10.004 0 Td 10.004 0 Td ET", 0.01);
    assert!(text.contains("10 0 Td\n10 0 Td\n10.01 0 Td"), "{text}");
    assert_eq!(out.rounded.count, 3);
    assert!(out.rounded.max <= 0.01 && out.rounded.max > 0.0);
}

#[test]
fn a_tm_rounds_its_translation_and_its_scale_reaches_each_td() {
    let (text, _) = rounded(b"BT 1 0 0 1 100.123456 50.5 Tm ET", 0.01);
    assert!(text.contains("1 0 0 1 100.12 50.5 Tm"), "{text}");
    // At ten times the scale, 0.004 of text space is 0.04 of user space,
    // 0.0004 is 0.004.
    let (text, _) = rounded(b"BT 10 0 0 10 0 0 Tm 1.004 0 Td 1.0004 0 Td ET", 0.01);
    assert!(text.contains("1.004 0 Td\n1 0 Td"), "{text}");
    let (text, _) = rounded(b"BT 1.004 0 Td ET", 0.01);
    assert!(text.contains("1 0 Td"), "{text}");
}

#[test]
fn td_sets_the_leading_and_is_left_as_written() {
    let (text, out) = rounded(b"BT 1.23456 2.5 TD ET", 0.01);
    assert!(text.contains("1.23456 2.5 TD"), "{text}");
    assert_eq!(out.rounded.count, 0);
}

#[test]
fn a_matrix_change_inside_a_text_object_unrounds_it() {
    let (text, out) = rounded(b"BT 1.23456 0 Td 2 0 0 2 0 0 cm 1.23456 0 Td ET", 0.01);
    assert!(text.contains("1.23456 0 Td"), "{text}");
    assert_eq!(out.rounded.count, 0, "{text}");
}

#[test]
fn a_line_start_counts_the_error_of_its_tm_too() {
    // The Tm writes 0 for 0.006 and the Td would too: 0.012 together.
    let (text, _) = rounded(b"BT 1 0 0 1 .006 0 Tm .006 0 Td ET", 0.01);
    assert!(text.contains("1 0 0 1 0 0 Tm\n.01 0 Td"), "{text}");
}
