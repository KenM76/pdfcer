//! Moving a radio button into another radio group (`Pass 582.0`): one
//! command, the widget keeps its object number, a taken on-state is renamed
//! on the moved member only (§12.7.4.2.3), and an emptied source goes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use std::path::Path;

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms/radio-regroup-form.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn reload(s: &EditSession) -> EditSession {
    let (bytes, _) = s.to_full_bytes(&Default::default()).expect("rewrite");
    EditSession::new(Document::from_bytes(bytes).expect("reload"))
}

fn fields(s: &EditSession) -> Vec<forms::Field> {
    forms::parse_acroform(&s.graph()).unwrap().fields
}

fn field(s: &EditSession, fqn: &str) -> forms::Field {
    fields(s)
        .into_iter()
        .find(|f| f.fully_qualified_name == fqn)
        .unwrap_or_else(|| panic!("no field {fqn}"))
}

fn id(n: u32) -> ObjId {
    ObjId::new(n, 0)
}

fn dict(s: &EditSession, id: ObjId) -> Dict {
    match s.value(id) {
        Some(Object::Dict(d)) => d.clone(),
        other => panic!("{id}: {other:?}"),
    }
}

fn name(d: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    match d.get(key) {
        Some(Object::Name(n)) => Some(n.as_bytes().to_vec()),
        _ => None,
    }
}

fn normal_keys(s: &EditSession, widget: ObjId) -> Vec<Vec<u8>> {
    let g = s.graph();
    let w = dict(s, widget);
    let ap = g.resolve(w.get(b"AP").unwrap()).as_dict().unwrap().clone();
    let n = g.resolve(ap.get(b"N").unwrap()).as_dict().unwrap().clone();
    let mut keys: Vec<_> = n.iter().map(|(k, _)| k.as_bytes().to_vec()).collect();
    keys.sort();
    keys
}

fn widget_ids(s: &EditSession, fqn: &str) -> Vec<ObjId> {
    field(s, fqn).widgets.iter().map(|w| w.id).collect()
}

fn annots(s: &EditSession) -> Vec<ObjId> {
    let page = dict(s, id(3));
    page.get(b"Annots")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .filter_map(Object::as_reference)
        .collect()
}

#[test]
fn a_selected_member_with_a_taken_state_moves_renamed_and_unselected() {
    let mut s = session();
    let depth = s.undo_depth();
    let out = s.move_widget_to_field("Left", 0, "Right").expect("move");
    assert_eq!(s.undo_depth(), depth + 1, "one undo entry");
    assert_eq!(out.widget, id(11), "the object number is kept");
    assert_eq!(out.on_state.as_deref(), Some("Red1"));
    assert!(!out.source_removed);
    assert!(out.disclosures.iter().any(|d| d.contains("\"Red1\"")));
    assert!(out.disclosures.iter().any(|d| d.contains("unselected")));

    let s = reload(&s);
    assert_eq!(widget_ids(&s, "Right"), vec![id(31), id(32), id(11)]);
    assert_eq!(widget_ids(&s, "Left"), vec![id(12)]);
    assert_eq!(name(&dict(&s, id(10)), b"V").as_deref(), Some(&b"Off"[..]));
    assert_eq!(name(&dict(&s, id(11)), b"AS").as_deref(), Some(&b"Off"[..]));
    assert_eq!(
        normal_keys(&s, id(11)),
        vec![b"Off".to_vec(), b"Red1".to_vec()]
    );
    assert_eq!(
        normal_keys(&s, id(31)),
        vec![b"Off".to_vec(), b"Red".to_vec()],
        "the shared /N the staying member uses is untouched"
    );
    assert_eq!(annots(&s).len(), 7, "no page entry added or lost");
}

#[test]
fn a_free_state_moves_unchanged_and_the_source_keeps_its_selection() {
    let mut s = session();
    let out = s.move_widget_to_field("Left", 1, "Right").unwrap();
    assert_eq!(out.on_state.as_deref(), Some("Blue"));
    assert!(out.disclosures.is_empty(), "{:?}", out.disclosures);
    let s = reload(&s);
    assert_eq!(name(&dict(&s, id(10)), b"V").as_deref(), Some(&b"Red"[..]));
    assert_eq!(widget_ids(&s, "Right"), vec![id(31), id(32), id(12)]);
}

