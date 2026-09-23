//! **A `/Btn` rotation reaches the pixels** (`Pass 308.5`, request `G023`).
//!
//! ## The defect, as the operator met it
//!
//! *"The rotate buttons don't work for check boxes."*
//!
//! `rotate_widget` wrote `/MK` `/R` correctly for every field kind and then
//! regenerated. For a text or choice field `regen_field_appearance` reads the
//! staged angle, swaps the authored `/BBox` and emits `quarter_turn_matrix`.
//! For a button, `regen_button_appearance` read the staged **rect**, the
//! staged **caption** and the staged **colours** — and never the staged
//! rotation. `build_button_states` had no parameter to pass one to.
//!
//! So a button whose artwork pdfcer drew was recognised, redrawn from
//! unchanged inputs, rewritten **byte-identical**, and reported `Ok(true)`:
//!
//! * `appearance_regenerated = true`, so
//! * `appearance_stale = None`, so
//! * the shell printed *"Turned to 90° anticlockwise."*, and
//! * **zero pixels moved.**
//!
//! ⇒ *A redraw that reads three of four staged inputs reports the same
//! success as one that reads all four.* The boolean answers "did I rewrite
//! the stream", which was true; nobody could ask "did the rewrite differ".
//!
//! ## Why the renderer must not be where this is fixed
//!
//! `pdfcer-render` implements §12.5.5 from `/BBox` + `/Matrix` only and reads
//! no `/MK` at all. Per PDF-Association erratum #56 a conforming PDF 2.0
//! reader **ignores `/MK` at render time when an appearance stream is
//! present**, so teaching the renderer to honour `/MK` `/R` would be
//! deliberate non-conformance. The turn has to be baked.
//!
//! ## WHERE THE ROTATION LIVES, which is not where either of us assumed
//!
//! The requester warned that `Circle`, `Square` and `Cross` are rotationally
//! symmetric, so *"a test that rotates a `Circle` check box and asserts the
//! bytes changed will fail on a correct fix"* — pick the style deliberately.
//! Sound advice, and the conclusion is right. **The reason is not the style.**
//!
//! The content stream is **never drawn turned**. It is drawn upright into a
//! box, and `/Matrix` turns the whole form XObject. So the stream bytes are a
//! function of the box the drawing went into, and nothing else:
//!
//! | turn | box authored | stream bytes change? |
//! |---|---|---|
//! | 0° | `w × h` | — |
//! | 90° / 270° | **`h × w`** | only if `w ≠ h` |
//! | 180° | `w × h` | **never** |
//!
//! ⇒ **Style symmetry does not enter into it.** A `Check` box that is square
//! produces identical bytes at 90°, and a `Circle` box that is oblong
//! produces different ones — the opposite of what picking by style predicts.
//! At 180° *every* style and *every* shape of box is byte-identical, because
//! the drawing frame did not move; the entire turn is the `/Matrix`.
//!
//! Two tests here failed on the first run for exactly this, and both were the
//! TEST being wrong rather than the fix. So the assertions below are split:
//! **`/Matrix` is what proves a turn happened**, and the stream bytes are
//! asserted only where the authored box actually changed size.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::CheckStyle;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewCheckBox, NewPushButton, NewRadioButton};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("dimension/plain-base.pdf")).unwrap())
}

/// Deliberately NOT square: a quarter turn has to swap the authored `/BBox`,
/// and a square box would let a missing swap pass unnoticed.
fn rect() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 68.0,
        ury: 124.0,
    }
}

