use super::*;

fn presets(f: impl FnOnce(&mut PrintPresets)) -> PrintPresets {
    let mut p = PrintPresets::default();
    f(&mut p);
    p
}

fn all_set() -> PrintPresets {
    presets(|p| {
        p.scaling = Some(PresetScaling::None);
        p.duplex = Some(PresetDuplex::FlipShortEdge);
        p.pick_tray_by_pdf_size = Some(true);
        p.page_ranges = vec![(2, 3), (5, 5)];
        p.copies = Some(2);
    })
}

#[test]
fn no_presets_gives_the_documented_defaults_and_no_notes() {
    let e = merge(Requested::default(), &PrintPresets::default(), 4, false);
    assert_eq!(e.scale, PrintScaleArg::Fit);
    assert_eq!(e.pages, "all");
    assert_eq!(e.copies, 1);
    assert_eq!(e.duplex, DuplexArg::Simplex);
    assert!(!e.pick_tray);
    assert!(e.notes.is_empty());
}

#[test]
fn presets_fill_unset_options_and_each_is_reported() {
    let e = merge(Requested::default(), &all_set(), 9, false);
    assert_eq!(e.scale, PrintScaleArg::Actual);
    assert_eq!(e.pages, "2-3,5");
    assert_eq!(e.copies, 2);
    assert_eq!(e.duplex, DuplexArg::ShortEdge);
    assert!(e.pick_tray);
    assert_eq!(e.notes.len(), 5, "{:?}", e.notes);
}

#[test]
fn operator_options_win_over_presets() {
    let req = Requested {
        scale: Some(PrintScaleArg::Shrink),
        pages: Some("1".into()),
        copies: Some(4),
        duplex: Some(DuplexArg::Simplex),
        pick_tray: true,
        ..Requested::default()
    };
    let e = merge(req, &all_set(), 9, false);
    assert_eq!(e.scale, PrintScaleArg::Shrink);
    assert_eq!(e.pages, "1");
    assert_eq!(e.copies, 4);
    assert_eq!(e.duplex, DuplexArg::Simplex);
    assert!(e.notes.is_empty(), "{:?}", e.notes);
}

#[test]
fn a_scale_percent_also_counts_as_the_operator_choosing() {
    let req = Requested {
        scale_percent: Some(50),
        ..Requested::default()
    };
    let e = merge(req, &all_set(), 9, false);
    assert_eq!(e.scale, PrintScaleArg::Fit);
    assert_eq!(e.scale_percent, Some(50));
}

#[test]
fn enforced_actual_size_beats_an_explicit_scale_and_says_so() {
    let mut p = all_set();
    p.scaling_enforced = true;
    let req = Requested {
        scale: Some(PrintScaleArg::Fit),
        scale_percent: Some(50),
        ..Requested::default()
    };
    let e = merge(req, &p, 9, false);
    assert_eq!(e.scale, PrintScaleArg::Actual);
    assert_eq!(e.scale_percent, None);
    assert!(
        e.notes.iter().any(|n| n.contains("/Enforce")),
        "{:?}",
        e.notes
    );
}

#[test]
fn ignore_drops_everything_including_enforce_and_says_so() {
    let mut p = all_set();
    p.scaling_enforced = true;
    let e = merge(Requested::default(), &p, 9, true);
    assert_eq!(e.scale, PrintScaleArg::Fit);
    assert_eq!(e.pages, "all");
    assert_eq!(e.copies, 1);
    assert_eq!(e.notes.len(), 2, "{:?}", e.notes);
}

#[test]
fn ranges_clip_to_the_last_page_and_report_skips() {
    let p = presets(|p| p.page_ranges = vec![(2, 8), (7, 9)]);
    let e = merge(Requested::default(), &p, 4, false);
    assert_eq!(e.pages, "2-4");
    assert!(e.notes.iter().any(|n| n.contains("7-9")), "{:?}", e.notes);
}

#[test]
fn a_range_wholly_past_the_end_leaves_all_pages() {
    let p = presets(|p| p.page_ranges = vec![(7, 9)]);
    assert_eq!(merge(Requested::default(), &p, 4, false).pages, "all");
}

#[test]
fn copies_cap_at_the_cli_limit() {
    let p = presets(|p| p.copies = Some(5000));
    let e = merge(Requested::default(), &p, 1, false);
    assert_eq!(e.copies, 999);
    assert!(e.notes.iter().any(|n| n.contains("5000")), "{:?}", e.notes);
}

#[test]
fn a_print_area_other_than_the_crop_box_is_disclosed() {
    let p = presets(|p| {
        p.print_area = Some(PresetBoundary::TrimBox);
        p.print_clip = Some(PresetBoundary::CropBox);
    });
    let e = merge(Requested::default(), &p, 1, false);
    assert_eq!(e.notes.len(), 1, "{:?}", e.notes);
    assert!(e.notes[0].contains("TrimBox"));
}
