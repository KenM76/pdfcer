//! Several form fields copied and pasted as one (`Pass 587.0`): one shared
//! closure, one all-or-nothing undo entry, names and radio states chosen by
//! the verb, calculations re-pointed at the copies.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::formclip::{FieldClipSet, FieldSetPastePolicy, PasteTooltip};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::vector::ClipError;
use std::path::Path;

fn session(rel: &str) -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms")
        .join(rel);
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn reload(s: &EditSession) -> EditSession {
    let (bytes, _) = s.to_full_bytes(&Default::default()).expect("rewrite");
    EditSession::new(Document::from_bytes(bytes).expect("reload"))
}

fn new_fields(copy_actions: bool) -> FieldSetPastePolicy {
    FieldSetPastePolicy::NewFields {
        tooltip: PasteTooltip::Declined,
        copy_value: false,
        copy_actions,
    }
}

fn field(s: &EditSession, fqn: &str) -> Option<forms::Field> {
    forms::parse_acroform(&s.graph())?
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == fqn)
}

fn dr_fonts(s: &EditSession) -> Dict {
    let g = s.graph();
    let catalog = g.catalog_dict().unwrap();
    let acroform = g
        .resolve(catalog.get(b"AcroForm").unwrap())
        .as_dict()
        .unwrap();
    let dr = g.resolve(acroform.get(b"DR").unwrap()).as_dict().unwrap();
    g.resolve(dr.get(b"Font").unwrap())
        .as_dict()
        .unwrap()
        .clone()
}

