//! Ambiguity **`QP-A1`** — `/QuadPoints` corner order (ISO 32000-1
//! §12.5.6.10) — is now an operator setting rather than a fixed choice.
//!
//! # Why this ambiguity is the register's "worst case"
//!
//! §12.5.6.10 states a corner order and essentially no producer follows it:
//! Acrobat, PDFBox and pdf.js all emit **Z / reading order** (`UL, UR, LL,
//! LR`) while the clause describes a **counterclockwise** walk (`UL, UR, LR,
//! LL`). The two differ only in the last pair.
//!
//! What makes it the worst entry in
//! `iso32000__ref__ambiguity_settings_register.md` is not the size of the
//! divergence but its **invisibility**: pdfcer bakes a full `/AP` (R44), so
//! its own rendering never consults `/QuadPoints` at all. The order is
//! consumed only by a *third party* re-deriving geometry from the array —
//! and there, the wrong pair ordering crosses the bottom edge and describes a
//! **bow-tie** rather than a rectangle.
//!
//! A test that rendered the annotation would therefore pass under both
//! orders. These assert on the emitted **numbers**, which is the only place
//! the choice exists.

use pdfcer_core::annot_author::{
    Color, MarkupSpec, Quad, RedactSpec, TextMarkupKind, build_appearance, build_appearance_with,
    build_redact_mark, build_redact_mark_with,
};
use pdfcer_core::object::Object;
use pdfcer_core::settings::QuadPointOrder;

/// A quad whose four corners are all distinct, so any permutation is
/// detectable. A square would make two orders indistinguishable on two of the
/// eight numbers.
fn quad() -> Quad {
    Quad {
        ul: (10.0, 90.0),
        ur: (70.0, 92.0),
        ll: (12.0, 40.0),
        lr: (74.0, 44.0),
    }
}

fn quad_points(annot: &pdfcer_core::object::Dict) -> Vec<f64> {
    match annot.get(b"QuadPoints") {
        Some(Object::Array(a)) => a
            .iter()
            .map(|o| match o {
                Object::Real(v) => *v,
                Object::Integer(v) => *v as f64,
                other => panic!("non-numeric quad point: {other:?}"),
            })
            .collect(),
        other => panic!("no /QuadPoints array: {other:?}"),
    }
}

fn markup_with(order: QuadPointOrder) -> Vec<f64> {
    let spec = MarkupSpec::TextMarkup {
        kind: TextMarkupKind::Highlight,
        quads: vec![quad()],
        color: Color::Rgb(1.0, 1.0, 0.0),
    };
    quad_points(&build_appearance_with(&spec, order).annot)
}

/// Would catch: the setting existing and changing nothing — a knob wired to
/// no behaviour, which is worse than no knob because it reads as a promise.
#[test]
fn the_two_orders_emit_different_bytes_and_differ_only_in_the_last_pair() {
    let reading = markup_with(QuadPointOrder::ReadingOrder);
    let ccw = markup_with(QuadPointOrder::Counterclockwise);
    assert_ne!(reading, ccw, "the setting must actually change the output");

    let q = quad();
    assert_eq!(
        reading,
        vec![
            q.ul.0, q.ul.1, q.ur.0, q.ur.1, q.ll.0, q.ll.1, q.lr.0, q.lr.1
        ],
        "reading order is UL, UR, LL, LR"
    );
    assert_eq!(
        ccw,
        vec![
            q.ul.0, q.ul.1, q.ur.0, q.ur.1, q.lr.0, q.lr.1, q.ll.0, q.ll.1
        ],
        "the counterclockwise walk is UL, UR, LR, LL"
    );

    // The whole of QP-A1 is the last pair. Pinned so a future change that
    // touched the top corners would be caught as the different bug it is.
    assert_eq!(
        reading[..4],
        ccw[..4],
        "the first two corners are not in dispute — only the bottom pair is"
    );
}

/// Would catch: the default flipping to the spec-literal order.
///
/// The default is **reading order**, and that is a deliberate divergence from
/// §12.5.6.10 rather than an oversight. A markup annotation is read by
/// whatever the recipient already has — overwhelmingly Acrobat, PDFBox or
/// pdf.js, all of which emit and expect this order. A file that is
/// spec-literal and draws a bow-tie in the reader the recipient actually
/// opened has helped nobody.
///
/// The other order exists for output destined for a conformance checker.
#[test]
fn the_default_is_the_interoperable_order_not_the_spec_literal_one() {
    assert_eq!(QuadPointOrder::default(), QuadPointOrder::ReadingOrder);
    let spec = MarkupSpec::TextMarkup {
        kind: TextMarkupKind::Highlight,
        quads: vec![quad()],
        color: Color::Rgb(1.0, 1.0, 0.0),
    };
    assert_eq!(
        quad_points(&build_appearance(&spec).annot),
        markup_with(QuadPointOrder::ReadingOrder),
        "the no-argument entry point must agree with the explicit default"
    );
}

/// Would catch: `/Redact` being left on the old fixed order while text markup
/// gained the setting — the shape of bug that appears when one of two
/// `/QuadPoints` writers is found and the other is not.
///
/// `/Redact` matters more than text markup here, not less: a third-party tool
/// that re-derived a redaction region from mis-ordered quads would remove a
/// bow-tie rather than the marked rectangle. pdfcer applies its own redactions
/// from the parsed quads, so it cannot mis-redact itself; the hazard is
/// hand-off.
#[test]
fn redaction_marks_honour_the_same_setting() {
    let q = vec![quad()];
    let spec = RedactSpec {
        quads: q,
        fill: None,
        overlay_text: None,
        quadding: pdfcer_core::vartext::Quadding::default(),
    };
    let reading = quad_points(&build_redact_mark_with(&spec, QuadPointOrder::ReadingOrder).annot);
    let ccw = quad_points(&build_redact_mark_with(&spec, QuadPointOrder::Counterclockwise).annot);
    assert_ne!(
        reading, ccw,
        "the redact writer must honour the setting too"
    );
    assert_eq!(
        quad_points(&build_redact_mark(&spec).annot),
        reading,
        "and its default must match text markup's"
    );
    assert_eq!(
        &reading[..4],
        &ccw[..4],
        "same invariant: only the bottom pair moves"
    );
}

/// Would catch: the setting not round-tripping through the settings file, or
/// its token drifting from what the parser accepts.
///
/// A knob that cannot be set is not a knob. This is asserted through the
/// public parse path rather than by constructing the enum, because the token
/// string is the actual operator-facing surface.
#[test]
fn the_setting_round_trips_through_the_settings_file() {
    for (token, expected) in [
        ("reading_order", QuadPointOrder::ReadingOrder),
        ("counterclockwise", QuadPointOrder::Counterclockwise),
    ] {
        let mut notes = Vec::new();
        let settings = pdfcer_core::settings::Settings::parse(
            &format!("quad_point_order = {token}\n"),
            &mut notes,
        );
        assert_eq!(
            settings.quad_point_order, expected,
            "token {token:?} must parse"
        );
        assert!(
            notes.is_empty(),
            "a valid token must produce no note: {notes:?}"
        );
    }

    // An unrecognised value must be reported, not silently accepted — the
    // operator asked for something and pdfcer did something else.
    let mut notes = Vec::new();
    let settings =
        pdfcer_core::settings::Settings::parse("quad_point_order = widdershins\n", &mut notes);
    assert_eq!(
        settings.quad_point_order,
        QuadPointOrder::default(),
        "a bad value must leave the default in place"
    );
    assert!(
        !notes.is_empty(),
        "a bad value must be disclosed, not swallowed"
    );
}