fn field_named(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .expect("an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .expect("the field")
}

/// Every `/AP` `/N` state of widget 0, as `(state name, stream bytes)`.
fn ap_states(s: &EditSession, name: &str) -> Vec<(String, Vec<u8>)> {
    let g = s.graph();
    let field = field_named(s, name);
    let dict = g
        .resolved(field.widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(n) = ap.get(b"N") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match g.resolve(n).clone() {
        Object::Stream(st) => out.push((
            "N".to_owned(),
            s.view().slice(st.data_span).unwrap_or_default().to_vec(),
        )),
        Object::Dict(states) => {
            for (k, v) in &states.0 {
                if let Object::Stream(st) = g.resolve(v).clone() {
                    out.push((
                        String::from_utf8_lossy(&k.0).into_owned(),
                        s.view().slice(st.data_span).unwrap_or_default().to_vec(),
                    ));
                }
            }
        }
        _ => {}
    }
    out
}

/// The `/AP` `/N` dictionaries of widget 0's states, for `/Matrix` and
/// `/BBox`.
fn ap_dicts(s: &EditSession, name: &str) -> Vec<pdfcer_core::object::Dict> {
    let g = s.graph();
    let field = field_named(s, name);
    let dict = g
        .resolved(field.widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(n) = ap.get(b"N") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match g.resolve(n).clone() {
        Object::Stream(st) => out.push(st.dict),
        Object::Dict(states) => {
            for (_, v) in &states.0 {
                if let Object::Stream(st) = g.resolve(v).clone() {
                    out.push(st.dict);
                }
            }
        }
        _ => {}
    }
    out
}

fn numbers(d: &pdfcer_core::object::Dict, key: &[u8]) -> Option<Vec<f64>> {
    d.get(key)?
        .as_array()?
        .iter()
        .map(Object::as_number)
        .collect()
}

// -------------------------------------------------------------------------
// The defect: a rotation reaches the artwork
// -------------------------------------------------------------------------

#[test]
fn rotating_a_check_box_changes_its_artwork() {
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
        spec.style = CheckStyle::Check;
        spec
    })
    .unwrap();
    let before = ap_states(&s, "Agree");
    assert_eq!(before.len(), 2, "off and on");

    let out = s.rotate_widget("Agree", 0, 90).unwrap();
    assert!(
        out.appearance_stale.is_none(),
        "pdfcer drew this box, so it can turn it: {:?}",
        out.appearance_stale
    );

    let after = ap_states(&s, "Agree");
    assert_eq!(after.len(), 2);
    for ((name, old), (_, new)) in before.iter().zip(&after) {
        assert_ne!(
            old, new,
            "the {name} state must be redrawn in the turned frame — a \
             byte-identical rewrite is exactly the defect this closes"
        );
    }
}

#[test]
fn the_turned_states_carry_a_matrix_and_a_swapped_bbox() {
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
        spec.style = CheckStyle::Check;
        spec
    })
    .unwrap();

    // Before: identity is the DEFAULT, so no `/Matrix` is written at all —
    // which is what keeps every unrotated button byte-identical (R34).
    for d in ap_dicts(&s, "Agree") {
        assert!(d.get(b"Matrix").is_none(), "no /Matrix while upright");
        assert_eq!(
            numbers(&d, b"BBox").unwrap(),
            vec![0.0, 0.0, 48.0, 24.0],
            "the BBox is the /Rect's own extent"
        );
    }

    s.rotate_widget("Agree", 0, 90).unwrap();

    for d in ap_dicts(&s, "Agree") {
        assert_eq!(
            numbers(&d, b"Matrix").unwrap(),
            vec![0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
            "§8.3.4's counterclockwise quarter turn, no sign flip — /MK /R is \
             counterclockwise too"
        );
        // The swap is what makes §12.5.5 step (b) an identity rather than a
        // squash: an `h x w` BBox turned a quarter bounds to `w x h`, which is
        // exactly `/Rect`. Without it the tick would render turned AND
        // stretched.
        assert_eq!(
            numbers(&d, b"BBox").unwrap(),
            vec![0.0, 0.0, 24.0, 48.0],
            "h x w, not w x h"
        );
    }
}

#[test]
fn rotating_a_radio_button_changes_its_artwork() {
    // A radio's ring and dot are circular, but its BOX is not square — so the
    // turn still changes the geometry it is drawn into.
    let mut s = session();
    s.add_radio_button(&NewRadioButton::new(0, "Choice", rect(), "A").declining_tooltip())
        .unwrap();
    let before = ap_states(&s, "Choice");

    s.rotate_widget("Choice", 0, 90).unwrap();
    let after = ap_states(&s, "Choice");
    assert_ne!(before, after);
    for d in ap_dicts(&s, "Choice") {
        assert_eq!(numbers(&d, b"BBox").unwrap(), vec![0.0, 0.0, 24.0, 48.0]);
    }
}

#[test]
fn rotating_a_push_button_changes_its_plate() {
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();
    let before = ap_states(&s, "Go");
    assert_eq!(before.len(), 1, "a push button has one state");

    let out = s.rotate_widget("Go", 0, 270).unwrap();
    assert!(out.appearance_stale.is_none());

    let after = ap_states(&s, "Go");
    assert_ne!(before[0].1, after[0].1, "the caption turns with the plate");
    let d = &ap_dicts(&s, "Go")[0];
    assert_eq!(
        numbers(d, b"Matrix").unwrap(),
        vec![0.0, -1.0, 1.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(numbers(d, b"BBox").unwrap(), vec![0.0, 0.0, 24.0, 48.0]);
}

#[test]
fn a_half_turn_changes_the_matrix_and_not_one_byte_of_the_stream() {
    // 180° does not swap the box, so the drawing frame is unchanged and the
    // content is byte-identical. The ENTIRE turn is the `/Matrix`.
    //
    // This test was first written asserting the opposite — "the bytes must
    // differ" — on the assumption that a turn always redraws. It failed
    // against a CORRECT fix, which is how the table in the module header got
    // written. Kept in this shape so nobody re-derives it.
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
        spec.style = CheckStyle::Check;
        spec
    })
    .unwrap();
    let before = ap_states(&s, "Agree");

    let out = s.rotate_widget("Agree", 0, 180).unwrap();
    assert!(out.appearance_stale.is_none());

    assert_eq!(
        before,
        ap_states(&s, "Agree"),
        "the frame did not move, so the drawing did not either"
    );
    for d in ap_dicts(&s, "Agree") {
        assert_eq!(
            numbers(&d, b"Matrix").unwrap(),
            vec![-1.0, 0.0, 0.0, -1.0, 0.0, 0.0],
            "and this is the whole of the turn"
        );
        assert_eq!(
            numbers(&d, b"BBox").unwrap(),
            vec![0.0, 0.0, 48.0, 24.0],
            "a half turn keeps w x h"
        );
    }
}

