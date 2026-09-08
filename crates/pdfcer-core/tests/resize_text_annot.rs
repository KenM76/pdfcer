//! # "pdfcer did not draw it" — about an appearance pdfcer drew seconds earlier
//!
//! ## The operator
//!
//! > *"Also when will being able to drag on the canvas be able to resize the
//! > Text Box and Stamp."*
//!
//! ## ★★★ The measurement, with the control that makes it a report
//!
//! One fresh session per attempt; every annotation authored by pdfcer in that
//! same session, seconds before the resize:
//!
//! | authored by | subtype | resize |
//! |---|---|---|
//! | `add_markup` | `/Square` | ✅ accepted, appearance rebuilt |
//! | `add_text_annotation` | `/FreeText` | ❌ refused |
//! | `add_text_annotation` | `/Text` | ❌ refused |
//!
//! …with the refusal *"pdfcer did not draw it, so pdfcer will not redraw it"*.
//!
//! ★ The `/Square` row is why this is a defect and not a limitation. Without
//! it the finding reads *"text boxes cannot be resized"* — a confident
//! sentence about the wrong subject. With it, two pdfcer-authored annotations
//! differ, and the only difference is **which verb authored them**.
//!
//! ## The cause
//!
//! `resize_annotation` decides authorship honestly — it rebuilds from the
//! unmodified spec and compares bytes, because only an appearance pdfcer
//! would have drawn is pdfcer's to redraw. But it rebuilds through
//! `annot_author::spec_from_dict` → `build_appearance_opts`, which is the
//! **markup** family: `/Square`, `/Circle`, `/Line`, `/Polygon`, `/Ink`, text
//! markup.
//!
//! A `/FreeText` or `/Text` is authored by a different verb through a
//! different builder. The comparison therefore cannot reproduce it, concludes
//! *"foreign"*, and refuses — **about pdfcer's own work**.
//!
//! ★★ The same shape this project keeps finding: a check written for one
//! member of a family and not the other, where the guarded member looks
//! correct and the unguarded one fails in a way that blames the document.
//! Here it is sharper than usual, because the refusal makes a **false factual
//! claim** about who drew the appearance rather than merely declining.
//!
//! ## Not a request to loosen the refusal
//!
//! The refusal is right when it is true. `/Square` proves the accept path
//! works; what is wrong is the authorship *test*, not the policy it feeds.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::{Color, MarkupSpec, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, ResizeOptions};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture()).expect("fixture parses"))
}

fn rect() -> Rect {
    Rect {
        llx: 40.0,
        lly: 40.0,
        urx: 240.0,
        ury: 160.0,
    }
}

/// A `/FreeText` authored by pdfcer, in this session, just now.
fn author_free_text(s: &mut EditSession) -> ObjId {
    s.add_text_annotation(
        0,
        &TextAnnotSpec::FreeText {
            rect: rect(),
            text: "RESIZE ME".to_owned(),
            font: Std14::Helvetica,
            font_size: 12.0,
            color: pdfcer_core::vartext::TextColor::Gray(0.0),
            quadding: pdfcer_core::vartext::Quadding::Left,
            multiline: false,
            border: None,
            border_width: 0.0,
        },
    )
    .expect("author a text box")
}

/// The control: a `/Square` authored by the OTHER verb, same session.
fn author_square(s: &mut EditSession) -> ObjId {
    s.add_markup(
        0,
        &MarkupSpec::Square {
            rect: rect(),
            border: Some(Color::Gray(0.0)),
            interior: None,
            border_width: 1.0,
            border_effect: None,
        },
    )
    .expect("author a square")
}

/// ★ THE CONTROL. If this ever fails the rest of the file is testing the
/// wrong thing — a blanket resize breakage would make every assertion below
/// pass for the wrong reason.
#[test]
fn a_pdfcer_authored_square_resizes() {
    let mut s = session();
    let id = author_square(&mut s);
    s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect("pdfcer drew it and pdfcer can redraw it");
}

