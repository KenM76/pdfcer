use super::*;

fn presets(f: impl FnOnce(&mut PrintPresets)) -> PrintPresets {
    let mut p = PrintPresets::default();
    f(&mut p);
    p
}

fn request(f: impl FnOnce(&mut PrintRequest)) -> PrintRequest {
    let mut r = PrintRequest::default();
    f(&mut r);
    r
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
    let e = merge_print_presets(PrintRequest::default(), &PrintPresets::default(), 4, false);
    assert_eq!(e.scale, PrintScale::Fit);
    assert_eq!(e.pages, "all");
    assert_eq!(e.copies, 1);
    assert_eq!(e.duplex, PresetDuplex::Simplex);
    assert!(!e.pick_tray);
    assert!(e.notes.is_empty());
}

#[test]
fn presets_fill_unset_options_and_each_is_reported() {
    let e = merge_print_presets(PrintRequest::default(), &all_set(), 9, false);
    assert_eq!(e.scale, PrintScale::Actual);
    assert_eq!(e.pages, "2-3,5");
    assert_eq!(e.copies, 2);
    assert_eq!(e.duplex, PresetDuplex::FlipShortEdge);
    assert!(e.pick_tray);
    assert_eq!(
        e.notes,
        [
            PresetNote::ActualSize { enforced: false },
            PresetNote::Pages("2-3,5".into()),
            PresetNote::Copies(2),
            PresetNote::Duplex(PresetDuplex::FlipShortEdge),
            PresetNote::PickTrayBySize,
        ]
    );
}

#[test]
fn operator_options_win_over_presets() {
    let req = request(|r| {
        r.scale = Some(PrintScale::Shrink);
        r.pages = Some("1".into());
        r.copies = Some(4);
        r.duplex = Some(PresetDuplex::Simplex);
        r.pick_tray = true;
    });
    let e = merge_print_presets(req, &all_set(), 9, false);
    assert_eq!(e.scale, PrintScale::Shrink);
    assert_eq!(e.pages, "1");
    assert_eq!(e.copies, 4);
    assert_eq!(e.duplex, PresetDuplex::Simplex);
    assert!(e.notes.is_empty(), "{:?}", e.notes);
}

#[test]
fn a_scale_percent_also_counts_as_the_operator_choosing() {
    let req = request(|r| {
        r.scale_percent = Some(50);
    });
    let e = merge_print_presets(req, &all_set(), 9, false);
    assert_eq!(e.scale, PrintScale::Fit);
    assert_eq!(e.scale_percent, Some(50));
    assert!(
        !e.notes
            .iter()
            .any(|n| matches!(n, PresetNote::ActualSize { .. }))
    );
}

#[test]
fn enforced_actual_size_beats_an_explicit_scale_and_says_so() {
    let mut p = all_set();
    p.scaling_enforced = true;
    let req = request(|r| {
        r.scale = Some(PrintScale::Fit);
        r.scale_percent = Some(50);
    });
    let e = merge_print_presets(req, &p, 9, false);
    assert_eq!(e.scale, PrintScale::Actual);
    assert_eq!(e.scale_percent, None);
    assert_eq!(
        e.notes.first(),
        Some(&PresetNote::ActualSize { enforced: true })
    );
}

#[test]
fn ignore_drops_everything_including_enforce_and_says_so() {
    let mut p = all_set();
    p.scaling_enforced = true;
    let e = merge_print_presets(PrintRequest::default(), &p, 9, true);
    assert_eq!(e.scale, PrintScale::Fit);
    assert_eq!(e.pages, "all");
    assert_eq!(e.copies, 1);
    assert_eq!(
        e.notes,
        [PresetNote::AllIgnored, PresetNote::EnforcedOverridden]
    );
}

#[test]
fn ignore_with_no_presets_says_nothing() {
    let e = merge_print_presets(PrintRequest::default(), &PrintPresets::default(), 1, true);
    assert!(e.notes.is_empty());
}

#[test]
fn ranges_clip_to_the_last_page_and_report_skips() {
    let p = presets(|p| p.page_ranges = vec![(2, 8), (7, 9)]);
    let e = merge_print_presets(PrintRequest::default(), &p, 4, false);
    assert_eq!(e.pages, "2-4");
    assert_eq!(
        e.notes,
        [
            PresetNote::RangePastEnd {
                first: 7,
                last: 9,
                page_count: 4
            },
            PresetNote::Pages("2-4".into()),
        ]
    );
}

#[test]
fn a_range_wholly_past_the_end_leaves_all_pages() {
    let p = presets(|p| p.page_ranges = vec![(7, 9)]);
    assert_eq!(
        merge_print_presets(PrintRequest::default(), &p, 4, false).pages,
        "all"
    );
}

#[test]
fn copies_cap_at_the_limit() {
    let p = presets(|p| p.copies = Some(5000));
    let e = merge_print_presets(PrintRequest::default(), &p, 1, false);
    assert_eq!(e.copies, MAX_PRINT_COPIES);
    assert_eq!(
        e.notes,
        [
            PresetNote::CopiesClamped { asked: 5000 },
            PresetNote::Copies(MAX_PRINT_COPIES)
        ]
    );
}

#[test]
fn a_print_area_other_than_the_crop_box_is_disclosed() {
    let p = presets(|p| {
        p.print_area = Some(PresetBoundary::TrimBox);
        p.print_clip = Some(PresetBoundary::CropBox);
    });
    let e = merge_print_presets(PrintRequest::default(), &p, 1, false);
    assert_eq!(
        e.notes,
        [PresetNote::PrintAreaNotHonoured(PresetBoundary::TrimBox)]
    );
}

#[test]
fn unusable_entries_are_carried_last() {
    let p = presets(|p| {
        p.print_clip = Some(PresetBoundary::ArtBox);
        p.ignored = vec!["NumCopies: not a number".into()];
    });
    let e = merge_print_presets(PrintRequest::default(), &p, 1, false);
    assert_eq!(
        e.notes,
        [
            PresetNote::PrintClipNotHonoured(PresetBoundary::ArtBox),
            PresetNote::Unusable("NumCopies: not a number".into()),
        ]
    );
}
