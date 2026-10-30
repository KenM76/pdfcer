// Test code: a failed fixture or unexpected shape should panic.
#![allow(clippy::expect_used, clippy::panic)]

use pdfcer_core::compare::RegionKind;

use super::*;
use crate::ChangedRegion;
use crate::visual_compare::VisualRegion;

fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
    Rect { llx, lly, urx, ury }
}

fn pair(old: Option<usize>, new: Option<usize>, how: PageMatch, regions: usize) -> PagePair {
    let region = VisualRegion {
        pixels: ChangedRegion {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        rect: rect(10.0, 20.0, 30.0, 40.0),
        changed_pixels: 42,
    };
    PagePair {
        old,
        new,
        how,
        similarity: None,
        size_differs: false,
        regions: vec![region; regions],
        error: None,
    }
}

fn report(pairs: Vec<PagePair>, automatic: bool) -> VisualCompareReport {
    VisualCompareReport {
        pairs,
        scale: 1.5,
        threshold: 48,
        merge_gap: 4.0,
        min_pixels: 6,
        match_threshold: 0.5,
        matched_automatically: automatic,
    }
}

/// Two pages: 200x300 and 400x500, so the crop boxes are told apart.
fn two_pages() -> Document {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 500] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).expect("fixture parses")
}

#[test]
fn match_text_names_each_row_one_based() {
    let t = |old, new, how| pair(old, new, how, 0).match_text();
    assert_eq!(t(Some(0), Some(0), PageMatch::InOrder), "old p1 = new p1");
    assert_eq!(
        t(Some(1), Some(2), PageMatch::Moved),
        "old p2 moved to new p3"
    );
    assert_eq!(
        t(Some(0), Some(1), PageMatch::Mapped),
        "old p1 = new p2 (mapped)"
    );
    assert_eq!(
        t(None, Some(3), PageMatch::Inserted),
        "new p4 inserted (no matching old page)"
    );
    assert_eq!(
        t(Some(2), None, PageMatch::Deleted),
        "old p3 deleted (no matching new page)"
    );
}

#[test]
fn settings_text_says_how_pages_were_paired() {
    let auto = report(Vec::new(), true).settings_text();
    assert_eq!(
        auto,
        "108 dpi, tolerance 48, regions merged within 4 pt and under 6 px dropped; \
         pages matched by appearance (paired at similarity 0.5 or more)"
    );
    let given = report(Vec::new(), false).settings_text();
    assert!(given.ends_with("; pages paired as given"), "{given}");
    assert!(
        !given.contains("--"),
        "no CLI flag in a note written into the PDF: {given}"
    );
}

#[test]
fn summary_note_lists_only_rows_not_in_order() {
    let r = report(
        vec![
            pair(Some(0), Some(0), PageMatch::InOrder, 2),
            pair(None, Some(1), PageMatch::Inserted, 0),
        ],
        true,
    );
    let note = r.summary_note();
    assert!(note.starts_with("Visual comparison: 2 changed area(s), each an orange cloud"));
    assert!(note.contains(&r.settings_text()));
    assert!(note.contains("new p2 inserted"));
    assert!(!note.contains("old p1 = new p1"), "{note}");
    assert!(!note.contains("No page of one document"));
}

#[test]
fn summary_note_says_when_nothing_matched() {
    let r = report(
        vec![
            pair(None, Some(0), PageMatch::Inserted, 0),
            pair(Some(0), None, PageMatch::Deleted, 0),
        ],
        true,
    );
    assert!(
        r.summary_note()
            .ends_with("No page of one document looked like any page of the other.\n")
    );
}

#[test]
fn region_marks_frame_inserted_pages_and_cloud_changed_regions() {
    let r = report(
        vec![
            pair(Some(0), Some(0), PageMatch::InOrder, 2),
            pair(None, Some(1), PageMatch::Inserted, 0),
            pair(Some(1), None, PageMatch::Deleted, 0),
            pair(None, Some(9), PageMatch::Inserted, 0),
        ],
        true,
    );
    let marks = r.region_marks(&two_pages());
    assert_eq!(marks.len(), 3, "deleted and out-of-range rows get none");
    let [a, b, c] = marks.as_slice() else {
        panic!("{marks:?}")
    };
    for m in [a, b] {
        assert_eq!((m.page, m.kind), (0, RegionKind::Changed));
        assert_eq!(m.rect, rect(10.0, 20.0, 30.0, 40.0));
        assert_eq!(
            m.note,
            "Changed appearance (compared with old page 1; 42 px)"
        );
    }
    assert_eq!((c.page, c.kind), (1, RegionKind::InsertedPage));
    assert_eq!(c.rect, rect(0.0, 0.0, 400.0, 500.0));
    assert_eq!(c.note, INSERTED_PAGE_NOTE);
}