/// The calculate script on field `id`, decoded.
fn calculate_js(s: &EditSession, id: ObjId) -> String {
    let g = s.graph();
    let Some(Object::Dict(f)) = s.value(id) else {
        panic!("field {id} missing");
    };
    let aa = g.resolve(f.get(b"AA").unwrap()).as_dict().unwrap();
    let c = g.resolve(aa.get(b"C").unwrap()).as_dict().unwrap();
    match g.resolve(c.get(b"JS").unwrap()) {
        Object::String(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => panic!("unexpected /JS {other:?}"),
    }
}

#[test]
fn a_set_round_trips_through_bytes_and_shares_its_closure() {
    let s = session("field-set-form.pdf");
    let set = s.copy_fields(&["Qty", "Total", "Pick", "Qty"]).unwrap();
    assert_eq!(set.source_names(), ["Qty", "Total", "Pick"], "deduplicated");
    assert_eq!(set.widget_count(), 4);
    let separate: usize = ["Qty", "Total", "Pick"]
        .iter()
        .map(|f| s.copy_field(f).unwrap().object_count())
        .sum();
    assert!(
        set.object_count() < separate,
        "the shared /TB font travels once: {} vs {separate}",
        set.object_count()
    );

    let back = FieldClipSet::from_bytes(&set.to_bytes()).unwrap();
    assert_eq!(back, set);
    let single = s.copy_field("Qty").unwrap().to_bytes();
    assert!(matches!(
        FieldClipSet::from_bytes(&single),
        Err(ClipError::NotAClip)
    ));
    let mut cut = set.to_bytes();
    cut.truncate(cut.len() - 3);
    assert!(FieldClipSet::from_bytes(&cut).is_err());
}

#[test]
fn new_fields_get_distinct_free_names_in_one_undo_entry() {
    let mut s = session("field-set-form.pdf");
    let set = s.copy_fields(&["Qty", "Total", "Pick"]).unwrap();
    let depth = s.undo_depth();
    let out = s
        .paste_fields(&set, 0, (200.0, 100.0), &new_fields(false))
        .unwrap();
    assert_eq!(out.names, ["Qty_2", "Total_2", "Pick_2"]);
    assert_eq!(out.fields.len(), 3);
    assert!(out.fields.iter().all(|f| f.disclosures.is_empty()));
    assert!(out.disclosures.iter().any(|d| d.contains("\"Qty_2\"")));
    let mut unique = out.disclosures.clone();
    unique.dedup();
    assert_eq!(unique.len(), out.disclosures.len(), "each sentence once");
    assert_eq!(s.undo_depth(), depth + 1, "one undo entry");

    let r = reload(&s);
    let qty = field(&r, "Qty_2").expect("Qty_2 exists");
    let total = field(&r, "Total_2").expect("Total_2 exists");
    let pick = field(&r, "Pick_2").expect("Pick_2 exists");
    assert_eq!(pick.widgets.len(), 2);
    // Layout kept: the selection's lower-left (Total at 20,200) lands on the
    // origin, and Qty stays 40pt above Total.
    let q = qty.widgets[0].rect.unwrap();
    let t = total.widgets[0].rect.unwrap();
    assert!((t.llx - 200.0).abs() < 1e-6 && (t.lly - 100.0).abs() < 1e-6);
    assert!((q.lly - t.lly - 40.0).abs() < 1e-6);

    s.undo();
    assert!(field(&s, "Qty_2").is_none() && field(&s, "Pick_2").is_none());
}

#[test]
fn a_calculation_naming_a_sibling_names_the_pasted_sibling() {
    let mut s = session("field-set-form.pdf");
    let set = s.copy_fields(&["Qty", "Total"]).unwrap();
    let out = s
        .paste_fields(&set, 0, (200.0, 100.0), &new_fields(true))
        .unwrap();
    let js = calculate_js(&s, out.fields[1].field_id);
    assert!(js.contains("\"Qty_2\""), "{js}");
    assert!(!js.contains("\"Qty\""), "{js}");
    assert!(out.disclosures.iter().any(|d| d.contains("calculation")));
    // The source keeps its own script.
    let original = field(&s, "Total").unwrap();
    assert!(calculate_js(&s, original.widgets[0].id).contains("\"Qty\""));
}

#[test]
fn a_calculation_naming_a_field_outside_the_batch_is_left_alone() {
    let mut s = session("field-set-form.pdf");
    let set = s.copy_fields(&["Total"]).unwrap();
    let out = s
        .paste_fields(&set, 0, (200.0, 100.0), &new_fields(true))
        .unwrap();
    assert!(calculate_js(&s, out.fields[0].field_id).contains("\"Qty\""));
}

#[test]
fn two_radio_members_pasted_as_widgets_get_distinct_states() {
    let mut s = session("field-set-form.pdf");
    let set = s.copy_fields(&["Pick"]).unwrap();
    let depth = s.undo_depth();
    let out = s
        .paste_fields(
            &set,
            0,
            (200.0, 240.0),
            &FieldSetPastePolicy::AdditionalWidgets,
        )
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    let states: Vec<_> = out
        .fields
        .iter()
        .map(|f| f.radio_on_state.clone().unwrap())
        .collect();
    assert_eq!(states.len(), 2);
    assert_ne!(states[0], states[1]);
    assert!(
        !out.disclosures.iter().any(|d| d.contains("ONE was added")),
        "every widget was added: {:?}",
        out.disclosures
    );

    let r = reload(&s);
    let widgets = field(&r, "Pick").unwrap().widgets;
    assert_eq!(widgets.len(), 4);
    let mut on: Vec<_> = widgets.iter().flat_map(|w| w.on_states.clone()).collect();
    let n = on.len();
    on.sort();
    on.dedup();
    assert_eq!(on.len(), n, "every member's on-state is distinct: {on:?}");
}

#[test]
fn a_refusal_mid_batch_writes_nothing() {
    let source = session("field-set-form.pdf");
    let mut s = session("rival-font-form.pdf");
    // Qty exists here, Total does not: the first step succeeds, the second
    // refuses.
    let qty = source.copy_fields(&["Qty"]).unwrap();
    s.paste_fields(&qty, 0, (20.0, 20.0), &new_fields(false))
        .unwrap();
    let depth = s.undo_depth();
    let both = source.copy_fields(&["Qty", "Total"]).unwrap();
    let err = s
        .paste_fields(
            &both,
            0,
            (20.0, 60.0),
            &FieldSetPastePolicy::AdditionalWidgets,
        )
        .unwrap_err();
    assert!(matches!(err, EditError::FieldNotFound { ref name } if name == "Total"));
    assert_eq!(s.undo_depth(), depth, "undo depth unchanged");
    assert_eq!(field(&s, "Qty").unwrap().widgets.len(), 1);
}

#[test]
fn a_font_two_fields_share_is_installed_once() {
    let source = session("field-set-form.pdf");
    let mut s = session("rival-font-form.pdf");
    let before = dr_fonts(&s).len();
    let set = source.copy_fields(&["Qty", "Total"]).unwrap();
    s.paste_fields(&set, 0, (20.0, 20.0), &new_fields(false))
        .unwrap();

    let r = reload(&s);
    let fonts = dr_fonts(&r);
    assert_eq!(fonts.len(), before + 1, "one new /DR font: {fonts:?}");
    // The destination's own /TB (Courier) is not the carried Helvetica-Bold,
    // so the copy is installed under a free name both fields' /DA use.
    let qty = field(&r, "Qty").unwrap().default_appearance.unwrap();
    let total = field(&r, "Total").unwrap().default_appearance.unwrap();
    assert_eq!(qty, total);
    assert!(
        !qty.starts_with(b"/TB "),
        "{}",
        String::from_utf8_lossy(&qty)
    );
}
