//! The annotation half of the object clipboard, across the wire
//! (`Pass 169.0`).
//!
//! ## The claim being tested, and why it was doubted
//!
//! `ObjectClip::to_bytes` shipped in `Pass 120.1` **dropping every
//! annotation**, and the field's own doc comment gave the reason as a stated
//! limit rather than an oversight:
//!
//! > `MarkupSpec` and `DimensionKind` are rich enums whose byte encoding
//! > would be a second format to version alongside the content one, and
//! > getting it wrong means a clip that parses and pastes the wrong shape.
//!
//! That objection is correct **about a byte encoding**, and it is answered by
//! not writing one. Both models already have a **COS** representation that
//! ships, is exercised on every real document, and is fuzzed:
//!
//! - a `MarkupSpec` is what `build_appearance(spec).annot` writes and what
//!   `spec_from_dict` reads back — the *annotation dictionary* the spec
//!   describes, governed by §12.5.6;
//! - a `DimensionKind` is what `dimension::sidecar` writes into `/PieceInfo`
//!   and reads out of it.
//!
//! So the clip carries each annotation as **the COS object pdfcer already
//! knows how to write and read for it**, through
//! `writer::serialize::write_object` and `parser::Parser` — the same route
//! the clip's resource objects and `FieldClip` take. One COS grammar
//! implementation on each side; no second format.
//!
//! ## What that makes testable, and what these tests actually assert
//!
//! The design is only sound if the round trip is **lossless per variant**.
//! `spec_from_dict` is a reader for *foreign* annotations, so nothing
//! previously required it to be the exact inverse of `build_appearance` — and
//! one variant is a live trap: there is no `/Cloud` subtype in ISO 32000, so
//! `MarkupSpec::Cloud` is written as a `/Polygon` with `/BE << /S /C >>` and
//! is only recoverable by reading `/BE` back. A round trip that quietly
//! returned `Polygon` would flatten every revision cloud that crossed a
//! document boundary, and the document would look fine.
//!
//! So every variant is round-tripped explicitly, and the assertion is
//! **equality with the original spec**, not "it parsed".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{
    Color, LineEnding, MarkupSpec, Quad, TextMarkupKind, build_appearance, decode_spec,
    encode_spec, spec_from_dict,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// Every `MarkupSpec` variant, with values chosen so a dropped field shows.
