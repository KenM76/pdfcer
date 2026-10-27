//! `EditSession::bake_missing_appearances` (decision 203, opt-in half): the
//! look drawn for an `/AP`-less markup annotation, written as its `/AP`.
//! The page renders the same before and after, and now through the `/AP`.

use crate::named_icon_without_ap::{no_ap_pdf, render};
use pdfcer_core::annot_author::FreeTextC;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{RenderOptions, RenderedPage};

const ID: ObjId = ObjId::new(4, 0);

fn bake(bytes: Vec<u8>, mode: FreeTextC) -> (EditSession, Vec<u8>) {
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let outcome = s.bake_missing_appearances(mode).unwrap();
    assert_eq!(outcome.baked.len(), 1, "skipped: {:?}", outcome.skipped);
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    (s, saved)
}

/// How many bytes differ, and the largest difference.
fn diff(a: &RenderedPage, b: &RenderedPage) -> (usize, u8) {
    let pairs = a.pixmap.data().iter().zip(b.pixmap.data());
    let d: Vec<u8> = pairs
        .map(|(x, y)| x.abs_diff(*y))
        .filter(|d| *d > 0)
        .collect();
    (d.len(), d.iter().copied().max().unwrap_or(0))
}

fn render_opts(bytes: Vec<u8>, options: &RenderOptions) -> RenderedPage {
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    pdfcer_render::render_page_with(&doc, &pages[0], 1.0, options).unwrap()
}

/// Rendered without `/AP`, then baked, saved, reopened and rendered from
/// the `/AP`: the same pixels.
#[test]
fn the_baked_page_renders_as_the_unbaked_one_did() {
    for annot in [
        "<< /Type /Annot /Subtype /Ink /C [1 0 0] /InkList [[50 50 250 250]] \
         /Rect [40 40 260 260] >>",
        "<< /Type /Annot /Subtype /Highlight /C [1 1 0] \
         /QuadPoints [50 130 250 130 50 110 250 110] /Rect [40 100 260 140] >>",
        "<< /Type /Annot /Subtype /Square /C [0 0 1] /BS << /W 2 >> \
         /Rect [40 40 260 260] /RD [50 50 50 50] >>",
        "<< /Type /Annot /Subtype /Square /IC [0 0 0] /CA 0.5 /Rect [40 40 260 260] >>",
        "<< /Type /Annot /Subtype /Line /C [0 0 0] /L [50 150 250 150] \
         /LE [/OpenArrow /ClosedArrow] /BS << /W 3 >> /Rect [48 148 252 152] >>",
        "<< /Type /Annot /Subtype /Polygon /C [1 0 0] /Vertices [50 50 250 50 150 250] \
         /Rect [40 40 260 260] >>",
        "<< /Type /Annot /Subtype /FreeText /Contents (Hello) /DA (/Helv 12 Tf 0 g) \
         /C [1 0 0] /RD [20 10 20 10] /Rect [40 40 260 120] >>",
    ] {
        let before = render(no_ap_pdf("4 0 R", annot, 1));
        assert_eq!(before.diagnostics.annotations_painted, 0, "{annot}");
        let (_, saved) = bake(no_ap_pdf("4 0 R", annot, 1), FreeTextC::Fill);
        let after = render(saved);
        assert_eq!(after.diagnostics.annotations_painted, 1, "{annot}: via /AP");
        assert!(after.diagnostics.annotations_geometry_drawn.is_empty());
        assert_eq!(diff(&before, &after), (0, 0), "{annot}");
    }
}

/// The FreeText reading chosen at bake time is the one written.
#[test]
fn the_free_text_reading_is_baked_in() {
    let annot = "<< /Type /Annot /Subtype /FreeText /Contents (Hi) /DA (/Helv 12 Tf 0 g) \
                 /C [1 0 0] /Rect [40 40 260 120] >>";
    let border = RenderOptions::default().with_free_text_c(FreeTextC::Border);
    let before = render_opts(no_ap_pdf("4 0 R", annot, 1), &border);
    let (_, saved) = bake(no_ap_pdf("4 0 R", annot, 1), FreeTextC::Border);
    // Rendered with the default (Fill): the /AP, not the setting, decides.
    assert_eq!(diff(&render(saved), &before), (0, 0));
}

