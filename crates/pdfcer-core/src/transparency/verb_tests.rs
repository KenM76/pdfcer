#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use std::cell::Cell;

use super::*;
use crate::document::Document;
use crate::edit::{CommandKind, EditError, EditSession};
use crate::page_tree::Rect;
use crate::pageops::tests_support::build_pdf_bytes;
use crate::writer::SaveOptions;

/// A flattener that paints one opaque square and one 1x1 patch, counting
/// its calls.
struct Fake {
    mode: FlattenMode,
    fail: bool,
    calls: Cell<usize>,
}

impl Fake {
    fn new(mode: FlattenMode) -> Self {
        Self {
            mode,
            fail: false,
            calls: Cell::new(0),
        }
    }
}

impl TransparencyFlattener for Fake {
    fn flatten_page(
        &self,
        _view: &DocumentView<'_>,
        _page: usize,
        _options: &FlattenOptions,
    ) -> Result<FlattenedPage, FlattenFailure> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            return Err(FlattenFailure::new("no renderer"));
        }
        let mut page = FlattenedPage::new(self.mode);
        page.content = b"0.5 0.5 1 rg 10 10 50 50 re f q 1 0 0 1 0 0 cm /TrRaster1 Do Q".to_vec();
        page.images
            .push(RasterPatch::new("TrRaster1".into(), 1, 1, vec![255, 0, 0]).unwrap());
        page.vector_regions = 1;
        page.rasterised.push(RasterisedRegion::new(
            Rect {
                llx: 0.0,
                lly: 0.0,
                urx: 1.0,
                ury: 1.0,
            },
            RasterReason::Image,
        ));
        page.spot_inks.push("Gold".into());
        Ok(page)
    }
}

/// Page 1 and page 2 share graphics state 5 (`/ca 0.5`); page 3 is
/// opaque. Page 1 has a transparent annotation 9.
fn three_pages() -> Vec<u8> {
    let content = "<< /Length 25 >>\nstream\n/G gs 10 10 50 50 re f\nendstream";
    let plain = "<< /Length 13 >>\nstream\n0 0 10 10 re f\nendstream";
    build_pdf_bytes(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R 6 0 R 7 0 R] /Count 3 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Group << /S /Transparency >> /Resources << /ExtGState << /G 5 0 R >> >> /Contents 4 0 R /Annots [9 0 R] >>",
        ),
        (4, content),
        (5, "<< /Type /ExtGState /ca 0.5 >>"),
        (
            6,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /ExtGState << /G 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (
            7,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 8 0 R >>",
        ),
        (8, plain),
        (
            9,
            "<< /Type /Annot /Subtype /Square /Rect [0 0 20 20] /CA 0.5 >>",
        ),
    ])
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(three_pages()).unwrap())
}

fn in_content(s: &EditSession, page: usize) -> bool {
    has_transparency(&s.view(), page).unwrap().in_page_content()
}

/// The object numbers an incremental update appended after `base`.
fn appended_numbers(base: &[u8], saved: &[u8]) -> Vec<u32> {
    assert!(
        saved.starts_with(base),
        "an incremental save keeps the base"
    );
    let tail = String::from_utf8_lossy(&saved[base.len()..]).into_owned();
    tail.split('\n')
        .filter_map(|line| line.strip_suffix(" 0 obj"))
        .filter_map(|n| n.trim().parse().ok())
        .collect()
}

#[test]
fn overlay_switches_off_the_page_and_leaves_the_rest() {
    let mut s = session();
    let fake = Fake::new(FlattenMode::Overlay);
    let report = s
        .flatten_transparency(&[0, 2], &FlattenOptions::new(), &fake)
        .unwrap();
    assert!(report.changed);
    assert_eq!(fake.calls.get(), 1, "the opaque page is not flattened");
    assert_eq!(report.pages[0].mode, Some(FlattenMode::Overlay));
    assert_eq!(report.pages[1].mode, None);
    let annot = ObjId {
        num: 9,
        generation: 0,
    };
    assert_eq!(report.pages[0].annotations_left, vec![annot]);
    let text = report.disclosures.join("\n");
    for want in [
        "rasterised at 300 ppi",
        "(image)",
        "Gold",
        "left 1 annotation",
        "untouched",
    ] {
        assert!(text.contains(want), "{want} missing from {text}");
    }

    assert!(!in_content(&s, 0));
    assert!(in_content(&s, 1), "page 2 shares the state and keeps it");
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(
        s.undo_kind(),
        Some(CommandKind::FlattenTransparency { pages: 1 })
    );

    let base = three_pages();
    let (saved, _) = s.to_incremental_bytes(&SaveOptions::default()).unwrap();
    let written = appended_numbers(&base, &saved);
    assert!(written.contains(&3), "{written:?}");
    for untouched in [4, 5, 6, 7, 8, 9] {
        assert!(!written.contains(&untouched), "{untouched} in {written:?}");
    }
    let reopened = Document::from_bytes(saved).unwrap();
    assert!(
        !has_transparency(&reopened.view(), 0)
            .unwrap()
            .in_page_content()
    );
    assert!(
        has_transparency(&reopened.view(), 1)
            .unwrap()
            .in_page_content()
    );

    assert_eq!(
        s.undo(),
        Some(CommandKind::FlattenTransparency { pages: 1 })
    );
    assert!(in_content(&s, 0));
}

#[test]
fn rebuilt_replaces_the_content() {
    let mut s = session();
    let fake = Fake::new(FlattenMode::Rebuilt);
    let options = FlattenOptions::new().with_keep_text(false);
    let report = s.flatten_transparency(&[0], &options, &fake).unwrap();
    assert_eq!(report.pages[0].mode, Some(FlattenMode::Rebuilt));
    let graph = s.graph();
    let page = graph
        .resolved(ObjId {
            num: 3,
            generation: 0,
        })
        .as_dict()
        .unwrap()
        .clone();
    assert!(page.get(b"Group").is_none());
    let contents = page
        .get(b"Contents")
        .and_then(Object::as_reference)
        .unwrap();
    assert_ne!(contents.num, 4);
    let res = page.get(b"Resources").and_then(Object::as_dict).unwrap();
    assert!(res.get(b"ExtGState").is_none());
    assert!(!in_content(&s, 0));
}

#[test]
fn a_failure_or_a_bad_resolution_writes_nothing() {
    let mut s = session();
    let mut fake = Fake::new(FlattenMode::Overlay);
    fake.fail = true;
    let err = s
        .flatten_transparency(&[0], &FlattenOptions::new(), &fake)
        .unwrap_err();
    assert!(matches!(
        err,
        EditError::TransparencyFlattenFailed { page: 0, .. }
    ));
    let low = FlattenOptions::new().with_raster_ppi(10.0);
    let err = s.flatten_transparency(&[0], &low, &fake).unwrap_err();
    assert!(matches!(err, EditError::FlattenPpiOutOfRange { .. }));
    let err = s
        .flatten_transparency(&[5], &FlattenOptions::new(), &fake)
        .unwrap_err();
    assert!(matches!(err, EditError::PageOutOfRange { index: 5, .. }));
    assert_eq!(s.undo_depth(), 0);
    assert!(in_content(&s, 0));
}

#[test]
fn an_all_opaque_request_is_not_an_edit() {
    let mut s = session();
    let fake = Fake::new(FlattenMode::Overlay);
    let report = s
        .flatten_transparency(&[2], &FlattenOptions::new(), &fake)
        .unwrap();
    assert!(!report.changed);
    assert_eq!(s.undo_depth(), 0);
    assert_eq!(fake.calls.get(), 0);
}
