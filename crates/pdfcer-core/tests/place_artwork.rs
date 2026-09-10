//! `Pass 293.0` — placing a custom stamp's artwork on a page.
//!
//! # The gap
//!
//! `Pass 288.0` made stamp COLLECTIONS readable and authorable. A custom
//! stamp's whole point is its **artwork**, which is a page, and nothing could
//! draw one page onto another — so pdfcer could read the operator's own
//! signature stamps and could not stamp anything with them.
//!
//! # The shape, and why it is this shape
//!
//! The artwork becomes a **form XObject** and the placement is a `/Stamp`
//! annotation whose `/AP` `/N` points at it. That is what Acrobat writes, it
//! keeps the artwork VECTOR (the operator's documents are CAD drawings, where
//! a raster stamp is the one thing that does not survive zooming), and — the
//! part that matters on a drawing pdfcer must not silently alter — **the
//! page's own content stream is never touched**.
//!
//! The rejected alternative is recorded because it was tempting: render the
//! stamp page and place a bitmap through `add_image`. It would not be
//! compatible with Acrobat, it would inflate a 5.6 MB drawing per stamp, and
//! it would pick a resolution nobody asked for.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::{Name, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

/// A one-page document `width` x `height`, with `extra` spliced into the page
/// dictionary and `body` as its content stream.
fn doc_with_page(width: f64, height: f64, body: &str, extra: &str) -> Vec<u8> {
    let bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
                 /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R {extra} >>"
            ),
        ),
        (
            4,
            format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
        ),
        (
            5,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for (_, off) in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

const ARTWORK: &str = "1 0 0 RG 4 w 10 10 m 130 60 l S";

/// The stamp collection: one page of artwork, 144 x 72.
fn stamp_source() -> Document {
    Document::from_bytes(doc_with_page(144.0, 72.0, ARTWORK, "")).expect("the source loads")
}

/// The drawing being stamped: 612 x 792, its own `/F1` resource name.
fn target() -> EditSession {
    let doc = Document::from_bytes(doc_with_page(612.0, 792.0, "BT /F1 12 Tf ET", ""))
        .expect("the target loads");
    EditSession::new(doc)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        llx: x,
        lly: y,
        urx: x + w,
        ury: y + h,
    }
}

// ------------------------------------------------- 1. the placement itself

/// ★★★ The artwork lands as a form XObject behind a `/Stamp`, and the PAGE'S
/// OWN CONTENT STREAM IS UNTOUCHED (R47).
///
/// The second half is the one an operator's drawing depends on: a stamp must
/// not rewrite the sheet it is stamped onto.
#[test]
fn the_artwork_becomes_a_form_xobject_behind_a_stamp() {
    let source = stamp_source();
    let mut session = target();
    let content_before = page_content(&session);
    // ★ The fixture-can-fail check (`R225`): a comparison of two EMPTY byte
    // vectors passes whatever the verb does to the page. Assert the page has
    // content before asserting that the content did not change.
    assert!(
        !content_before.is_empty(),
        "the target page must actually have content for the R47 check to mean anything"
    );

    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 144.0, 72.0))
        .expect("the placement succeeds");

    // The annotation.
    let annot = match session.value(placed.annot_id) {
        Some(Object::Dict(d)) => d.clone(),
        other => panic!("expected an annotation dictionary, got {other:?}"),
    };
    assert_eq!(
        annot
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Stamp".to_vec())
    );

    // Its appearance is the form, and the form carries the source's bytes.
    let form = match session.value(placed.form_id) {
        Some(Object::Stream(s)) => s.clone(),
        other => panic!("expected a form XObject stream, got {other:?}"),
    };
    assert_eq!(
        form.dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Form".to_vec()),
        "§8.10.2: the artwork is a form XObject"
    );
    assert!(
        form.dict.contains_key(b"BBox"),
        "a form XObject without a /BBox cannot be mapped to a /Rect (§12.5.5)"
    );

    // ★ And nothing was written to the page's content stream.
    assert_eq!(
        page_content(&session),
        content_before,
        "R47: placing a stamp must not rewrite the page it lands on"
    );
}