///
/// Nothing here is at a default: colours differ per variant, widths are not
/// 1.0, and the cloud's intensity is not an integer. A round trip that
/// silently substituted a default would pass against a fixture built the
/// lazy way.
fn every_variant() -> Vec<(&'static str, MarkupSpec)> {
    vec![
        (
            "Square",
            MarkupSpec::Square {
                rect: Rect {
                    llx: 10.0,
                    lly: 20.0,
                    urx: 110.0,
                    ury: 90.0,
                },
                border: Some(Color::Rgb(0.2, 0.4, 0.6)),
                interior: Some(Color::Gray(0.85)),
                border_width: 2.5,
                border_effect: None,
            },
        ),
        (
            "Square + cloudy border",
            MarkupSpec::Square {
                rect: Rect {
                    llx: 10.0,
                    lly: 20.0,
                    urx: 110.0,
                    ury: 90.0,
                },
                border: Some(Color::Rgb(1.0, 0.0, 0.0)),
                interior: None,
                border_width: 3.0,
                border_effect: Some(1.5),
            },
        ),
        (
            "Circle",
            MarkupSpec::Circle {
                rect: Rect {
                    llx: 30.0,
                    lly: 30.0,
                    urx: 130.0,
                    ury: 100.0,
                },
                border: Some(Color::Cmyk(0.1, 0.2, 0.3, 0.4)),
                interior: Some(Color::Rgb(0.9, 0.9, 0.1)),
                border_width: 4.0,
            },
        ),
        (
            "Line",
            MarkupSpec::Line {
                start: (12.0, 34.0),
                end: (156.0, 78.0),
                color: Color::Rgb(0.0, 0.5, 0.25),
                width: 1.75,
                endings: (LineEnding::OpenArrow, LineEnding::ClosedArrow),
            },
        ),
        (
            "Ink",
            MarkupSpec::Ink {
                strokes: vec![
                    vec![(10.0, 10.0), (20.0, 30.0), (40.0, 15.0)],
                    vec![(60.0, 60.0), (80.0, 90.0)],
                ],
                color: Color::Gray(0.25),
                width: 2.25,
            },
        ),
        (
            "Polygon",
            MarkupSpec::Polygon {
                vertices: vec![(10.0, 10.0), (90.0, 20.0), (50.0, 80.0)],
                border: Some(Color::Rgb(0.3, 0.3, 0.9)),
                interior: Some(Color::Gray(0.5)),
                width: 2.0,
            },
        ),
        (
            "Cloud",
            MarkupSpec::Cloud {
                vertices: vec![(10.0, 10.0), (110.0, 20.0), (60.0, 90.0)],
                border: Some(Color::Rgb(1.0, 0.25, 0.0)),
                interior: None,
                width: 2.0,
                intensity: 1.25,
            },
        ),
        (
            "PolyLine",
            MarkupSpec::PolyLine {
                vertices: vec![(5.0, 5.0), (55.0, 45.0), (95.0, 15.0)],
                color: Color::Rgb(0.1, 0.7, 0.2),
                width: 3.5,
            },
        ),
        (
            "TextMarkup/Highlight",
            MarkupSpec::TextMarkup {
                kind: TextMarkupKind::Highlight,
                quads: vec![Quad {
                    ul: (10.0, 60.0),
                    ur: (90.0, 60.0),
                    ll: (10.0, 40.0),
                    lr: (90.0, 40.0),
                }],
                color: Color::Rgb(1.0, 1.0, 0.0),
            },
        ),
        (
            "TextMarkup/StrikeOut",
            MarkupSpec::TextMarkup {
                kind: TextMarkupKind::StrikeOut,
                quads: vec![
                    Quad {
                        ul: (10.0, 60.0),
                        ur: (90.0, 60.0),
                        ll: (10.0, 40.0),
                        lr: (90.0, 40.0),
                    },
                    Quad {
                        ul: (10.0, 30.0),
                        ur: (70.0, 30.0),
                        ll: (10.0, 10.0),
                        lr: (70.0, 10.0),
                    },
                ],
                color: Color::Rgb(0.8, 0.0, 0.0),
            },
        ),
    ]
}

/// ★ Every variant survives `spec → annotation dictionary → spec` exactly.
///
/// This is the property the whole serialisation design rests on: the clip
/// does not invent an encoding, it carries the COS object pdfcer already
/// writes for the spec. If `spec_from_dict` were not the exact inverse of
/// `build_appearance` for some variant, the clip would carry a shape that
/// pastes as something else — and `Cloud` is the variant where that would
/// have been invisible, because a flattened revision cloud is still a
/// perfectly good polygon.
#[test]
fn every_markup_variant_round_trips_through_the_clipboard_codec() {
    for (label, spec) in every_variant() {
        let back = decode_spec(&encode_spec(&spec))
            .unwrap_or_else(|e| panic!("{label}: could not read back: {e}"));
        assert_eq!(back, spec, "{label} did not survive the round trip");
    }
}

/// And through the COS grammar too — the codec's output is written and read
/// by the crate's own serialiser and parser, not held in memory.
#[test]
fn every_markup_variant_round_trips_through_cos_syntax() {
    use pdfcer_core::object::ObjId;
    use pdfcer_core::parser::Parser;
    use pdfcer_core::writer::encoder::IdentityEncoder;
    use pdfcer_core::writer::serialize::write_object;

    for (label, spec) in every_variant() {
        let mut bytes = Vec::new();
        write_object(
            &mut bytes,
            &encode_spec(&spec),
            ObjId::new(0, 0),
            &[],
            &IdentityEncoder,
        );
        let parsed = Parser::at(&bytes, 0)
            .parse_object()
            .unwrap_or_else(|e| panic!("{label}: not COS syntax: {e}"));
        let back =
            decode_spec(&parsed).unwrap_or_else(|e| panic!("{label}: could not read back: {e}"));
        assert_eq!(
            back, spec,
            "{label} did not survive a write/parse round trip",
        );
    }
}

