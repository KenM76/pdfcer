// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
    Rect { llx, lly, urx, ury }
}

#[test]
fn check_refuses_nothing_bad_sizes_and_unencodable_labels() {
    assert_eq!(
        PrinterMarks::none("x").check(),
        Err(PrinterMarksError::NoMarks)
    );
    let mut m = PrinterMarks::all("job");
    assert_eq!(m.check(), Ok(()));
    m.length = 0.0;
    assert!(matches!(
        m.check(),
        Err(PrinterMarksError::Geometry { what: "length", .. })
    ));
    m.length = 18.0;
    m.offset = f64::NAN;
    assert!(matches!(m.check(), Err(PrinterMarksError::Geometry { .. })));
    m.offset = 0.0;
    assert_eq!(m.check(), Ok(()));
    m.label = "\u{65e5}".into();
    assert_eq!(m.check(), Err(PrinterMarksError::Unencodable('\u{65e5}')));
}

#[test]
fn the_sheet_grows_by_offset_plus_length_and_the_gap_clears_the_bleed() {
    let m = PrinterMarks::all("");
    let trim = rect(0.0, 0.0, 612.0, 792.0);
    let flush = MarksLayout::new(&m, trim, trim);
    assert_eq!(flush.gap, 6.0);
    assert_eq!(flush.band(), 24.0);
    assert_eq!(flush.sheet, rect(-24.0, -24.0, 636.0, 816.0));
    let bled = MarksLayout::new(&m, trim, rect(-9.0, -9.0, 621.0, 801.0));
    assert_eq!(bled.gap, 9.0);
    assert_eq!(bled.band(), 27.0);
}

#[test]
fn a_bleed_smaller_than_the_trim_is_taken_as_the_trim() {
    let trim = rect(10.0, 10.0, 100.0, 100.0);
    let l = MarksLayout::new(&PrinterMarks::all(""), trim, rect(20.0, 20.0, 90.0, 90.0));
    assert_eq!(l.bleed, trim);
}

#[test]
fn every_mark_stays_outside_the_trim_box() {
    let m = PrinterMarks::all("job.pdf");
    let trim = rect(0.0, 0.0, 612.0, 792.0);
    let layout = MarksLayout::new(&m, trim, rect(-9.0, -9.0, 621.0, 801.0));
    let content = String::from_utf8(marks_content(&m, &layout, b"All0", b"F0", "x")).unwrap();
    let nums: Vec<f64> = content
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    assert!(content.starts_with("\nQ\n"));
    assert!(content.contains("/All0 CS 1 SCN"));
    assert!(content.contains("<</Type /Page>> BDC"));
    // Each `m`/`l` pair: at least one end is outside the trim, and no
    // stroke crosses into it.
    for seg in content.lines().filter(|l| l.ends_with(" l S")) {
        let v: Vec<f64> = seg
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        let inside = |x: f64, y: f64| x > 0.0 && x < 612.0 && y > 0.0 && y < 792.0;
        assert!(!inside(v[0], v[1]) && !inside(v[2], v[3]), "{seg}");
    }
    assert!(nums.iter().all(|v| v.is_finite()));
}

#[test]
fn each_mark_can_be_drawn_alone() {
    let trim = rect(0.0, 0.0, 300.0, 300.0);
    let mut m = PrinterMarks::none("");
    m.color_bars = true;
    let l = MarksLayout::new(&m, trim, trim);
    let c = String::from_utf8(marks_content(&m, &l, b"A", b"F", "")).unwrap();
    assert!(c.contains(" k "));
    assert!(!c.contains(" l S"));
    assert!(!c.contains("Tj"));
    m.color_bars = false;
    m.page_info = true;
    let c = String::from_utf8(marks_content(&m, &l, b"A", b"F", "a(b)")).unwrap();
    assert!(c.contains("(a\\(b\\)) Tj"));
}

#[test]
fn bleed_marks_are_skipped_when_bleed_equals_trim() {
    let trim = rect(0.0, 0.0, 300.0, 300.0);
    let mut m = PrinterMarks::none("");
    m.bleed = true;
    let l = MarksLayout::new(&m, trim, trim);
    let c = String::from_utf8(marks_content(&m, &l, b"A", b"F", "")).unwrap();
    assert!(!c.contains(" l S"));
}

#[test]
fn info_line_names_the_page() {
    assert_eq!(
        PrinterMarks::all(" a.pdf ").info_line(2, 5),
        "a.pdf   page 2 of 5" // string-gap-exempt: the deliberate page-info separator
    );
    assert_eq!(PrinterMarks::all("").info_line(1, 1), "page 1 of 1");
}

#[test]
fn clip_is_the_bleed_rectangle() {
    assert_eq!(
        clip_content(rect(-9.0, -9.0, 621.0, 801.0)),
        b"q -9 -9 630 810 re W n\n".to_vec()
    );
}