/// The whole placement is ONE undo entry (R49) — the form, the annotation,
/// the imported resources and the `/Annots` patch all go together.
#[test]
fn the_placement_is_one_undo_entry() {
    let source = stamp_source();
    let mut session = target();
    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 144.0, 72.0))
        .expect("the placement succeeds");
    assert!(session.value(placed.annot_id).is_some());

    session.undo().expect("one undo");
    assert!(
        session.value(placed.annot_id).is_none(),
        "the annotation is gone"
    );
    assert!(
        session.value(placed.form_id).is_none(),
        "and so is its artwork -- not a second undo entry"
    );
}

// ------------------------------------------------------- 2. the disclosures

/// ★★ A rectangle of the wrong proportions SQUASHES the artwork, and the
/// operator is told.
///
/// Nothing on the page says a signature is 30 % wider than it was drawn.
#[test]
fn a_rectangle_of_the_wrong_shape_is_reported_as_distortion() {
    let source = stamp_source();
    let mut session = target();

    // 144 x 72 artwork into a 144 x 144 box: twice as tall as it should be.
    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 144.0, 144.0))
        .expect("the placement succeeds");
    assert!(placed.distorted);
    assert!((placed.scale_x - 1.0).abs() < 1e-9);
    assert!((placed.scale_y - 2.0).abs() < 1e-9);
}

/// ...and a rectangle with the artwork's own proportions reports NO
/// distortion, at any size. This is the half that keeps the disclosure
/// meaningful: a flag that were always true would be noise.
#[test]
fn a_proportional_rectangle_is_not_distortion() {
    let source = stamp_source();
    let mut session = target();
    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 288.0, 144.0))
        .expect("the placement succeeds");
    assert!(!placed.distorted, "2x in both directions is a resize");
    assert!((placed.scale_x - 2.0).abs() < 1e-9);
    assert!((placed.scale_y - 2.0).abs() < 1e-9);
}

/// Annotations on the SOURCE page are counted and left behind — they are not
/// page content, and a dynamic stamp's live text lives in exactly such
/// objects.
#[test]
fn annotations_on_the_stamp_page_are_counted_not_carried() {
    // The stamp page carries one `/Text` annotation of its own.
    let bytes = doc_with_page(144.0, 72.0, ARTWORK, "/Annots [6 0 R]");
    // Splice the annotation object in, keeping the xref honest by re-saving
    // through pdfcer rather than hand-patching offsets.
    let doc = Document::from_bytes(bytes).expect("loads");
    let mut prep = EditSession::new(doc);
    prep.add_text_annotation(
        0,
        &pdfcer_core::annot_author::TextAnnotSpec::Sticky {
            rect: rect(10.0, 10.0, 20.0, 20.0),
            icon: pdfcer_core::annot_author::StickyIcon::Note,
            contents: "a comment on the stamp page".to_owned(),
            color: pdfcer_core::annot_author::Color::Rgb(1.0, 1.0, 0.0),
            open: false,
        },
    )
    .expect("the note is authored");
    let (with_annot, _) = prep
        .to_full_bytes(&SaveOptions::default())
        .expect("the source saves");
    let source = Document::from_bytes(with_annot).expect("and re-opens");

    let mut session = target();
    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 144.0, 72.0))
        .expect("the placement succeeds");

    assert!(
        placed.source_annotations_ignored >= 1,
        "the stamp page's own annotation is counted, got {}",
        placed.source_annotations_ignored
    );
}

