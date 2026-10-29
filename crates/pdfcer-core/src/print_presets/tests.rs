use super::*;
use crate::pageops::tests_support::build_pdf;

fn read(vp: &str) -> PrintPresets {
    let catalog = format!("<< /Type /Catalog /Pages 2 0 R {vp} >>");
    let doc = build_pdf(&[
        (1, catalog.as_str()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        (4, "[1 2 5 5]"),
    ]);
    print_presets(&doc)
}

#[test]
fn no_viewer_preferences_reads_empty() {
    let p = read("");
    assert!(p.is_empty());
    assert!(p.ignored.is_empty());
}

#[test]
fn every_entry_reads() {
    let p = read(
        "/ViewerPreferences << /PrintScaling /None /Duplex /DuplexFlipLongEdge \
         /PickTrayByPDFSize true /PrintPageRange [1 2 5 5] /NumCopies 3 \
         /PrintArea /TrimBox /PrintClip /BleedBox >>",
    );
    assert_eq!(p.scaling, Some(PresetScaling::None));
    assert!(!p.scaling_enforced);
    assert_eq!(p.duplex, Some(PresetDuplex::FlipLongEdge));
    assert_eq!(p.pick_tray_by_pdf_size, Some(true));
    assert_eq!(p.page_ranges, vec![(1, 2), (5, 5)]);
    assert_eq!(p.copies, Some(3));
    assert_eq!(p.print_area, Some(PresetBoundary::TrimBox));
    assert_eq!(p.print_clip, Some(PresetBoundary::BleedBox));
    assert!(p.ignored.is_empty(), "{:?}", p.ignored);
}

#[test]
fn an_unknown_scaling_name_reads_as_app_default() {
    assert_eq!(
        read("/ViewerPreferences << /PrintScaling /Shrink >>").scaling,
        Some(PresetScaling::AppDefault)
    );
}

#[test]
fn enforce_counts_only_with_scaling_none() {
    let on = read("/ViewerPreferences << /PrintScaling /None /Enforce [/PrintScaling] >>");
    assert!(on.scaling_enforced);
    let app = read("/ViewerPreferences << /PrintScaling /AppDefault /Enforce [/PrintScaling] >>");
    assert!(!app.scaling_enforced);
    let other = read("/ViewerPreferences << /PrintScaling /None /Enforce [/Other] >>");
    assert!(!other.scaling_enforced);
}

#[test]
fn copies_below_one_are_ignored_and_reported() {
    for v in ["0", "-1"] {
        let p = read(&format!("/ViewerPreferences << /NumCopies {v} >>"));
        assert_eq!(p.copies, None);
        assert_eq!(p.ignored.len(), 1, "{:?}", p.ignored);
    }
}

#[test]
fn bad_ranges_are_dropped_and_reported() {
    let p = read("/ViewerPreferences << /PrintPageRange [0 2 4 3 6 7 9] >>");
    assert_eq!(p.page_ranges, vec![(6, 7)]);
    assert_eq!(p.ignored.len(), 3, "{:?}", p.ignored);
}

#[test]
fn an_indirect_range_array_resolves() {
    let p = read("/ViewerPreferences << /PrintPageRange 4 0 R >>");
    assert_eq!(p.page_ranges, vec![(1, 2), (5, 5)]);
}

#[test]
fn wrong_types_are_reported_not_used() {
    let p = read(
        "/ViewerPreferences << /Duplex /Sideways /PickTrayByPDFSize 1 /PrintArea /Paper \
         /PrintScaling (None) >>",
    );
    assert!(p.is_empty());
    assert_eq!(p.ignored.len(), 4, "{:?}", p.ignored);
}