/// ★ THE MEASUREMENT THAT REJECTED THE FREE ROUTE, kept as a test.
///
/// Carrying the *annotation dictionary* would have cost no new code: both
/// `build_appearance` and `spec_from_dict` already ship. This is why it was
/// not done, and it is pinned so nobody re-proposes it — including a future
/// session of mine reading the codec above and wondering why it exists.
///
/// `build_appearance` computes a `/Rect` that BOUNDS WHAT IS DRAWN (§12.5.2
/// requires it), and a cloudy square's scallops bulge outside the nominal
/// rectangle. So the authored `/Rect` is bigger than the spec's, and reading
/// it back yields the expanded one — which on a clipboard would grow the box
/// on **every** copy/paste cycle, compounding, with no error at any step.
///
/// `spec_from_dict` is not at fault. It reads FOREIGN annotations, where the
/// stored `/Rect` is the truth; shrinking it would be an invention. It simply
/// was never the inverse of the author, and nothing had required it to be.
#[test]
fn the_annotation_dictionary_route_is_not_lossless_which_is_why_the_codec_exists() {
    let s = session("hello.pdf");
    let cloudy = MarkupSpec::Square {
        rect: Rect {
            llx: 10.0,
            lly: 20.0,
            urx: 110.0,
            ury: 90.0,
        },
        border: Some(Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        border_width: 3.0,
        border_effect: Some(1.5),
    };
    let via_annot_dict =
        spec_from_dict(&s.graph(), &build_appearance(&cloudy).annot).expect("reads back");
    assert_ne!(
        via_annot_dict, cloudy,
        "if this ever becomes equal, the free route is viable and this codec \
         could be deleted -- but check EVERY variant before believing it",
    );
    let MarkupSpec::Square { rect, .. } = via_annot_dict else {
        panic!("still a square");
    };
    assert!(
        rect.llx < 10.0 && rect.ury > 90.0,
        "the rectangle GREW, in every direction: {rect:?}",
    );

    // The codec does not.
    assert_eq!(decode_spec(&encode_spec(&cloudy)).expect("codec"), cloudy);
}

/// The trap, called out on its own so a failure names itself.
#[test]
fn a_revision_cloud_does_not_come_back_as_a_plain_polygon() {
    let s = session("hello.pdf");
    let spec = MarkupSpec::Cloud {
        vertices: vec![(10.0, 10.0), (110.0, 20.0), (60.0, 90.0)],
        border: Some(Color::Rgb(1.0, 0.25, 0.0)),
        interior: None,
        width: 2.0,
        intensity: 1.25,
    };
    let back = spec_from_dict(&s.graph(), &build_appearance(&spec).annot).expect("read back");
    assert!(
        matches!(back, MarkupSpec::Cloud { .. }),
        "there is no /Cloud subtype in ISO 32000 -- a cloud IS a /Polygon with \
         /BE << /S /C >>, so it is recoverable only by reading /BE back. \
         Losing that flattens every revision cloud that crosses a document \
         boundary, and the result is still a valid polygon, so nothing else \
         would notice. Got {back:?}",
    );
}

// ---------------------------------------------------------------------------
// The clip file itself
// ---------------------------------------------------------------------------

/// ★ An annotation copied to a clip **file** pastes into another document.
///
/// This is the capability the whole Pass exists for. Until it landed,
/// `ObjectClip::to_bytes` dropped every annotation and `from_bytes` restored
/// an empty vector — so `pdfcer` could never paste an annotation of any
/// kind, because the CLI only ever has the file. The entire annotation half
/// of the clipboard was reachable in-process, and nothing in this workspace
/// was that in-process caller.
#[test]
fn an_annotation_survives_the_clip_file_and_pastes_into_another_document() {
    use pdfcer_core::vector::{Matrix, ObjectClip};

    let mut source = session("hello.pdf");
    let spec = MarkupSpec::Cloud {
        vertices: vec![(20.0, 20.0), (120.0, 30.0), (70.0, 100.0)],
        border: Some(Color::Rgb(1.0, 0.25, 0.0)),
        interior: None,
        width: 2.0,
        intensity: 1.25,
    };
    source.add_markup(0, &spec).expect("author");
    let clip = source.copy_annotations(0, &[0]).expect("copy");
    assert!(
        clip.annotations_survive_serialisation(),
        "and the clip says so",
    );

    // OUT THROUGH BYTES, exactly as the CLI does it.
    let wired = ObjectClip::from_bytes(&clip.to_bytes()).expect("round trip");
    assert_eq!(
        wired.annotation_count(),
        1,
        "the annotation is still on the clip after the wire",
    );

    let mut destination = session("minimal.pdf");
    let before = destination
        .page_slots()
        .expect("slots")
        .first()
        .map(|s| pdfcer_core::annot::page_annotations(&destination.graph(), s.id).len())
        .unwrap_or(0);
    let outcome = destination
        .paste_objects(0, &wired, Matrix::IDENTITY)
        .expect("paste");
    assert_eq!(outcome.annotations_pasted, 1);

    let slots = destination.page_slots().expect("slots");
    let page = slots.first().expect("a page").id;
    let placed = pdfcer_core::annot::page_annotations(&destination.graph(), page);
    assert_eq!(placed.len(), before + 1, "it landed");

    // And it landed as a CLOUD, not as the plain polygon a lossy route would
    // have produced -- checked through the parser, on the destination.
    let id = placed
        .last()
        .and_then(|a| a.id)
        .expect("the pasted annotation has an id");
    let graph = destination.graph();
    let dict = graph.resolved(id).as_dict().cloned().expect("a dict");
    let back = spec_from_dict(&graph, &dict).expect("reads back as a spec");
    assert!(
        matches!(back, MarkupSpec::Cloud { .. }),
        "a revision cloud that crossed a document boundary is still a cloud: {back:?}",
    );
}

/// A version-1 clip — one written before annotations were carried — still
/// pastes its content.
///
/// The reader must not refuse an older payload just because the format grew.
#[test]
fn a_clip_from_before_annotations_were_carried_still_reads() {
    use pdfcer_core::vector::ObjectClip;
    let source = session("hello.pdf");
    let clip = source.copy_objects(0, &[1]).expect("copy a path");
    let mut bytes = clip.to_bytes();
    // Rewrite the version word to 1 and truncate the annotation block, which
    // is what a v1 writer produced.
    bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
    let back = ObjectClip::from_bytes(&bytes).expect("a v1 payload still reads");
    assert_eq!(back.len(), 1, "its content came through");
    assert_eq!(back.annotation_count(), 0);
}

/// A ce dimension crosses the wire with its geometry and its group name.
#[test]
fn a_ce_dimension_survives_the_clip_file() {
    use pdfcer_core::dimension::{DimensionKind, Unit};
    use pdfcer_core::vector::{ClipAnnotation, ObjectClip};

    let mut s = session("hello.pdf");
    let group = s
        .add_dimension_group("Plan", Unit::Millimeter)
        .expect("group");
    s.add_dimension(
        0,
        group,
        DimensionKind::Linear {
            a: pdfcer_core::vector::Point::new(20.0, 20.0),
            b: pdfcer_core::vector::Point::new(120.0, 20.0),
            constraint: pdfcer_core::vector::AxisConstraint::Aligned,
            offset: 12.0,
            text_along: 0.5,
        },
    )
    .expect("dimension");

    let slots = s.page_slots().expect("slots");
    let page = slots.first().expect("page").id;
    let index = pdfcer_core::annot::page_annotations(&s.graph(), page)
        .iter()
        .position(|a| a.id.is_some())
        .expect("the ce dimension is on the page");

    let clip = s.copy_annotations(0, &[index]).expect("copy");
    let wired = ObjectClip::from_bytes(&clip.to_bytes()).expect("round trip");
    assert_eq!(
        wired.annotations, clip.annotations,
        "the ce dimension survived the wire unchanged -- geometry, group name \
         and unit",
    );
    match wired.annotations.first() {
        Some(ClipAnnotation::Dimension {
            group_name,
            format,
            scale,
            ..
        }) => {
            assert_eq!(group_name, "Plan");
            assert_eq!(format.unit, Unit::Millimeter);
            // ★ `Pass 173.1`: the SCALE travels. Without it a pasted ce
            // dimension landed in a freshly created default-styled group and
            // its label -- which is DERIVED from the scale -- read a
            // different number, with nothing erroring.
            assert_eq!(*scale, pdfcer_core::dimension::ScaleState::NeverSet);
        }
        other => panic!("expected a ce dimension, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The raw carrier (`Pass 170.0`)
// ---------------------------------------------------------------------------

/// ★ A **sticky note** copies and pastes — the single most-copied comment in
/// a review workflow, and until `Pass 170.0` the clipboard refused it.
///
/// pdfcer could always *author* one (`TextAnnotSpec::Sticky`); it had no
/// reader that turned one back into a spec, so it landed in
/// `ClipAnnotation::Unsupported` and the paste declined it by name. The raw
/// carrier needs no reader: the annotation travels as its own dictionary plus
/// the closure it reaches, which is both simpler and lossless.
#[test]
fn a_sticky_note_survives_a_copy_and_a_paste() {
    use pdfcer_core::annot_author::{StickyIcon, TextAnnotSpec};
    use pdfcer_core::object::{Name, Object};
    use pdfcer_core::vector::{Matrix, ObjectClip};

    let mut s = session("hello.pdf");
    s.add_text_annotation(
        0,
        &TextAnnotSpec::Sticky {
            rect: Rect {
                llx: 30.0,
                lly: 30.0,
                urx: 54.0,
                ury: 54.0,
            },
            icon: StickyIcon::Comment,
            contents: "check this dimension".to_owned(),
            color: Color::Rgb(1.0, 0.85, 0.0),
            open: false,
        },
    )
    .expect("author a sticky note");

    let clip = s.copy_annotations(0, &[0]).expect("copy");
    let wired = ObjectClip::from_bytes(&clip.to_bytes()).expect("through the wire");
    assert_eq!(wired.annotations, clip.annotations, "unchanged on the wire");

    let mut destination = session("minimal.pdf");
    let outcome = destination
        .paste_objects(0, &wired, Matrix::translate(40.0, 0.0))
        .expect("paste");
    assert_eq!(outcome.annotations_pasted, 1, "it was PLACED, not refused");

    let slots = destination.page_slots().expect("slots");
    let page = slots.first().expect("page").id;
    let placed = pdfcer_core::annot::page_annotations(&destination.graph(), page);
    let id = placed.last().and_then(|a| a.id).expect("an id");
    let graph = destination.graph();
    let dict = graph.resolved(id).as_dict().cloned().expect("a dict");

    // The things a MODEL route would have had to re-derive, and that the raw
    // carrier simply keeps.
    assert_eq!(
        dict.get(b"Subtype").and_then(Object::as_name),
        Some(&Name::from(b"Text")),
    );
    assert!(
        matches!(dict.get(b"Contents"), Some(Object::String(t)) if t == b"check this dimension"),
        "the note text came with it",
    );
    assert!(dict.get(b"Name").is_some(), "and so did the icon");
    assert!(dict.get(b"AP").is_some(), "and the baked appearance");

    // The source-bound keys did NOT travel; `/P` was rewritten to where it
    // landed rather than dropped.
    assert_eq!(
        dict.get(b"P").and_then(Object::as_reference),
        Some(page),
        "/P names the page it landed on",
    );
    assert!(dict.get(b"NM").is_none(), "/NM must be unique per page");
    assert!(
        dict.get(b"StructParent").is_none(),
        "/StructParent indexes the SOURCE document's parent tree",
    );
}

/// A **stamp** travels with its artwork.
#[test]
fn a_stamp_survives_a_copy_and_a_paste() {
    use pdfcer_core::annot_author::{StampName, TextAnnotSpec};
    use pdfcer_core::vector::{Matrix, ObjectClip};

    let mut s = session("hello.pdf");
    s.add_text_annotation(
        0,
        &TextAnnotSpec::Stamp {
            rect: Rect {
                llx: 20.0,
                lly: 20.0,
                urx: 140.0,
                ury: 60.0,
            },
            name: StampName::Draft,
            label: Some("SUPERSEDED".to_owned()),
            color: Color::Rgb(0.8, 0.0, 0.0),
        },
    )
    .expect("author a stamp");
    let clip = s.copy_annotations(0, &[0]).expect("copy");
    let wired = ObjectClip::from_bytes(&clip.to_bytes()).expect("wire");
    let mut destination = session("minimal.pdf");
    assert_eq!(
        destination
            .paste_objects(0, &wired, Matrix::IDENTITY)
            .expect("paste")
            .annotations_pasted,
        1,
    );
}

/// A `/FreeText` travels with everything a model route would have dropped —
/// and a model route dropped **the whole annotation**, because pdfcer has no
/// reader that turns one back into a spec.
#[test]
fn a_text_box_travels_with_everything_a_model_route_would_have_dropped() {
    use pdfcer_core::annot_author::TextAnnotSpec;
    use pdfcer_core::fontdata::Std14;
    use pdfcer_core::object::Object;
    use pdfcer_core::vartext::{Quadding, TextColor};
    use pdfcer_core::vector::{Matrix, ObjectClip};

    let mut s = session("hello.pdf");
    s.add_text_annotation(
        0,
        &TextAnnotSpec::FreeText {
            rect: Rect {
                llx: 20.0,
                lly: 20.0,
                urx: 180.0,
                ury: 70.0,
            },
            text: "revise per RFI 12".to_owned(),
            font: Std14::Helvetica,
            font_size: 10.0,
            color: TextColor::Rgb(0.0, 0.0, 0.8),
            quadding: Quadding::Left,
            multiline: true,
            border: Some(Color::Gray(0.0)),
            border_width: 1.0,
        },
    )
    .expect("author a text box");

    let clip = s.copy_annotations(0, &[0]).expect("copy");
    let wired = ObjectClip::from_bytes(&clip.to_bytes()).expect("wire");
    let mut destination = session("minimal.pdf");
    destination
        .paste_objects(0, &wired, Matrix::IDENTITY)
        .expect("paste");

    let slots = destination.page_slots().expect("slots");
    let page = slots.first().expect("page").id;
    let placed = pdfcer_core::annot::page_annotations(&destination.graph(), page);
    let id = placed.last().and_then(|a| a.id).expect("an id");
    let graph = destination.graph();
    let dict = graph.resolved(id).as_dict().cloned().expect("dict");
    assert!(
        matches!(dict.get(b"Contents"), Some(Object::String(t)) if t == b"revise per RFI 12"),
        "the words came with it",
    );
    assert!(dict.get(b"DA").is_some(), "and the appearance string");
}

/// A `/Popup` is refused **by policy**, not for want of a model.
///
/// §12.5.6.14 makes a pop-up belong to the comment that opens it. One pasted
/// on its own would have no parent, so it is refused with that reason rather
/// than planted as an orphan.
#[test]
fn a_popup_is_refused_because_it_is_not_an_independent_annotation() {
    use pdfcer_core::vector::{ClipAnnotation, Matrix};
    let mut s = session("annot/demo-annotated.pdf");
    let clip = s.copy_annotations(0, &[3]).expect("copy");
    assert!(
        matches!(
            clip.annotations.first(),
            Some(ClipAnnotation::Unsupported { subtype }) if subtype == "Popup"
        ),
        "got {:?}",
        clip.annotations.first(),
    );
    let said = s
        .paste_objects(0, &clip, Matrix::IDENTITY)
        .expect("paste reports")
        .disclosures
        .join(" ");
    assert!(
        said.contains("belongs to the comment that opens it"),
        "the refusal gives the POP-UP's reason: {said:?}",
    );
}

/// ★★ A pasted ce dimension shows THE SAME NUMBER as the one it was copied
/// from (`Pass 173.1`).
///
/// This is the acceptance criterion `Pass 120.5` stated in 2026-08-21 and
/// `Pass 169.0` did not meet: carry a ce dimension's group **name, scale and
/// unit**. `169.0` carried the name and the unit.
///
/// A ce dimension's label is **derived** from its group's scale. Landing in a
/// freshly created, uncalibrated group changed the number on the page —
/// nothing errored, nothing was marked, and the drawing simply said something
/// else. That is the worst shape a defect can take in a measuring tool.
#[test]
fn a_pasted_ce_dimension_reads_the_same_number_it_was_copied_from() {
    use pdfcer_core::dimension::{DimensionKind, NumberFormat, ScaleState, Unit};
    use pdfcer_core::vector::{ObjectClip, Point};

    let mut source = session("hello.pdf");
    let group = source
        .add_dimension_group("Plan", Unit::Millimeter)
        .expect("group");
    // 1 pt = 10 mm. A 100 pt span therefore reads 1000 mm here, and would
    // read something else in an uncalibrated group.
    source
        .set_group_scale(
            group,
            ScaleState::Calibrated { scale: 10.0 },
            NumberFormat {
                unit: Unit::Millimeter,
                ..Unit::Millimeter.default_format()
            },
        )
        .expect("calibrate");
    source
        .add_dimension(
            0,
            group,
            DimensionKind::Linear {
                a: Point::new(20.0, 20.0),
                b: Point::new(120.0, 20.0),
                constraint: pdfcer_core::vector::AxisConstraint::Aligned,
                offset: 12.0,
                text_along: 0.5,
            },
        )
        .expect("ce dimension");

    let slots = source.page_slots().expect("slots");
    let page = slots.first().expect("page").id;
    let index = pdfcer_core::annot::page_annotations(&source.graph(), page)
        .iter()
        .position(|a| a.id.is_some())
        .expect("it is on the page");
    let clip = source.copy_annotations(0, &[index]).expect("copy");
    let wired = ObjectClip::from_bytes(&clip.to_bytes()).expect("through the wire");

    // The SCALE is on the clip -- the whole point of the Pass.
    match wired.annotations.first() {
        Some(pdfcer_core::vector::ClipAnnotation::Dimension { scale, .. }) => {
            assert_eq!(
                *scale,
                ScaleState::Calibrated { scale: 10.0 },
                "the group's scale crossed the wire",
            );
        }
        other => panic!("expected a ce dimension, got {other:?}"),
    }

    // And it lands in a group calibrated the same way, so the LABEL agrees.
    let mut destination = session("hello.pdf");
    destination
        .paste_objects(0, &wired, pdfcer_core::vector::Matrix::IDENTITY)
        .expect("paste");
    let model = destination.dimension_model();
    let landed = model
        .groups()
        .iter()
        .find(|g| g.name == "Plan")
        .expect("the group was created");
    assert_eq!(
        landed.scale,
        ScaleState::Calibrated { scale: 10.0 },
        "a pasted ce dimension measures at the scale it was drawn at -- \
         before this Pass it landed uncalibrated and the number on the page \
         changed",
    );
    assert_eq!(landed.unit(), Unit::Millimeter);
}

/// ★ When the destination ALREADY has a group of that name, **its** scale
/// wins — and the paste says so.
///
/// The alternative would be to re-scale the operator's existing group to
/// match an incoming clip, which would change the label of **every ce
/// dimension already in it** — objects that were not part of the paste.
#[test]
fn an_existing_group_keeps_its_own_scale_and_the_paste_discloses_it() {
    use pdfcer_core::dimension::{DimensionKind, NumberFormat, ScaleState, Unit};
    use pdfcer_core::vector::{Matrix, Point};

    let make = |scale: f64| {
        let mut s = session("hello.pdf");
        let group = s.add_dimension_group("Plan", Unit::Millimeter).expect("g");
        s.set_group_scale(
            group,
            ScaleState::Calibrated { scale },
            NumberFormat {
                unit: Unit::Millimeter,
                ..Unit::Millimeter.default_format()
            },
        )
        .expect("calibrate");
        (s, group)
    };

    let (mut source, group) = make(10.0);
    source
        .add_dimension(
            0,
            group,
            DimensionKind::Linear {
                a: Point::new(20.0, 20.0),
                b: Point::new(120.0, 20.0),
                constraint: pdfcer_core::vector::AxisConstraint::Aligned,
                offset: 12.0,
                text_along: 0.5,
            },
        )
        .expect("ce dimension");
    let slots = source.page_slots().expect("slots");
    let page = slots.first().expect("page").id;
    let index = pdfcer_core::annot::page_annotations(&source.graph(), page)
        .iter()
        .position(|a| a.id.is_some())
        .expect("on the page");
    let clip = source.copy_annotations(0, &[index]).expect("copy");

    // The destination has a "Plan" group at a DIFFERENT scale.
    let (mut destination, _) = make(25.0);
    let outcome = destination
        .paste_objects(0, &clip, Matrix::IDENTITY)
        .expect("paste");
    let said = outcome.disclosures.join(" ");
    assert!(
        said.contains("ITS scale was kept"),
        "the operator is told the label may read differently: {said:?}",
    );
    let model = destination.dimension_model();
    let landed = model
        .groups()
        .iter()
        .find(|g| g.name == "Plan")
        .expect("group");
    assert_eq!(
        landed.scale,
        ScaleState::Calibrated { scale: 25.0 },
        "the destination's own group was NOT re-scaled to match the clip",
    );
}