/// An incremental save appends the annotation and its new stream only, and
/// keeps every key the annotation had.
#[test]
fn the_save_is_minimal_and_keeps_the_annotation_keys() {
    let annot = "<< /Type /Annot /Subtype /Ink /C [1 0 0] /InkList [[50 50 250 250]] \
                 /T (Ken) /NM (ink-1) /Rect [40 40 260 260] >>";
    let original = no_ap_pdf("4 0 R", annot, 1);
    let (_, saved) = bake(original.clone(), FreeTextC::Fill);
    assert!(saved.starts_with(&original), "an append, not a rewrite");
    let tail = String::from_utf8_lossy(&saved[original.len()..]).into_owned();
    assert!(tail.contains("4 0 obj") && tail.contains("5 0 obj"));
    assert!(!tail.contains("3 0 obj"), "the page is not rewritten");
    let doc = Document::from_bytes(saved).unwrap();
    let Object::Dict(d) = doc.get(ID).unwrap().value.clone() else {
        panic!("not a dict")
    };
    for key in [&b"T"[..], b"NM", b"InkList", b"C", b"AP"] {
        assert!(d.get(key).is_some(), "{}", String::from_utf8_lossy(key));
    }
}

#[test]
fn a_line_whose_caps_pass_its_rect_changes_the_rect() {
    let annot = "<< /Type /Annot /Subtype /Line /C [0 0 0] /L [50 150 250 150] \
                 /LE [/OpenArrow /None] /BS << /W 3 >> /Rect [50 150 250 150] >>";
    let mut s = EditSession::new(Document::from_bytes(no_ap_pdf("4 0 R", annot, 1)).unwrap());
    let out = s.bake_missing_appearances(FreeTextC::Fill).unwrap();
    assert!(out.baked[0].rect_changed);
}

#[test]
fn one_undo_removes_every_bake() {
    let bytes = no_ap_pdf(
        "4 0 R 5 0 R",
        "<< /Type /Annot /Subtype /Ink /C [1 0 0] /InkList [[50 50 250 250]] \
         /Rect [40 40 260 260] >>|\
         << /Type /Annot /Subtype /Circle /C [0 0 1] /Rect [40 40 200 200] >>",
        2,
    );
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let out = s.bake_missing_appearances(FreeTextC::Fill).unwrap();
    assert_eq!(out.baked.len(), 2);
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.undo(), Some(CommandKind::BakeAppearances { count: 2 }));
    let Some(Object::Dict(d)) = s.value(ID) else {
        panic!("annotation")
    };
    assert!(d.get(b"AP").is_none());
}

/// What is declined is reported with its reason and left as it was; an
/// annotation outside the class is not mentioned; a second run is a no-op.
#[test]
fn declined_and_locked_annotations_are_skipped_by_name() {
    let bytes = no_ap_pdf(
        "4 0 R 5 0 R 6 0 R 7 0 R",
        "<< /Type /Annot /Subtype /Line /C [0 0 0] /L [50 150 250 150] /LL 10 \
         /Rect [40 140 260 160] >>|\
         << /Type /Annot /Subtype /Ink /C [1 0 0] /F 128 /InkList [[50 50 250 250]] \
         /Rect [40 40 260 260] >>|\
         << /Type /Annot /Subtype /Square /Rect [40 40 260 260] >>|\
         << /Type /Annot /Subtype /Link /Rect [40 40 60 60] >>",
        4,
    );
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let out = s.bake_missing_appearances(FreeTextC::Fill).unwrap();
    assert!(out.baked.is_empty());
    let reasons: Vec<_> = out
        .skipped
        .iter()
        .map(|k| (k.subtype.as_str(), k.reason.as_str()))
        .collect();
    assert_eq!(reasons.len(), 3, "{reasons:?}");
    assert!(reasons[0].1.contains("pdf dimension"));
    assert!(reasons[1].1.contains("locked"));
    assert!(reasons[2].1.contains("no colour"));
    assert_eq!(s.undo_depth(), 0, "nothing baked, nothing committed");
}

#[test]
fn a_baked_annotation_is_not_baked_again() {
    let (mut s, _) = bake(
        no_ap_pdf(
            "4 0 R",
            "<< /Type /Annot /Subtype /Circle /C [0 0 1] /Rect [40 40 200 200] >>",
            1,
        ),
        FreeTextC::Fill,
    );
    let again = s.bake_missing_appearances(FreeTextC::Fill).unwrap();
    assert!(again.baked.is_empty() && again.skipped.is_empty());
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn a_substituted_free_text_font_is_reported() {
    let mut s = EditSession::new(
        Document::from_bytes(no_ap_pdf(
            "4 0 R",
            "<< /Type /Annot /Subtype /FreeText /Contents (Hi) /DA (/Helv 12 Tf 0 g) \
             /Rect [40 40 260 120] >>",
            1,
        ))
        .unwrap(),
    );
    let out = s.bake_missing_appearances(FreeTextC::Fill).unwrap();
    assert!(out.baked[0].notes.iter().any(|n| n.contains("Helvetica")));
}