#[test]
fn a_merged_last_member_leaves_its_field_behind_as_a_plain_widget() {
    let mut s = session();
    let out = s.move_widget_to_field("Solo", 0, "Right").unwrap();
    assert!(out.source_removed);
    assert_eq!(out.on_state.as_deref(), Some("Yes"));
    let s = reload(&s);
    assert!(fields(&s).iter().all(|f| f.fully_qualified_name != "Solo"));
    assert_eq!(widget_ids(&s, "Right"), vec![id(31), id(32), id(40)]);
    let w = dict(&s, id(40));
    for key in [&b"T"[..], b"FT", b"Ff", b"V"] {
        assert!(w.get(key).is_none(), "field key {key:?} shed");
    }
    assert_eq!(
        name(&w, b"AS").as_deref(),
        Some(&b"Off"[..]),
        "Right's /V is Green"
    );
    assert!(annots(&s).contains(&id(40)));
}

#[test]
fn a_merged_target_is_split_and_keeps_its_place_on_the_page() {
    let mut s = session();
    s.move_widget_to_field("Right", 0, "Solo").unwrap();
    let s = reload(&s);
    let members = widget_ids(&s, "Solo");
    assert_eq!(members.len(), 2);
    assert_eq!(members[1], id(31));
    let promoted = members[0];
    let page = annots(&s);
    assert!(
        !page.contains(&id(40)),
        "the field dict is no longer an annotation"
    );
    assert_eq!(page[4], promoted, "the split widget takes the field's slot");
    assert_eq!(
        normal_keys(&s, promoted),
        vec![b"Off".to_vec(), b"Yes".to_vec()]
    );
}

#[test]
fn an_emptied_grouping_node_goes_with_its_last_field() {
    let mut s = session();
    let out = s.move_widget_to_field("Group.Lone", 0, "Right").unwrap();
    assert!(out.source_removed);
    assert_eq!(out.on_state.as_deref(), Some("Red1"));
    let s = reload(&s);
    let names: Vec<_> = fields(&s)
        .into_iter()
        .map(|f| f.fully_qualified_name)
        .collect();
    assert!(names.iter().all(|n| !n.starts_with("Group")), "{names:?}");
    assert!(s.value(id(60)).is_none(), "the grouping node is deleted");
    assert_eq!(widget_ids(&s, "Right"), vec![id(31), id(32), id(50)]);
}

#[test]
fn undo_restores_the_document_exactly() {
    let mut s = session();
    let (before, _) = s.to_full_bytes(&Default::default()).unwrap();
    s.move_widget_to_field("Left", 0, "Right").unwrap();
    s.undo();
    let (after, _) = s.to_full_bytes(&Default::default()).unwrap();
    assert_eq!(before, after);
}

#[test]
fn refusals_write_nothing() {
    let mut s = session();
    let depth = s.undo_depth();
    let cases: [(&str, usize, &str); 4] = [
        ("Agree", 0, "Right"),
        ("Left", 0, "Agree"),
        ("Left", 0, "Left"),
        ("Left", 5, "Right"),
    ];
    for (from, index, to) in cases {
        let err = s.move_widget_to_field(from, index, to).unwrap_err();
        let fitting = match (from, to) {
            ("Agree", _) | (_, "Agree") => matches!(err, EditError::NotARadioGroup { .. }),
            ("Left", "Left") => matches!(err, EditError::WidgetAlreadyInField { .. }),
            _ => matches!(err, EditError::WidgetIndexOutOfRange { .. }),
        };
        assert!(fitting, "{from}[{index}] -> {to}: {err:?}");
    }
    assert!(matches!(
        s.move_widget_to_field("Nope", 0, "Right"),
        Err(EditError::FieldNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), depth);
}
