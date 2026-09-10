//! `Pass 287.0` — a stamp's label size is a property, and the box follows the
//! text instead of trapping it.
//!
//! # The operator report, verbatim
//!
//! > *"I have to draw the size of the stamp before it gets applied and if I
//! > don't make it long enough to hold all the text it just cuts off and I
//! > have no way to fix it after because if I stretch the box out the text
//! > stretches with it."*
//!
//! Two defects that compound into a trap:
//!
//! 1. **The label was clipped.** It is laid into a band as wide as the box and
//!    the `/BBox` clips overflow (`vartext.rs`, §12.7.3.3) — right for a form
//!    field, whose box is a *field boundary*; wrong for a stamp, whose box is
//!    a *drawing gesture*.
//! 2. **The repair scaled the text.** The size was
//!    `(rect_height * 0.42).clamp(8.0, 28.0)` — derived from the box, stored
//!    nowhere — so widening the box to reveal the clipped text enlarged the
//!    text by the same act.
//!
//! Either alone is an annoyance. Together, the first mistake is unfixable.
//!
//! # ★★ There was nothing to copy
//!
//! §12.5.6.12's `/Stamp` table defines exactly one subtype key, `/Name` — no
//! `/DA`, no font entry. Acrobat has no answer either, and (having no
//! regeneration-on-resize hook) very likely stretches its own stamp text on
//! resize exactly as pdfcer did. So the storage location is pdfcer's choice,
//! and `/DA` is chosen because it is **the string the standard already defines
//! for this question** (§12.7.3.3) on `/FreeText`, the annotation with the
//! identical problem.

use pdfcer_core::annot_author::{
    StampFit, StampName, StampStyle, TextAnnotSpec, build_text_annotation,
};
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;

fn spec(rect: Rect, label: &str, style: StampStyle) -> TextAnnotSpec {
    TextAnnotSpec::Stamp {
        rect,
        name: StampName::Draft,
        label: Some(label.to_owned()),
        color: pdfcer_core::annot_author::Color::Gray(0.0),
        style,
    }
}

/// A box far too narrow for the label — the operator's first mistake.
fn too_narrow() -> Rect {
    Rect {
        llx: 100.0,
        lly: 100.0,
        urx: 140.0,
        ury: 130.0,
    }
}

/// The `Tf` size baked into an appearance stream.
fn baked_size(content: &[u8]) -> f64 {
    let text = String::from_utf8_lossy(content);
    let tokens: Vec<&str> = text.split_ascii_whitespace().collect();
    let at = tokens
        .iter()
        .position(|t| *t == "Tf")
        .expect("the appearance sets a font");
    tokens[at - 1].parse().expect("the Tf size parses")
}

// ------------------------------------------- 1. the box follows the text

/// ★★★ THE TRAP IS GONE: a box too small for the label is WIDENED, not
/// clipped.
///
/// The drawn rectangle becomes a position and a minimum size rather than a
/// cage, which is the direct answer to *"I have to draw the size of the stamp
/// before it gets applied"*.
#[test]
fn a_box_too_narrow_for_the_label_grows_to_fit_it() {
    let drawn = too_narrow();
    let a = build_text_annotation(&spec(
        drawn,
        "APPROVED FOR CONSTRUCTION",
        StampStyle::default(),
    ))
    .expect("the stamp builds");

    assert!(
        a.rect.width() > drawn.width(),
        "the box must grow to hold the label: drawn {} -> {}",
        drawn.width(),
        a.rect.width()
    );
    assert_eq!(
        a.rect.llx, drawn.llx,
        "it grows to the RIGHT — the drawn corner is where the operator put it"
    );
    assert_eq!(a.rect.lly, drawn.lly, "and the baseline does not move");
}

/// ★ The box is only ever GROWN, never shrunk.
///
/// Without this, "fit the box to the text" would silently shrink a stamp the
/// operator deliberately drew large — trading his trap for a different one.
#[test]
fn a_box_larger_than_its_label_is_left_alone() {
    let drawn = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 400.0,
        ury: 60.0,
    };
    let a = build_text_annotation(&spec(drawn, "OK", StampStyle::default())).expect("builds");
    assert_eq!(
        a.rect.width(),
        drawn.width(),
        "a deliberately large stamp keeps its size"
    );
}

// --------------------------------------- 2. the size is a property, stored