/// ★★★ THE DEFECT: pdfcer drew this one too.
#[test]
fn a_pdfcer_authored_text_box_resizes() {
    let mut s = session();
    let id = author_free_text(&mut s);
    let r = s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new());
    assert!(
        r.is_ok(),
        "`add_text_annotation` baked this /AP in this session, seconds ago. \
         Refusing it as foreign is not a limitation, it is a false claim about \
         who drew it: {:?}",
        r.err()
    );
}

/// And non-uniformly, which is the gesture the operator described.
///
/// Dragging a corner is rarely uniform. The uniform case passing alone would
/// leave the reported gesture broken.
#[test]
fn a_pdfcer_authored_text_box_resizes_non_uniformly() {
    let mut s = session();
    let id = author_free_text(&mut s);
    let r = s.resize_annotation(id, (40.0, 40.0), 1.5, 0.8, &ResizeOptions::new());
    assert!(r.is_ok(), "{:?}", r.err());
}

/// A genuinely FOREIGN appearance is still refused.
///
/// The guard against over-correcting, and the reason this Pass fixes the
/// authorship *test* rather than relaxing the policy. `rect-differences-square.pdf`
/// carries a generator-written `/AP` that pdfcer did not draw; resizing it
/// without `scale_stroke_width` must still be refused, or the fix has bought
/// text boxes by giving up the guarantee that pdfcer never silently redraws
/// somebody else's artwork.
#[test]
fn a_genuinely_foreign_appearance_is_still_refused() {
    let s = EditSession::new(
        Document::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/synthetic/annot/rect-differences-square.pdf"),
        )
        .expect("fixture parses"),
    );
    let id = {
        pdfcer_core::annot::page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id)
            .first()
            .and_then(|a| a.id)
            .expect("the fixture's annotation")
    };

    let mut s = s;
    let r = s.resize_annotation(id, (100.0, 100.0), 1.5, 1.5, &ResizeOptions::new());
    assert!(
        r.is_err(),
        "pdfcer did NOT draw this one, and must still say so"
    );
}

/// The appearance stream of an annotation, from the SAVED bytes.
fn appearance(s: &EditSession, id: ObjId) -> String {
    use pdfcer_core::object::Object;
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let Object::Dict(annot) = &doc.get(id).expect("present").value else {
        panic!("not a dict")
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
        panic!("no /AP")
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").expect("/N")) else {
        panic!("not a stream")
    };
    String::from_utf8_lossy(st.data_span.slice(doc.bytes()).expect("bytes")).into_owned()
}

/// A MULTILINE text box stays wrapped across a resize.
///
/// ★ This test exists because a sabotage survived without it. `/FreeText` has
/// no multiline key -- 12.5.6.6 gives it none, and `/Ff` is a form-field entry
/// -- so a rebuild that ignored the measured layout produced a perfectly valid
/// SINGLE-line box, and every other test here stayed green because their
/// fixtures were single-line to begin with.
///
/// Un-wrapping a text box on resize is precisely the defect
/// `measure_free_text_multiline` was written for; this is what makes the
/// resize path actually call it rather than merely compile beside it.
#[test]
fn a_multiline_text_box_stays_wrapped_across_a_resize() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::FreeText {
                rect: rect(),
                text: "FIRST LINE
SECOND LINE"
                    .to_owned(),
                font: Std14::Helvetica,
                font_size: 12.0,
                color: pdfcer_core::vartext::TextColor::Gray(0.0),
                quadding: pdfcer_core::vartext::Quadding::Left,
                multiline: true,
                border: None,
                border_width: 0.0,
            },
        )
        .expect("author a wrapped text box");

    let before = appearance(&s, id).matches(") Tj").count();
    assert_eq!(
        before, 2,
        "the fixture must start wrapped, or this proves nothing"
    );

    s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect("pdfcer drew it");

    let after = appearance(&s, id).matches(") Tj").count();
    assert_eq!(
        after, 2,
        "the box must still be TWO lines after the resize -- a rebuild that ignored the measured layout would silently un-wrap it into one"
    );
}
