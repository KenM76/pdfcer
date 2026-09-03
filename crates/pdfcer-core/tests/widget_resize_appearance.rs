//! **A resized widget's appearance is REDRAWN at the new size, not stretched
//! into it** (`Pass 187.0`).
//!
//! ## What §12.5.5 does, which is the whole reason this file exists
//!
//! A widget's appearance is a form XObject with its own `/BBox`, placed into
//! the annotation's `/Rect` by an algorithm that maps one onto the other.
//! That map is a **scale**. So an appearance drawn for a 12 pt box and left in
//! place while the `/Rect` grows to 40 pt is not merely "the old artwork" — it
//! is the old artwork magnified 3.3×, borders and all. A 1 pt border draws at
//! 3.3 pt and the tick thickens with it.
//!
//! The operator reported the visible half: *"Form shape outlines of checkboxes
//! and such scale when I drag them larger."*
//!
//! ## ★ Three routes were broken, and only one had been reported
//!
//! `pdfcer-gui`'s request named the `/Btn` route
//! (`request_resizing_a_check_box_stretches_its_appearance`, 2026-08-31) and
//! diagnosed it exactly: `regen_after_property_change` returned `Ok(false)`
//! for every field type except Text and Choice, so nothing was rewritten at
//! all. Scoping it turned up two more of the same shape:
//!
//! 1. **`/Btn` was not rebuilt** — the reported one.
//! 2. **★★ Text and Choice WERE rebuilt, at the OLD size.** The regenerator
//!    reads `field.widgets[i].rect`, and that snapshot is taken *before* the
//!    caller stages its `/Rect` write. Measured: a text field dragged from
//!    100×24 to 300×100 came back with `/AP` `/BBox [0 0 100 24]` — a rebuild
//!    that reproduced the defect it existed to prevent. Nobody had looked,
//!    because an empty text field's stretched border reads as a border.
//!    `edit_widget`'s own documentation asserted the opposite.
//! 3. **A push button's caption change never redrew anything.** The caption is
//!    baked into the plate by `build_push_button_appearance`, and
//!    `needs_regen` did not include `edit.caption.is_some()`, so `/MK` `/CA`
//!    changed and the button kept showing its old word.
//!
//! That is the recorded lesson *fixing one route makes the others look
//! broken*, met before the bug reports rather than after: all three land in
//! one Pass.
//!
//! ## What the tests assert on
//!
//! **The `/AP` `/BBox`**, because that is where the defect lives. A model
//! assertion ("the widget was resized") passes on every broken variant — the
//! `/Rect` was always written correctly; it is the artwork that did not
//! follow. `R159`: a defect that lives in the bytes is asserted in the bytes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    EditError, EditSession, NewCheckBox, NewPushButton, NewTextField, ResizeOptions, WidgetEdit,
};
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

fn small() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 32.0,
        ury: 112.0,
    }
}

fn big() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 60.0,
        ury: 140.0,
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