/// ★★ THE SIZE SURVIVES INTO THE FILE, in `/DA`.
///
/// Without storage there is no re-bake that can keep it, and the repair path
/// cannot exist at all.
#[test]
fn the_label_size_is_written_to_da() {
    let a =
        build_text_annotation(&spec(too_narrow(), "DRAFT", StampStyle::default())).expect("builds");

    let Some(Object::String(da)) = a.annot.get(b"DA") else {
        panic!("a stamp must carry /DA, got {:?}", a.annot.get(b"DA"));
    };
    let da = String::from_utf8_lossy(da);
    assert!(da.contains("Tf"), "/DA must set a font size, got {da:?}");
    assert!(
        da.contains("12"),
        "/DA must carry the default 12 pt, got {da:?}"
    );
}

/// ★★★ THE SIZE NO LONGER FOLLOWS THE BOX — the defect, stated as a
/// comparison.
///
/// Two boxes of very different heights, same label, same style: the baked
/// `Tf` must be identical. Under the old `(h * 0.42)` formula these differ by
/// more than a factor of two, so this test is red on every build before this
/// Pass.
#[test]
fn two_boxes_of_different_heights_bake_the_same_text_size() {
    let short = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 300.0,
        ury: 24.0,
    };
    let tall = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 300.0,
        ury: 60.0,
    };

    let a = build_text_annotation(&spec(short, "DRAFT", StampStyle::default())).expect("a");
    let b = build_text_annotation(&spec(tall, "DRAFT", StampStyle::default())).expect("b");

    assert_eq!(
        baked_size(&a.ap_content),
        baked_size(&b.ap_content),
        "the label size must not depend on the box height"
    );
}

/// ★ THE CONTROL: the old derived behaviour is still reachable BY NAME.
///
/// `font_size: None` asks for `(h * 0.42).clamp(8.0, 28.0)` deliberately, so
/// an older document's appearance stays reproducible. Without this test the
/// legacy path could rot unnoticed, and "kept reachable" would be a claim
/// rather than a fact.
#[test]
fn the_derived_size_is_still_available_by_asking_for_it() {
    let short = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 300.0,
        ury: 24.0,
    };
    let tall = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 300.0,
        ury: 60.0,
    };
    let legacy = StampStyle::legacy_derived();

    let a = build_text_annotation(&spec(short, "DRAFT", legacy)).expect("a");
    let b = build_text_annotation(&spec(tall, "DRAFT", legacy)).expect("b");

    assert_ne!(
        baked_size(&a.ap_content),
        baked_size(&b.ap_content),
        "under the legacy policy the size DOES follow the box — that is what \
         makes it the legacy policy"
    );
}

// ------------------------------------------------- 3. the other fit policies

/// `ShrinkToBox` keeps the drawn box and makes the text smaller.
#[test]
fn shrink_to_box_keeps_the_box_and_shrinks_the_text() {
    let drawn = too_narrow();
    let a = build_text_annotation(&spec(
        drawn,
        "APPROVED FOR CONSTRUCTION",
        StampStyle::points(12.0).with_fit(StampFit::ShrinkToBox),
    ))
    .expect("builds");

    assert_eq!(a.rect.width(), drawn.width(), "the box is kept");
    assert!(
        baked_size(&a.ap_content) < 12.0,
        "and the text shrank to fit it, got {}",
        baked_size(&a.ap_content)
    );
}

/// `ClipToBox` reproduces the reported behaviour exactly — kept so it can be
/// asked for rather than suffered.
#[test]
fn clip_to_box_keeps_both_and_is_the_reported_behaviour() {
    let drawn = too_narrow();
    let a = build_text_annotation(&spec(
        drawn,
        "APPROVED FOR CONSTRUCTION",
        StampStyle::points(12.0).with_fit(StampFit::ClipToBox),
    ))
    .expect("builds");

    assert_eq!(a.rect.width(), drawn.width(), "the box is kept");
    assert_eq!(
        baked_size(&a.ap_content),
        12.0,
        "and so is the size — the label is simply clipped, as reported"
    );
}

// ------------------------- 4. the repair path, end to end (the operator's own)

