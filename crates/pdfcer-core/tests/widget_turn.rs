//! A form widget turned to any angle, and a widget's `/CA` opacity
//! (request `G160`).
//!
//! The free angle lives in the appearance `/Matrix` on top of `/MK /R`, and
//! `/Rect` is the upright bound of the turned artwork (ISO 32000-1 §12.5.5).
//! What must hold: the angle is absolute (turning twice does not grow the
//! box), a redraw keeps both the angle and the logical size, and anything
//! that cannot keep the angle refuses rather than standing the widget up.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, NewCheckBox, NewTextField, WidgetEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree::Rect;
use std::path::Path;

fn session() -> EditSession {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/dimension/plain-base.pdf");
    EditSession::new(Document::load(&p).unwrap())
}

/// Not square, so a lost swap or a lost logical size shows.
const BOX: Rect = Rect {
    llx: 100.0,
    lly: 100.0,
    urx: 200.0,
    ury: 120.0,
};

fn text(s: &mut EditSession) {
    s.add_text_field(&NewTextField::new(0, "T", BOX).declining_tooltip())
        .unwrap();
}

fn widget(s: &EditSession, name: &str) -> forms::Widget {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .unwrap()
        .widgets[0]
        .clone()
}

fn widget_dict(s: &EditSession, name: &str) -> Dict {
    let id = widget(s, name).id;
    s.graph().resolved(id).as_dict().cloned().unwrap()
}