/// Every `/BBox` reachable from this widget's `/AP` `/N`, whether `/N` is a
/// stream (text, choice, push button) or a state sub-dictionary (check box,
/// radio button). Returned as `(width, height)` pairs.
///
/// One reader for both shapes, because a test that only understood one of
/// them would silently assert nothing about the other.
fn ap_boxes(s: &EditSession, name: &str) -> Vec<(f64, f64)> {
    let g = s.graph();
    let field = field_named(s, name);
    let widget = &field.widgets[0];
    let dict = g
        .resolved(widget.id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(ap) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Object::Dict(ap) = ap else {
        return Vec::new();
    };
    let Some(n) = ap.get(b"N") else {
        return Vec::new();
    };
    let mut streams = Vec::new();
    match g.resolve(n).clone() {
        Object::Stream(st) => streams.push(st),
        Object::Dict(states) => {
            for (_, v) in &states.0 {
                if let Object::Stream(st) = g.resolve(v).clone() {
                    streams.push(st);
                }
            }
        }
        _ => {}
    }
    streams
        .into_iter()
        .filter_map(|st| {
            let Some(Object::Array(b)) = st.dict.get(b"BBox").map(|o| g.resolve(o).clone()) else {
                return None;
            };
            let v: Vec<f64> = b.iter().filter_map(Object::as_number).collect();
            (v.len() == 4).then(|| ((v[2] - v[0]).abs(), (v[3] - v[1]).abs()))
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// -------------------------------------------------------------------------
// Route 1 — the reported one: a check box is redrawn, not stretched
// -------------------------------------------------------------------------

#[test]
fn a_resized_check_box_is_redrawn_at_the_new_size() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", small()).declining_tooltip())
        .unwrap();
    let before = ap_boxes(&s, "Agree");
    assert_eq!(before.len(), 2, "a check box has an off and an on state");
    assert!(
        before
            .iter()
            .all(|(w, h)| close(*w, 12.0) && close(*h, 12.0))
    );

    let out = s
        .edit_widget("Agree", 0, &WidgetEdit::new().with_rect(big()))
        .expect("pdfcer drew this artwork, so pdfcer can redraw it");
    assert!(out.resized);
    assert!(
        out.appearance_regenerated,
        "a check box pdfcer drew must be rebuilt, not carried"
    );
    assert!(out.appearance_stale.is_none());

    let after = ap_boxes(&s, "Agree");
    assert_eq!(after.len(), 2, "both states must survive the rebuild");
    for (w, h) in after {
        assert!(
            close(w, 40.0) && close(h, 40.0),
            "the appearance must be drawn at the new 40x40 box, got {w}x{h}"
        );
    }
}

/// The rebuild must not disturb the state dictionary's shape: a check box
/// whose `Off` entry vanished renders as blank paper when unticked, and a
/// check box whose on-state was renamed can no longer be ticked at all.
#[test]
fn the_rebuild_keeps_the_state_names_and_their_object_ids() {
    let mut s = session();
    let mut spec = NewCheckBox::new(0, "Agree", small()).declining_tooltip();
    spec.on_state = "Yes".to_owned();
    s.add_check_box(&spec).unwrap();

    let ids_before = state_ids(&s, "Agree");
    s.edit_widget("Agree", 0, &WidgetEdit::new().with_rect(big()))
        .unwrap();
    let ids_after = state_ids(&s, "Agree");

    assert_eq!(
        ids_before, ids_after,
        "the states are rewritten IN PLACE — same names, same object ids. Allocating fresh \
         objects would need a second whole-dictionary write to the widget, which cannot compose \
         with the /Rect write already staged in this command."
    );
    assert!(ids_after.iter().any(|(n, _)| n == b"Off"));
    assert!(ids_after.iter().any(|(n, _)| n == b"Yes"));
}

fn state_ids(s: &EditSession, name: &str) -> Vec<(Vec<u8>, pdfcer_core::object::ObjId)> {
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
    let Some(Object::Dict(states)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let mut out: Vec<_> = states
        .0
        .iter()
        .filter_map(|(k, v)| v.as_reference().map(|id| (k.0.clone(), id)))
        .collect();
    out.sort();
    out
}

/// A push button pdfcer drew is its own artwork too, and its plate carries the
/// caption — so both the size and the word have to follow.
#[test]
fn a_resized_push_button_is_redrawn_at_the_new_size() {
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", small(), "Go").declining_tooltip())
        .unwrap();
    let out = s
        .edit_widget("Go", 0, &WidgetEdit::new().with_rect(big()))
        .expect("pdfcer drew this plate");
    assert!(out.appearance_regenerated);
    let after = ap_boxes(&s, "Go");
    assert_eq!(
        after.len(),
        1,
        "a push button has one appearance, not states"
    );
    assert!(close(after[0].0, 40.0) && close(after[0].1, 40.0));
}

// -------------------------------------------------------------------------
// Route 2 — the unreported one: text and choice were rebuilt at the OLD size
// -------------------------------------------------------------------------

/// ★★ The regeneration existed and was a no-op with respect to size. This is
/// the assertion that would have caught it, and it is on the `/BBox` rather
/// than on `appearance_regenerated` — which was `true` the whole time.
#[test]
fn a_resized_text_field_is_redrawn_at_the_new_size() {
    let mut s = session();
    s.add_text_field(
        &NewTextField::new(
            0,
            "Customer",
            Rect {
                llx: 20.0,
                lly: 100.0,
                urx: 120.0,
                ury: 124.0,
            },
        )
        .declining_tooltip(),
    )
    .unwrap();

    let out = s
        .edit_widget(
            "Customer",
            0,
            &WidgetEdit::new().with_rect(Rect {
                llx: 20.0,
                lly: 100.0,
                urx: 320.0,
                ury: 200.0,
            }),
        )
        .unwrap();
    assert!(
        out.appearance_regenerated,
        "this was already true before the fix — which is why it caught nothing"
    );

    let boxes = ap_boxes(&s, "Customer");
    assert_eq!(boxes.len(), 1);
    assert!(
        close(boxes[0].0, 300.0) && close(boxes[0].1, 100.0),
        "the rebuilt appearance must use the NEW extent; before Pass 187.0 this was 100x24 \
         and §12.5.5 stretched it 3x by 4.17x. Got {}x{}",
        boxes[0].0,
        boxes[0].1
    );
}

// -------------------------------------------------------------------------
// Route 3 — a caption change must redraw the plate that shows it
// -------------------------------------------------------------------------

#[test]
fn changing_a_push_buttons_caption_redraws_its_plate() {
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", small(), "Send").declining_tooltip())
        .unwrap();
    let before = ap_bytes(&s, "Go");

    let out = s
        .edit_widget("Go", 0, &WidgetEdit::new().with_caption("Cancel"))
        .unwrap();
    assert!(
        out.appearance_regenerated,
        "the caption is drawn INTO the plate, so changing it must redraw"
    );
    assert!(!out.resized, "no geometry changed");

    let after = ap_bytes(&s, "Go");
    assert_ne!(
        before, after,
        "the plate still reads the old word — /MK /CA changed and the artwork did not"
    );
}

fn ap_bytes(s: &EditSession, name: &str) -> Vec<u8> {
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
    let Object::Stream(st) = g.resolve(n).clone() else {
        return Vec::new();
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

// -------------------------------------------------------------------------
// Route 4 — the one found by reading: a Shape-B resize was DISCARDED
// -------------------------------------------------------------------------

/// ★★★ The worst of the family, and the only one nobody reported.
///
/// A field's widget can be the field dictionary itself (**Shape A**, the
/// one-widget case every test uses) or a separate dictionary under a `/Kids`
/// parent (**Shape B**, what a field placed on three pages looks like).
///
/// The appearance regenerator wrote `/AP` by calling `set_widget_ap`, which
/// builds a **whole-dictionary** write from the PRE-command dictionary and
/// pushes it onto the command. `commit` applies writes in order, last one
/// winning — so on Shape B that `/AP` write silently discarded the `/Rect`
/// write `edit_widget` had staged moments earlier.
///
/// Measured before the fix, on this fixture, resizing 140×22 → 380×100:
///
/// ```text
///   /Rect after        140 x 22    <- the resize, thrown away
///   /AP  /BBox         380 x 100   <- the rebuild, applied
///   outcome.resized    true
///   outcome.rect_after Rect { .. 400, 250 }   <- a box never written
/// ```
///
/// So the widget ended up with artwork §12.5.5 must squash into a box a third
/// its size, from a call that returned `Ok` and an outcome that described the
/// resize as done. **Every assertion anyone had written was on the outcome**,
/// which is why 4,861 passing tests said nothing about it — and Shape A takes
/// a different branch that has always folded correctly, so the ordinary
/// one-widget field is fine and always was.
#[test]
fn a_shape_b_widget_keeps_the_resize_that_triggered_its_rebuild() {
    let mut s = EditSession::new(Document::load(&fixture("forms/multi-widget-form.pdf")).unwrap());
    let target = Rect {
        llx: 20.0,
        lly: 150.0,
        urx: 400.0,
        ury: 250.0,
    };
    let out = s
        .edit_widget("Reference", 0, &WidgetEdit::new().with_rect(target))
        .unwrap();
    assert!(out.resized && out.appearance_regenerated);

    let field = field_named(&s, "Reference");
    assert_eq!(
        field.widgets[0].rect,
        Some(target),
        "the /Rect the outcome reports must be the /Rect in the document"
    );

    let boxes = ap_boxes(&s, "Reference");
    assert_eq!(boxes.len(), 1);
    assert!(
        close(boxes[0].0, 380.0) && close(boxes[0].1, 100.0),
        "and the artwork must be drawn for that same box, got {}x{}",
        boxes[0].0,
        boxes[0].1
    );

    // The siblings are untouched — a widget-scope edit is one placement.
    assert_eq!(out.siblings_untouched, 2);
    assert_eq!(
        field.widgets[1].rect,
        Some(Rect {
            llx: 20.0,
            lly: 40.0,
            urx: 160.0,
            ury: 62.0
        }),
        "resizing one placement must not move another"
    );
}

// -------------------------------------------------------------------------
// The ownership boundary — pdfcer redraws only what pdfcer drew
// -------------------------------------------------------------------------

/// ★ The load-bearing refusal. A check box whose artwork somebody else drew
/// must NOT come back as pdfcer's two-line vector tick as a side effect of a
/// drag.
///
/// The fixture is real rather than forged: `forms/demo-form.pdf` carries a
/// hand-authored `Subscribe` check box whose `/AP` `/N` holds
/// `0 0 14 14 re f 4 4 6 6 re f` — a filled square inside a filled square,
/// which pdfcer would never draw. That is what makes this a test of the
/// ownership boundary rather than a test of a mock.
#[test]
fn a_foreign_button_appearance_is_not_redrawn() {
    let mut s = EditSession::new(Document::load(&fixture("forms/demo-form.pdf")).unwrap());
    let before = ap_boxes(&s, "Subscribe");
    assert_eq!(before.len(), 2);

    let target = Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 60.0,
        ury: 130.0,
    };
    let err = s
        .edit_widget("Subscribe", 0, &WidgetEdit::new().with_rect(target))
        .expect_err("resizing foreign artwork must refuse by default");
    assert!(
        matches!(err, EditError::ResizeAppearanceNotRebuildable { .. }),
        "got {err:?}"
    );
    assert_eq!(
        before,
        ap_boxes(&s, "Subscribe"),
        "a refusal must leave the session untouched (rule 4)"
    );

    // ...and the escape hatch proceeds, with the distortion stated.
    let out = s
        .edit_widget(
            "Subscribe",
            0,
            &WidgetEdit::new()
                .with_rect(target)
                .with_resize(ResizeOptions::new().with_allow_appearance_distortion(true)),
        )
        .expect("the operator accepted the distortion");
    assert!(
        !out.appearance_regenerated,
        "somebody else's artwork must not be replaced with pdfcer's rendering of it"
    );
    assert!(
        out.appearance_stale
            .as_deref()
            .is_some_and(|m| m.contains("pdfcer did not draw") && m.contains("NON-UNIFORM")),
        "the disclosure must say whose artwork it is AND that this scale distorts: {:?}",
        out.appearance_stale
    );
    assert_eq!(
        before,
        ap_boxes(&s, "Subscribe"),
        "the foreign streams are carried, not rewritten"
    );
}

// -------------------------------------------------------------------------
// The three operator answers now reach a form field
// -------------------------------------------------------------------------

/// `/BS` `/W` follows the geometry only when the operator said so. The default
/// leaves it alone, which is Inkscape's and Illustrator's default too.
#[test]
fn the_stroke_width_answer_reaches_a_widget() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "A", small()).declining_tooltip())
        .unwrap();
    let out = s
        .edit_widget("A", 0, &WidgetEdit::new().with_rect(big()))
        .unwrap();
    assert!(
        out.stroke_width.is_none(),
        "default is OFF, and 'left at 1.0 pt' is a fact worth reporting"
    );

    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "A", small()).declining_tooltip())
        .unwrap();
    let out = s
        .edit_widget(
            "A",
            0,
            &WidgetEdit::new()
                .with_rect(big())
                .with_resize(ResizeOptions::new().with_scale_stroke_width(true)),
        )
        .unwrap();
    let (before, after) = out.stroke_width.expect("scaled");
    assert!(
        close(after, before * (40.0 / 12.0)),
        "a uniform 40/12 scale must carry the border width with it: {before} -> {after}"
    );
}

/// A pure translation is not a resize: nothing is rebuilt and nothing is
/// scaled. Guarding the cheap case explicitly, because the whole Pass pushes
/// in the direction of rebuilding more often.
#[test]
fn a_pure_translation_still_rebuilds_nothing() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "A", small()).declining_tooltip())
        .unwrap();
    let before = ap_boxes(&s, "A");
    let out = s
        .edit_widget(
            "A",
            0,
            &WidgetEdit::new().with_rect(Rect {
                llx: 200.0,
                lly: 300.0,
                urx: 212.0,
                ury: 312.0,
            }),
        )
        .unwrap();
    assert!(!out.resized, "same extent, different place");
    assert!(!out.appearance_regenerated);
    assert!(out.stroke_width.is_none());
    assert_eq!(before, ap_boxes(&s, "A"), "the artwork must be untouched");
}