/// ★★★ THE WHOLE REPORT, AS ONE TEST: place a stamp, then stretch the box —
/// and the text does NOT stretch with it.
///
/// > *"if I stretch the box out the text stretches with it."*
///
/// The stamp is authored, saved into a document, then resized through the real
/// verb. Its baked `Tf` must be the size it was authored with, not a size
/// derived from the new box.
///
/// ★ Two things had to be true for this to work, and either one missing makes
/// it fail:
///
/// 1. the size is **stored** (`/DA`) and **recovered** on the way back in;
/// 2. `resize_annotation` **recognises a pdfcer-drawn `/Stamp` as its own**.
///    Before this Pass the authorship test knew `/FreeText` and markup and not
///    stamps, so a stamp pdfcer had drawn was refused as foreign — `R245`'s
///    shape on a family of three routes.
#[test]
fn stretching_a_stamp_keeps_its_text_size() {
    use pdfcer_core::document::Document;
    use pdfcer_core::edit::{EditSession, ResizeOptions};

    let doc = Document::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/hello.pdf"),
    )
    .expect("load hello.pdf");
    let mut session = EditSession::new(doc);

    // Placed in a box deliberately too narrow — the operator's first mistake.
    let drawn = Rect {
        llx: 50.0,
        lly: 50.0,
        urx: 90.0,
        ury: 80.0,
    };
    let id = session
        .add_text_annotation(
            0,
            &spec(drawn, "APPROVED FOR CONSTRUCTION", StampStyle::default()),
        )
        .expect("the stamp is placed");

    let authored = stamp_tf_size(&session);

    // Now stretch it, the repair he could not make before.
    //
    // ★★ BOTH AXES, and the vertical one is the half that matters. The old
    // size formula was `(rect_height * 0.42)`, so a purely HORIZONTAL stretch
    // left it unchanged and this test passed with or without the fix. A
    // sabotage that reverted the size to the derived formula stayed green
    // here until the height moved too — the fixture, not the code, was what
    // made the assertion meaningless.
    session
        .resize_annotation(
            id,
            (drawn.llx, drawn.lly),
            2.0,
            1.8,
            &ResizeOptions::default(),
        )
        .expect("a pdfcer-drawn stamp resizes -- it used to be refused as foreign");

    let after = stamp_tf_size(&session);

    assert!(
        (authored - after).abs() < 0.001,
        "stretching the box must not change the text size: {authored} -> {after}"
    );
}

/// The `Tf` size baked into the document's one `/Stamp` appearance, read back
/// through a SAVE AND REOPEN.
///
/// ★ Deliberately not read out of the live session. Going through the file is
/// what proves the size survives serialization — a size that were correct in
/// memory and lost on save would satisfy an in-memory assertion and fail the
/// operator.
fn stamp_tf_size(session: &pdfcer_core::edit::EditSession) -> f64 {
    use pdfcer_core::writer::SaveOptions;

    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("the session saves");
    let doc = pdfcer_core::document::Document::from_bytes(bytes).expect("and reopens");

    for io in doc.objects() {
        let Some(annot) = io.value.as_dict() else {
            continue;
        };
        if !matches!(annot.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Stamp") {
            continue;
        }
        let Some(ap) = annot
            .get(b"AP")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        let Some(Object::Stream(stream)) = ap.get(b"N").map(|o| doc.resolve(o)) else {
            continue;
        };
        let Some(raw) = stream.data_span.slice(doc.bytes()) else {
            continue;
        };
        let decoded =
            pdfcer_core::filters::decode_stream(&stream.dict, raw).unwrap_or_else(|_| raw.to_vec());
        return baked_size(&decoded);
    }
    panic!("no /Stamp annotation with a baked appearance in the saved document");
}

// ------------- 5. `Pass 291.0` — the fit is REPORTED, not merely performed

/// ★★★ A shrunk label says so. This is the disclosure that made two of the
/// three policies offerable at all.
///
/// `applied_autosize` is `None` here and always will be — the stamp path
/// hands the layout an EXPLICIT size (the one the fit computed), and
/// `applied_autosize` reports only the variable-text auto-size. So the
/// number was computed, used, written to `/DA`, and dropped on the way back
/// to the caller. A shell that wanted to offer *shrink to fit* had to either
/// stay silent about the size it produced, or re-implement pdfcer's fit
/// formula in another crate and watch the two drift.
#[test]
fn a_shrunk_label_reports_the_size_it_was_shrunk_to() {
    let a = build_text_annotation(&spec(
        too_narrow(),
        "APPROVED FOR CONSTRUCTION",
        StampStyle::points(12.0).with_fit(StampFit::ShrinkToBox),
    ))
    .expect("builds");

    let fit = a.stamp_label_fit.expect("a stamp reports its fit");
    assert!(
        fit.is_inference(),
        "a size nobody asked for is an inference"
    );
    assert_eq!(fit.token(), "label_shrunk");
    match fit {
        pdfcer_core::annot_author::StampLabelFit::LabelShrunk { size, requested } => {
            assert_eq!(requested, 12.0, "what was asked for");
            assert!(size < 12.0, "what was drawn, got {size}");
            // The reported size is the size on the page, not an
            // approximation of it — the two must be the same number or the
            // disclosure is a second implementation of the fit.
            assert_eq!(size, baked_size(&a.ap_content));
        }
        other => panic!("expected LabelShrunk, got {other:?}"),
    }
    assert!(
        a.applied_autosize.is_none(),
        "the variable-text auto-size is NOT the stamp's answer and must stay None"
    );
}