// -------------------------------------------------------------------------
// The ownership test reads the STORED angle, never the staged one
// -------------------------------------------------------------------------

#[test]
fn a_second_rotation_still_recognises_pdfcers_own_artwork() {
    // THE TRAP THE REQUESTER NAMED. `button_ap_plan` asks *"did pdfcer draw
    // these bytes?"* and must therefore compare against the widget's angle
    // **as stored**. Comparing against the angle being staged would declare
    // pdfcer's own artwork foreign on every rotation — failing closed and
    // silently, as a turn that stopped turning.
    //
    // One rotation could pass on a broken build (the stored angle is 0 and so
    // is the artwork's). The SECOND is what catches it.
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
        spec.style = CheckStyle::Check;
        spec
    })
    .unwrap();

    let first = s.rotate_widget("Agree", 0, 90).unwrap();
    assert!(first.appearance_stale.is_none());
    let after_first = ap_states(&s, "Agree");

    let second = s.rotate_widget("Agree", 0, 180).unwrap();
    assert!(
        second.appearance_stale.is_none(),
        "the 90° artwork is still pdfcer's own: {:?}",
        second.appearance_stale
    );
    assert_ne!(after_first, ap_states(&s, "Agree"), "and it turned again");
}

#[test]
fn turning_back_to_zero_restores_the_original_bytes() {
    // The round trip, which pins both halves at once: the turn is applied
    // from the STAGED angle and the ownership test from the STORED one, so
    // 90 -> 0 must land exactly where it started, `/Matrix` removed.
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
        spec.style = CheckStyle::Check;
        spec
    })
    .unwrap();
    let original = ap_states(&s, "Agree");

    s.rotate_widget("Agree", 0, 90).unwrap();
    assert_ne!(original, ap_states(&s, "Agree"));

    s.rotate_widget("Agree", 0, 0).unwrap();
    assert_eq!(
        original,
        ap_states(&s, "Agree"),
        "back to the bytes it was authored with"
    );
    for d in ap_dicts(&s, "Agree") {
        assert!(
            d.get(b"Matrix").is_none(),
            "and the identity is the ABSENCE of /Matrix, not an identity \
             array — emitting one would rewrite every button pdfcer authored"
        );
    }
}

// -------------------------------------------------------------------------
// The box, not the style, decides whether the bytes move
// -------------------------------------------------------------------------

#[test]
fn a_square_box_turns_by_matrix_alone_whatever_the_style() {
    // The requester's warning was about `Circle`, `Square` and `Cross` being
    // rotationally symmetric. True of the SHAPES — and irrelevant to the
    // stream, because the stream is never drawn turned. What decides is the
    // authored box: a square one does not change size under a quarter turn,
    // so the content is identical for EVERY style, symmetric or not.
    //
    // `Check` is deliberately the style here: it is the most asymmetric one
    // available, and it still produces identical bytes. That is what makes
    // this test say something the style-based reading does not.
    let square = Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 44.0,
        ury: 124.0,
    };
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", square).declining_tooltip();
        spec.style = CheckStyle::Check;
        spec
    })
    .unwrap();
    let before = ap_states(&s, "Agree");

    let out = s.rotate_widget("Agree", 0, 90).unwrap();
    assert!(
        out.appearance_stale.is_none(),
        "it redrew; the frame simply had the same size afterwards"
    );
    assert_eq!(
        before,
        ap_states(&s, "Agree"),
        "a 24x24 box turned a quarter is still 24x24"
    );

    // The `/Matrix` is written all the same, because the file's rotation is
    // real even where the drawing cannot show it — and a later RESIZE must
    // find the widget already turned.
    for d in ap_dicts(&s, "Agree") {
        assert_eq!(
            numbers(&d, b"Matrix").unwrap(),
            vec![0.0, 1.0, -1.0, 0.0, 0.0, 0.0]
        );
    }
}

#[test]
fn a_symmetric_style_in_an_oblong_box_does_change() {
    // The control, and it runs the other way from the style-based prediction:
    // `Circle` is the symmetric style the report named, and in an OBLONG box
    // its bytes change — because the authored frame went from 48x24 to 24x48
    // and a circle inscribed in one is not the circle inscribed in the other.
    let mut s = session();
    s.add_check_box(&{
        let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
        spec.style = CheckStyle::Circle;
        spec
    })
    .unwrap();
    let before = ap_states(&s, "Agree");

    s.rotate_widget("Agree", 0, 90).unwrap();
    assert_ne!(
        before,
        ap_states(&s, "Agree"),
        "symmetric style, different frame, different bytes"
    );
}