/// Resource names cannot collide, and the count says so permanently: a form
/// XObject carries its OWN `/Resources` (§8.10.2 Table 96), so the artwork's
/// `/F1` and the page's `/F1` never meet.
#[test]
fn the_forms_resources_are_its_own_so_nothing_is_renamed() {
    let source = stamp_source();
    let mut session = target();
    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 144.0, 72.0))
        .expect("the placement succeeds");

    assert_eq!(placed.resources_renamed, 0);
    let form = match session.value(placed.form_id) {
        Some(Object::Stream(s)) => s.clone(),
        other => panic!("expected a stream, got {other:?}"),
    };
    let resources = form
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("the form carries its own resources");
    assert!(
        resources.contains_key(b"Font"),
        "the source page's font came with the artwork"
    );
    assert!(
        placed.objects_imported >= 1,
        "the font object itself was copied, got {}",
        placed.objects_imported
    );
}

// ------------------------------------------------------------ 3. refusals

/// ★ A source page out of range gets its OWN error, not the target's.
///
/// "You asked for page 9 of a 3-page stamp file" and "you asked for page 9 of
/// a 3-page drawing" are different mistakes, and a shell shows them in
/// different places.
#[test]
fn a_source_page_out_of_range_is_named_as_the_sources() {
    let source = stamp_source();
    let mut session = target();
    let err = session
        .place_page_artwork(&source.view(), 9, 0, rect(0.0, 0.0, 10.0, 10.0))
        .expect_err("the source has one page");
    match err {
        EditError::SourcePageOutOfRange { index, count } => {
            assert_eq!(index, 9);
            assert_eq!(count, 1);
        }
        other => panic!("expected SourcePageOutOfRange, got {other:?}"),
    }
}

/// The TARGET page's own range error is unchanged and still distinct.
#[test]
fn a_target_page_out_of_range_stays_the_targets_error() {
    let source = stamp_source();
    let mut session = target();
    let err = session
        .place_page_artwork(&source.view(), 0, 9, rect(0.0, 0.0, 10.0, 10.0))
        .expect_err("the target has one page");
    assert!(
        matches!(err, EditError::PageOutOfRange { index: 9, count: 1 }),
        "got {err:?}"
    );
}

/// The page's content stream bytes, for the R47 comparison.
fn page_content(session: &EditSession) -> Vec<u8> {
    let pages = session.pages().expect("the page tree walks");
    let page = pages.first().expect("one page");
    let view = session.view();
    let mut out = Vec::new();
    for id in &page.contents {
        if let Some(Object::Stream(stream)) = view.graph().value(*id) {
            out.extend_from_slice(view.slice(stream.data_span).unwrap_or_default());
        }
    }
    out
}

/// A source page with a transparency `/Group` brings it along — §8.10.2
/// Table 96 puts `/Group` on a form XObject, and a page whose transparency is
/// defined against its own group renders differently without one.
#[test]
fn a_transparency_group_travels_with_the_artwork() {
    let bytes = doc_with_page(
        144.0,
        72.0,
        ARTWORK,
        "/Group << /S /Transparency /CS /DeviceRGB >>",
    );
    let source = Document::from_bytes(bytes).expect("loads");
    let mut session = target();
    let placed = session
        .place_page_artwork(&source.view(), 0, 0, rect(100.0, 100.0, 144.0, 72.0))
        .expect("the placement succeeds");

    assert!(placed.transparency_group_carried);
    let form = match session.value(placed.form_id) {
        Some(Object::Stream(s)) => s.clone(),
        other => panic!("expected a stream, got {other:?}"),
    };
    let group = form
        .dict
        .get(b"Group")
        .and_then(Object::as_dict)
        .expect("the group came with it");
    assert_eq!(
        group
            .get(b"S")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Transparency".to_vec())
    );
    // And a page with no group reports `false` rather than claiming a drop.
    let plain = stamp_source();
    let placed = session
        .place_page_artwork(&plain.view(), 0, 0, rect(0.0, 0.0, 144.0, 72.0))
        .expect("the placement succeeds");
    assert!(!placed.transparency_group_carried);
    let _ = Name::from(b"unused");
}