/// ★★ And a label that FITS says nothing. This is the half that makes the
/// disclosure usable: `Some(_)` on its own means "a stamp", not "an
/// inference", and a caller that reported every `Some` would tell the
/// operator their own instruction back at them.
#[test]
fn a_label_that_fits_reports_no_inference() {
    let roomy = Rect {
        llx: 100.0,
        lly: 100.0,
        urx: 500.0,
        ury: 130.0,
    };
    let a = build_text_annotation(&spec(roomy, "OK", StampStyle::points(12.0))).expect("builds");

    let fit = a.stamp_label_fit.expect("a stamp reports its fit");
    assert!(
        !fit.is_inference(),
        "nothing was decided for anyone, so nothing is owed"
    );
    assert_eq!(fit.token(), "as_requested");
    assert_eq!(fit.size(), 12.0);
    assert_eq!(a.rect.width(), roomy.width(), "and the box is untouched");
}

/// A clipped label reports HOW MUCH is missing — characters and points.
///
/// The label is drawn centred, so the overflow is split between both ends;
/// the count is of characters not *entirely* inside the box, because half a
/// glyph is not a character the operator can read.
#[test]
fn a_clipped_label_reports_what_is_not_on_the_page() {
    let a = build_text_annotation(&spec(
        too_narrow(),
        "APPROVED FOR CONSTRUCTION",
        StampStyle::points(12.0).with_fit(StampFit::ClipToBox),
    ))
    .expect("builds");

    let fit = a.stamp_label_fit.expect("a stamp reports its fit");
    assert!(fit.is_inference());
    assert_eq!(fit.token(), "label_clipped");
    match fit {
        pdfcer_core::annot_author::StampLabelFit::LabelClipped {
            size,
            hidden_chars,
            overflow,
        } => {
            assert_eq!(size, 12.0, "clipping changes no size");
            assert!(overflow > 0.0, "the label is wider than the box");
            let total = "APPROVED FOR CONSTRUCTION".chars().count();
            assert!(
                hidden_chars > 0 && hidden_chars < total,
                "some but not all characters are hidden, got {hidden_chars} of {total}"
            );
        }
        other => panic!("expected LabelClipped, got {other:?}"),
    }
}

/// Growing is reported too, even though the operator can see it happen. A
/// caller should not have to diff two rectangles to write one sentence.
#[test]
fn a_grown_box_reports_the_width_it_grew_to() {
    let drawn = too_narrow();
    let a = build_text_annotation(&spec(
        drawn,
        "APPROVED FOR CONSTRUCTION",
        StampStyle::points(12.0).with_fit(StampFit::GrowToText),
    ))
    .expect("builds");

    let fit = a.stamp_label_fit.expect("a stamp reports its fit");
    assert_eq!(fit.token(), "box_grown");
    match fit {
        pdfcer_core::annot_author::StampLabelFit::BoxGrown { width, size } => {
            assert_eq!(size, 12.0, "growing the box does not resize the label");
            assert!(width > drawn.width());
            // The reported width IS the written rectangle's width. Compared
            // with a tolerance rather than exactly: the rect carries the
            // grown edge as `llx + needed` and `width()` subtracts `llx`
            // back off, so the two differ in the last ULP for a reason that
            // has nothing to do with either being wrong.
            assert!(
                (width - a.rect.width()).abs() < 1e-9,
                "reported {width} vs written {}",
                a.rect.width()
            );
        }
        other => panic!("expected BoxGrown, got {other:?}"),
    }
}

/// A `/FreeText` reports `None` — `StampFit` is a stamp policy and a field
/// that answered on an annotation it never touched would be a fact nobody
/// measured.
#[test]
fn a_freetext_has_no_stamp_fit_to_report() {
    let a = build_text_annotation(&TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 100.0,
            lly: 100.0,
            urx: 300.0,
            ury: 160.0,
        },
        text: "hello".to_owned(),
        font: pdfcer_core::fontdata::Std14::Helvetica,
        font_size: 12.0,
        color: pdfcer_core::vartext::TextColor::Gray(0.0),
        quadding: pdfcer_core::vartext::Quadding::Left,
        multiline: false,
        border: None,
        border_width: 1.0,
    })
    .expect("builds");
    assert!(a.stamp_label_fit.is_none());
}