/// Every `/AP /N` stream dict of the widget.
fn normal(s: &EditSession, name: &str) -> Vec<Dict> {
    let g = s.graph();
    let d = widget_dict(s, name);
    let Some(Object::Dict(ap)) = d.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    match ap.get(b"N").map(|o| g.resolve(o).clone()) {
        Some(Object::Stream(st)) => vec![st.dict],
        Some(Object::Dict(states)) => states
            .0
            .iter()
            .filter_map(|(_, v)| match g.resolve(v) {
                Object::Stream(st) => Some(st.dict.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn nums(d: &Dict, key: &[u8]) -> Option<Vec<f64>> {
    d.get(key)?
        .as_array()?
        .iter()
        .map(Object::as_number)
        .collect()
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

fn rect_of(s: &EditSession, name: &str) -> Vec<f64> {
    nums(&widget_dict(s, name), b"Rect").unwrap()
}

fn turned(deg: f64) -> Vec<f64> {
    let (sn, c) = deg.to_radians().sin_cos();
    vec![c, sn, -sn, c, 0.0, 0.0]
}

/// The upright bound of a `w x h` box turned `deg` about BOX's centre.
fn bound(deg: f64) -> Vec<f64> {
    let (sn, c) = deg.to_radians().sin_cos();
    let (hw, hh) = (
        (100.0 * c.abs() + 20.0 * sn.abs()) / 2.0,
        (100.0 * sn.abs() + 20.0 * c.abs()) / 2.0,
    );
    vec![150.0 - hw, 110.0 - hh, 150.0 + hw, 110.0 + hh]
}

#[test]
fn a_turn_writes_the_matrix_and_the_bounding_rect() {
    let mut s = session();
    text(&mut s);
    let out = s.turn_widget("T", 0, 30.0).unwrap();
    assert!(out.changed);
    assert_eq!((out.was, out.now), (0.0, 30.0));
    for d in normal(&s, "T") {
        assert!(close(&nums(&d, b"Matrix").unwrap(), &turned(30.0)));
        assert!(close(&nums(&d, b"BBox").unwrap(), &[0.0, 0.0, 100.0, 20.0]));
    }
    assert!(
        close(&rect_of(&s, "T"), &bound(30.0)),
        "{:?}",
        rect_of(&s, "T")
    );
    assert!(
        out.disclosures.iter().any(|d| d.contains("/MK")),
        "a regenerating viewer drops the free angle, and that is disclosed"
    );
}

#[test]
fn the_angle_is_absolute_so_turning_twice_does_not_grow_the_box() {
    let mut s = session();
    text(&mut s);
    s.turn_widget("T", 0, 30.0).unwrap();
    let out = s.turn_widget("T", 0, 45.0).unwrap();
    assert_eq!((out.was, out.now), (30.0, 45.0));
    assert!(close(&rect_of(&s, "T"), &bound(45.0)));
    let again = s.turn_widget("T", 0, 45.0).unwrap();
    assert!(!again.changed, "already there: nothing written");
    assert!(close(&rect_of(&s, "T"), &bound(45.0)));
}

#[test]
fn turning_back_to_zero_restores_the_box_and_drops_the_matrix() {
    let mut s = session();
    text(&mut s);
    s.turn_widget("T", 0, -70.0).unwrap();
    let out = s.turn_widget("T", 0, 0.0).unwrap();
    assert!(out.disclosures.is_empty(), "upright needs no disclosure");
    assert!(close(&rect_of(&s, "T"), &[100.0, 100.0, 200.0, 120.0]));
    for d in normal(&s, "T") {
        assert!(d.get(b"Matrix").is_none());
    }
}

#[test]
fn a_fill_after_a_turn_keeps_the_angle_and_the_size() {
    let mut s = session();
    text(&mut s);
    s.turn_widget("T", 0, 30.0).unwrap();
    let rect = rect_of(&s, "T");
    s.fill_text_field("T", "hello").unwrap();
    for d in normal(&s, "T") {
        assert!(close(&nums(&d, b"Matrix").unwrap(), &turned(30.0)));
        assert!(
            close(&nums(&d, b"BBox").unwrap(), &[0.0, 0.0, 100.0, 20.0]),
            "drawn at the logical size, not at the turned bound"
        );
    }
    assert_eq!(rect_of(&s, "T"), rect, "/Rect untouched by a fill");
}

#[test]
fn a_free_turn_sits_on_top_of_a_quarter_turn() {
    let mut s = session();
    text(&mut s);
    s.rotate_widget("T", 0, 90).unwrap();
    let out = s.turn_widget("T", 0, 30.0).unwrap();
    assert_eq!(out.was, 0.0, "the quarter turn is /MK /R, not a free angle");
    s.fill_text_field("T", "x").unwrap();
    for d in normal(&s, "T") {
        assert!(close(&nums(&d, b"Matrix").unwrap(), &turned(120.0)));
        assert!(close(&nums(&d, b"BBox").unwrap(), &[0.0, 0.0, 20.0, 100.0]));
    }
}

#[test]
fn a_check_box_turns_every_state_and_keeps_it_through_a_toggle() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "C", BOX).declining_tooltip())
        .unwrap();
    s.turn_widget("C", 0, 15.0).unwrap();
    let states = normal(&s, "C");
    assert_eq!(states.len(), 2);
    for d in &states {
        assert!(close(&nums(d, b"Matrix").unwrap(), &turned(15.0)));
    }
    // A colour change redraws pdfcer's own button: still turned, still the
    // logical size.
    let out = s
        .edit_widget(
            "C",
            0,
            &WidgetEdit::new().with_background(forms::MkColor::Rgb(1.0, 1.0, 0.0)),
        )
        .unwrap();
    assert!(
        out.appearance_regenerated,
        "pdfcer's own box, recognised turned"
    );
    for d in normal(&s, "C") {
        assert!(close(&nums(&d, b"Matrix").unwrap(), &turned(15.0)));
        assert!(close(&nums(&d, b"BBox").unwrap(), &[0.0, 0.0, 100.0, 20.0]));
    }
}

#[test]
fn quarter_turns_and_bad_angles_are_refused() {
    let mut s = session();
    text(&mut s);
    assert!(matches!(
        s.turn_widget("T", 0, 90.0),
        Err(EditError::WidgetTurnIsQuarterTurn { .. })
    ));
    assert!(matches!(
        s.turn_widget("T", 0, -270.0),
        Err(EditError::WidgetTurnIsQuarterTurn { .. })
    ));
    assert!(matches!(
        s.turn_widget("T", 0, f64::NAN),
        Err(EditError::ResizeFactorInvalid { .. })
    ));
    assert!(matches!(
        s.turn_widget("T", 3, 10.0),
        Err(EditError::WidgetIndexOutOfRange { .. })
    ));
    assert_eq!(rect_of(&s, "T"), vec![100.0, 100.0, 200.0, 120.0]);
}

#[test]
fn rotate_and_resize_refuse_a_turned_widget_but_a_move_does_not() {
    let mut s = session();
    text(&mut s);
    s.turn_widget("T", 0, 30.0).unwrap();
    assert!(matches!(
        s.rotate_widget("T", 0, 90),
        Err(EditError::WidgetTurned { .. })
    ));
    let r = rect_of(&s, "T");
    let grow = WidgetEdit::new().with_rect(Rect {
        llx: r[0],
        lly: r[1],
        urx: r[2] + 10.0,
        ury: r[3],
    });
    assert!(matches!(
        s.edit_widget("T", 0, &grow),
        Err(EditError::WidgetTurned { .. })
    ));
    let slide = WidgetEdit::new().with_rect(Rect {
        llx: r[0] + 5.0,
        lly: r[1],
        urx: r[2] + 5.0,
        ury: r[3],
    });
    s.edit_widget("T", 0, &slide).unwrap();
    for d in normal(&s, "T") {
        assert!(close(&nums(&d, b"Matrix").unwrap(), &turned(30.0)));
    }
}

#[test]
fn a_turn_is_one_undo_step() {
    let mut s = session();
    text(&mut s);
    let depth = s.undo_depth();
    s.turn_widget("T", 0, 30.0).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    s.undo();
    assert_eq!(rect_of(&s, "T"), vec![100.0, 100.0, 200.0, 120.0]);
    for d in normal(&s, "T") {
        assert!(d.get(b"Matrix").is_none());
    }
}

#[test]
fn opacity_is_written_cleared_and_range_checked() {
    let mut s = session();
    text(&mut s);
    let out = s
        .edit_widget("T", 0, &WidgetEdit::new().with_opacity(0.4))
        .unwrap();
    assert_eq!(
        widget_dict(&s, "T").get(b"CA").and_then(Object::as_number),
        Some(0.4)
    );
    assert!(out.opacity_disclosure.is_some());
    let opaque = s
        .edit_widget("T", 0, &WidgetEdit::new().with_opacity(1.0))
        .unwrap();
    assert!(opaque.opacity_disclosure.is_none());
    s.edit_widget("T", 0, &WidgetEdit::new().clearing_opacity())
        .unwrap();
    assert!(widget_dict(&s, "T").get(b"CA").is_none());
    for bad in [1.5, -0.1, f64::NAN] {
        assert!(matches!(
            s.edit_widget("T", 0, &WidgetEdit::new().with_opacity(bad)),
            Err(EditError::WidgetOpacityOutOfRange { .. })
        ));
    }
}

/// Two fields whose widgets share ONE appearance stream (legal, and what a
/// copy-pasting producer emits).
fn shared() -> EditSession {
    let ap = b"0 0 1 rg 0 0 100 20 re f";
    let mut pdf = Vec::new();
    pdf.extend_from_slice(
        b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R] >> >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R 5 0 R] >> endobj\n\
4 0 obj << /Type /Annot /Subtype /Widget /FT /Tx /T (A) /P 3 0 R /Rect [100 100 200 120] /AP << /N 6 0 R >> >> endobj\n\
5 0 obj << /Type /Annot /Subtype /Widget /FT /Tx /T (B) /P 3 0 R /Rect [100 200 200 220] /AP << /N 6 0 R >> >> endobj\n",
    );
    pdf.extend_from_slice(
        format!(
            "6 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 20] /Length {} >> stream\n",
            ap.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(ap);
    pdf.extend_from_slice(b"\nendstream endobj\ntrailer << /Size 7 /Root 1 0 R >>\n");
    EditSession::new(Document::from_bytes(pdf).unwrap())
}

#[test]
fn a_shared_appearance_is_copied_so_the_other_widget_stays_upright() {
    let mut s = shared();
    let out = s.turn_widget("A", 0, 30.0).unwrap();
    assert!(
        out.disclosures.iter().any(|d| d.contains("shared")),
        "{:?}",
        out.disclosures
    );
    assert!(close(
        &nums(&normal(&s, "A")[0], b"Matrix").unwrap(),
        &turned(30.0)
    ));
    assert!(
        normal(&s, "B")[0].get(b"Matrix").is_none(),
        "B did not turn"
    );
    assert_eq!(rect_of(&s, "B"), vec![100.0, 200.0, 200.0, 220.0]);
}

#[test]
fn a_foreign_appearance_turns_without_a_redraw() {
    let mut s = shared();
    s.turn_widget("B", 0, -20.0).unwrap();
    assert!(close(
        &nums(&normal(&s, "B")[0], b"Matrix").unwrap(),
        &turned(-20.0)
    ));
    let r = rect_of(&s, "B");
    assert!((r[0] + r[2] - 300.0).abs() < 1e-6 && (r[1] + r[3] - 420.0).abs() < 1e-6);
}
