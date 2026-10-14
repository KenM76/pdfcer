//! Pasting another member into a radio group (`Pass 581.0`): the copied
//! on-state is a member's by construction, so the new member gets a fresh
//! one and its appearance keys are renamed to it (§12.7.4.2.3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::formclip::FieldPastePolicy;
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use std::path::Path;

fn session(rel: &str) -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel);
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn reload(s: &EditSession) -> EditSession {
    let (bytes, _) = s.to_full_bytes(&Default::default()).expect("rewrite");
    EditSession::new(Document::from_bytes(bytes).expect("reload"))
}

fn rect() -> Rect {
    Rect::from_corners(200.0, 40.0, 212.0, 52.0)
}

fn add_member(
    s: &mut EditSession,
    fqn: &str,
) -> Result<pdfcer_core::formclip::FieldPasteOutcome, EditError> {
    let clip = s.copy_field(fqn).expect("copy");
    s.paste_field(
        &clip,
        0,
        rect(),
        &FieldPastePolicy::AdditionalWidget {
            existing: fqn.to_owned(),
        },
    )
}

fn field(s: &EditSession, fqn: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == fqn)
        .unwrap()
}

/// The keys of `/AP /<which>` on widget `id`, resolved through the graph.
fn ap_keys(s: &EditSession, id: ObjId, which: &[u8]) -> Vec<Vec<u8>> {
    let g = s.graph();
    let Some(Object::Dict(w)) = s.value(id) else {
        panic!("widget {id} missing");
    };
    let ap: &Dict = g.resolve(w.get(b"AP").unwrap()).as_dict().unwrap();
    let sub = g.resolve(ap.get(which).unwrap()).as_dict().unwrap();
    let mut keys: Vec<_> = sub.iter().map(|(k, _)| k.as_bytes().to_vec()).collect();
    keys.sort();
    keys
}

fn appearance_state(s: &EditSession, id: ObjId) -> Option<Vec<u8>> {
    let Some(Object::Dict(w)) = s.value(id) else {
        return None;
    };
    match w.get(b"AS") {
        Some(Object::Name(n)) => Some(n.as_bytes().to_vec()),
        _ => None,
    }
}

#[test]
fn a_pasted_member_takes_a_fresh_state_and_selects_alone() {
    let mut s = session("forms/radio-group-form.pdf");
    let before = field(&s, "Priority").widgets.len();
    let depth = s.undo_depth();

    let first = add_member(&mut s, "Priority").expect("a radio paste succeeds");
    assert_eq!(first.radio_on_state.as_deref(), Some("Red1"));
    assert!(first.disclosures.iter().any(|d| d.contains("\"Red1\"")));
    let second = add_member(&mut s, "Priority").unwrap();
    assert_eq!(
        second.radio_on_state.as_deref(),
        Some("Red2"),
        "Red1 is now taken"
    );
    assert_eq!(s.undo_depth(), depth + 2, "one entry per paste");

    let s = reload(&s);
    let widgets = field(&s, "Priority").widgets;
    assert_eq!(widgets.len(), before + 2);
    let mut states: Vec<_> = widgets.iter().flat_map(|w| w.on_states.clone()).collect();
    let n = states.len();
    states.sort();
    states.dedup();
    assert_eq!(
        states.len(),
        n,
        "every member's on-state is distinct: {states:?}"
    );
    let new_id = first.widget_ids[0];
    assert_eq!(
        ap_keys(&s, new_id, b"N"),
        vec![b"Off".to_vec(), b"Red1".to_vec()]
    );
    assert_ne!(appearance_state(&s, new_id).as_deref(), Some(&b"Red"[..]));
}

#[test]
fn an_indirect_normal_and_an_inline_down_appearance_are_both_renamed() {
    let mut s = session("forms/radio-opt-form.pdf");
    let pasted = add_member(&mut s, "Shade").unwrap();
    assert_eq!(pasted.radio_on_state.as_deref(), Some("Dark1"));
    let s = reload(&s);
    let id = pasted.widget_ids[0];
    assert_eq!(
        ap_keys(&s, id, b"N"),
        vec![b"Dark1".to_vec(), b"Off".to_vec()]
    );
    assert_eq!(
        ap_keys(&s, id, b"D"),
        vec![b"Dark1".to_vec(), b"Off".to_vec()]
    );
    assert_eq!(
        appearance_state(&s, id).as_deref(),
        Some(&b"Off"[..]),
        "starts unselected"
    );
    let original = field(&s, "Shade").widgets[0].id;
    assert_eq!(
        ap_keys(&s, original, b"N"),
        vec![b"Dark".to_vec(), b"Off".to_vec()],
        "the shared /N the source member uses is untouched",
    );
}

#[test]
fn a_group_with_an_opt_array_still_refuses_a_duplicate() {
    let mut s = session("forms/radio-opt-form.pdf");
    let depth = s.undo_depth();
    let err = add_member(&mut s, "Size").unwrap_err();
    assert!(
        matches!(err, EditError::RadioExportValueTaken { .. }),
        "{err:?}"
    );
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn a_new_field_paste_reports_no_radio_state() {
    let mut s = session("forms/radio-group-form.pdf");
    let clip = s.copy_field("Priority").unwrap();
    let out = s
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::NewField {
                name: "Priority2".to_owned(),
                tooltip: pdfcer_core::formclip::PasteTooltip::Declined,
                copy_value: false,
                copy_actions: false,
            },
        )
        .unwrap();
    assert_eq!(out.radio_on_state, None);
}
