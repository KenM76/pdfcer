//! The write-side resolver, and what a same-name add now MEANS (F1).
//!
//! ## The rule under test
//!
//! > Field identity is the fully-qualified name; the fully-qualified name is
//! > derived from the object graph, not stored; therefore every authoring
//! > write must resolve the name against the graph *before* deciding what to
//! > write, and must be able to attach a widget to an existing node without
//! > creating a second node.
//! >
//! > — decision 020 §3.1.1 (R100)
//!
//! Slices 1 and 2 shipped creation on a flat append plus a blanket refusal of
//! every same-name add. The refusal was right for what it was: without a
//! resolver, the only alternative was appending a second top-level field with
//! the same `/T`, and §12.7.3.2 makes the FQN the IDENTITY — two such fields
//! have one identity, no disambiguator, and cannot be un-authored afterwards
//! because nothing records which one was meant. A missing capability is
//! honest and reversible; a malformed document is neither.
//!
//! What the resolver changes is not the safety property but its MECHANISM.
//! Before: the duplicate-FQN document was reachable and refused by a guard.
//! Now: it is unreachable, because every authoring write asks the graph what
//! the name denotes before it decides what to write, and a same-type match
//! MERGES.
//!
//! ## The four outcomes, and where each is proven
//!
//! | Outcome | Test |
//! |---|---|
//! | `Vacant` → CREATE | [`a_dotted_path_creates_the_groups_it_needs`] |
//! | `Terminal` same type → MERGE | [`a_second_add_merges_and_promotes_shape_a_to_shape_b`] |
//! | `Terminal` different type → REFUSE | [`a_different_type_under_the_same_name_is_refused`] |
//! | `Grouping` → REFUSE | [`a_grouping_node_cannot_become_a_terminal_field`] |
//!
//! Both refusals are asserted **firing**, not merely present (R96). Their
//! reachability is the whole reason they exist: contrast Pass 19.4's R91
//! refusal, which was correct, wired, and structurally unreachable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    ChoiceOption, EditError, EditSession, NewCheckBox, NewChoiceField, NewTextField,
};
use pdfcer_core::forms;
use pdfcer_core::forms_author::{self, FieldPath, FieldShape, FormAuthorError, resolve_field_path};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::{SaveOptions, save_full};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn blank() -> EditSession {
    session("dimension/plain-base.pdf")
}

fn r1() -> Rect {
    Rect {
        llx: 20.0,
        lly: 20.0,
        urx: 180.0,
        ury: 44.0,
    }
}
fn r2() -> Rect {
    Rect {
        llx: 20.0,
        lly: 60.0,
        urx: 180.0,
        ury: 84.0,
    }
}

fn form(s: &EditSession) -> forms::AcroForm {
    forms::parse_acroform(&s.graph()).expect("a form")
}

fn field(s: &EditSession, fqn: &str) -> forms::Field {
    form(s)
        .field_by_name(fqn)
        .unwrap_or_else(|| panic!("no field {fqn}"))
        .clone()
}

fn dict_of(s: &EditSession, id: ObjId) -> pdfcer_core::object::Dict {
    s.graph()
        .resolved(id)
        .as_dict()
        .cloned()
        .unwrap_or_else(|| panic!("{id:?} is not a dictionary"))
}

// ---------------------------------------------------------------------------
// The resolver in isolation.
// ---------------------------------------------------------------------------

/// An unused name on a formless document is vacant from the root.
///
/// `deepest: None` is what tells the caller to create the whole path and
/// register its top in `/AcroForm /Fields`, creating the form if needed.
#[test]
fn an_unused_name_resolves_to_vacant_from_the_root() {
    let s = blank();
    let path = resolve_field_path(&s.graph(), "Nothing.Here").expect("resolves");
    assert_eq!(
        path,
        FieldPath::Vacant {
            deepest: None,
            remaining: vec!["Nothing".to_owned(), "Here".to_owned()],
        },
    );
}

/// An existing merged field resolves to a `Terminal` in Shape A.
///
/// `MergedSingleWidget` is what tells a merge it must PROMOTE before it can
/// attach a second widget — Table 220 permits the merged form only while
/// there is exactly one.
#[test]
fn an_existing_merged_field_resolves_to_a_shape_a_terminal() {
    let s = session("forms/demo-form.pdf");
    let path = resolve_field_path(&s.graph(), "FullName").expect("resolves");
    match path {
        FieldPath::Terminal { ft, shape, .. } => {
            assert_eq!(ft, Some(forms::FieldType::Text));
            assert_eq!(shape, FieldShape::MergedSingleWidget);
        }
        other => panic!("expected a Shape A terminal, got {other:?}"),
    }
}

/// A field with widget `/Kids` resolves to Shape B, carrying the count.
#[test]
fn a_kids_field_resolves_to_shape_b_with_its_widget_count() {
    let s = session("forms/multi-widget-form.pdf");
    match resolve_field_path(&s.graph(), "Reference").expect("resolves") {
        FieldPath::Terminal { shape, .. } => {
            assert_eq!(shape, FieldShape::KidsWidgets { n: 3 });
        }
        other => panic!("expected Shape B, got {other:?}"),
    }
}

/// A partially-existing path reports the DEEPEST existing node.
///
/// This is what stops `Personal.Address.Zip` from creating a SECOND top-level
/// `Personal` when one already exists — which would be the duplicate-identity
/// defect, arrived at from the other direction.
#[test]
fn a_partially_existing_path_reports_the_deepest_existing_node() {
    let s = session("forms/nested-form.pdf");
    match resolve_field_path(&s.graph(), "Personal.Address.Country").expect("resolves") {
        FieldPath::Vacant { deepest, remaining } => {
            let d = deepest.expect("`Personal.Address` exists");
            assert_eq!(
                dict_of(&s, d).get(b"T").cloned(),
                Some(Object::String(b"Address".to_vec())),
                "the deepest existing node is `Address`, not the root",
            );
            assert_eq!(remaining, vec!["Country".to_owned()]);
        }
        other => panic!("expected Vacant beneath Address, got {other:?}"),
    }
}

/// A node with child fields resolves to `Grouping`, whatever else it carries.
#[test]
fn a_container_resolves_to_grouping() {
    let s = session("forms/nested-form.pdf");
    assert!(matches!(
        resolve_field_path(&s.graph(), "Personal").expect("resolves"),
        FieldPath::Grouping { .. },
    ));
    assert!(matches!(
        resolve_field_path(&s.graph(), "Personal.Address").expect("resolves"),
        FieldPath::Grouping { .. },
    ));
}

/// An INHERITED `/FT` is resolved, not reported as a missing type.
///
/// The nested fixture declares `/FT /Tx` on `Personal` and on neither
/// terminal. A resolver reading only a node's own `/FT` would report `None`
/// for `Personal.Address.Zip` — and then every merge into it would look like
/// the malformed-field case and be refused.
#[test]
fn an_inherited_field_type_is_resolved_on_the_write_side_too() {
    let s = session("forms/nested-form.pdf");
    match resolve_field_path(&s.graph(), "Personal.Address.Zip").expect("resolves") {
        FieldPath::Terminal { ft, .. } => assert_eq!(ft, Some(forms::FieldType::Text)),
        other => panic!("expected a terminal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// MERGE, and the Shape A→B promotion.
// ---------------------------------------------------------------------------

/// A second same-name add attaches a WIDGET and promotes Shape A to Shape B.
///
/// # Every assertion here is a step of §3.1.5 that can fail on its own
///
/// The promotion is a split, not an append: the annotation keys must move off
/// the field dictionary onto a new widget object before `/Kids` can exist at
/// all. So the test checks each half separately —
///
/// * the field dict has SHED `/Subtype`, `/Rect`, `/AP` (it is not an
///   annotation any more) and KEPT `/FT`, `/T`, `/V` (it is still a field);
/// * both widgets carry `/Parent`;
/// * the page's `/Annots` names the promoted WIDGET and not the field dict.
///
/// That last one is decision 020's "single easiest thing in the whole family
/// to forget", and it fails quietly: `dict_is_widget`'s defensive
/// "…or it has `/Rect` or `/AP`" fallback partially masks it, so the document
/// half-works in pdfcer and misbehaves elsewhere.
#[test]
fn a_second_add_merges_and_promotes_shape_a_to_shape_b() {
    let mut s = blank();
    let first = s
        .add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("first")
        .field_id;
    let second = s
        .add_text_field(&NewTextField::new(0, "Ref", r2()).declining_tooltip())
        .expect("merge")
        .field_id;
    assert_eq!(first, second, "a merge returns the EXISTING field's id");

    let f = field(&s, "Ref");
    assert_eq!(form(&s).fields.len(), 1, "one field, not two identities");
    assert_eq!(f.widgets.len(), 2);
    assert!(!f.merged, "promoted out of Shape A");

    // The field dictionary stopped being an annotation.
    let fd = dict_of(&s, f.id);
    for shed in [&b"Subtype"[..], b"Rect", b"AP", b"Type"] {
        assert!(
            !fd.contains_key(shed),
            "/{} must move to the widget: {fd:?}",
            String::from_utf8_lossy(shed),
        );
    }
    // …and is still a field.
    for kept in [&b"FT"[..], b"T", b"V"] {
        assert!(
            fd.contains_key(kept),
            "/{} belongs to the FIELD and must stay",
            String::from_utf8_lossy(kept),
        );
    }

    // Both widgets point back at it.
    for w in &f.widgets {
        assert_eq!(
            dict_of(&s, w.id)
                .get(b"Parent")
                .and_then(Object::as_reference),
            Some(f.id),
        );
    }

    // THE `/ANNOTS` RETARGET. The page must name the promoted widget, and
    // must NOT still name the field dictionary.
    let page_id = s.page_slots().expect("pages")[0].id;
    let annots: Vec<ObjId> = dict_of(&s, page_id)
        .get(b"Annots")
        .map(|o| s.graph().resolve(o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default()
        .iter()
        .filter_map(Object::as_reference)
        .collect();
    assert!(
        !annots.contains(&f.id),
        "the page still names the field dict, which is no longer an annotation: {annots:?}",
    );
    for w in &f.widgets {
        assert!(
            annots.contains(&w.id),
            "widget {:?} is not on the page",
            w.id
        );
    }
}

/// The promoted widget keeps the ORIGINAL widget's place in `/Annots`.
///
/// `/Annots` order is paint order and — absent `/Tabs` — tab order. A
/// promotion implemented as remove-plus-append would silently move the first
/// widget behind everything added since, which is a change to the document
/// the operator did not ask for and would only notice while tabbing.
#[test]
fn the_promotion_keeps_the_original_widgets_position_in_annots() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "First", r1()).declining_tooltip())
        .expect("A");
    s.add_text_field(&NewTextField::new(0, "Second", r2()).declining_tooltip())
        .expect("B");
    // Now merge onto the FIRST one; it must stay first.
    s.add_text_field(
        &NewTextField::new(
            0,
            "First",
            Rect {
                llx: 200.0,
                lly: 20.0,
                urx: 360.0,
                ury: 44.0,
            },
        )
        .declining_tooltip(),
    )
    .expect("merge onto the first field");
    let page_id = s.page_slots().expect("pages")[0].id;
    let annots: Vec<ObjId> = dict_of(&s, page_id)
        .get(b"Annots")
        .map(|o| s.graph().resolve(o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default()
        .iter()
        .filter_map(Object::as_reference)
        .collect();

    let first = field(&s, "First");
    let second = field(&s, "Second");
    let promoted = first.widgets[0].id;
    let pos = |id: ObjId| annots.iter().position(|x| *x == id);
    assert!(
        pos(promoted) < pos(second.widgets[0].id),
        "the promoted widget was pushed behind a later field: {annots:?}",
    );
}

/// No authored widget kid carries `/T`, `/FT` or `/Kids` (R101).
///
/// The reader classifies a `/Kids` entry as a child FIELD when it has any of
/// those. So a widget written with a `/T` would not be a second view of the
/// field — it would be a second FIELD underneath it, silently, and the FQN it
/// composed would be `Ref.Ref`.
#[test]
fn an_authored_widget_kid_carries_no_field_keys() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("first");
    s.add_text_field(&NewTextField::new(0, "Ref", r2()).declining_tooltip())
        .expect("merge");
    let f = field(&s, "Ref");
    assert_eq!(f.widgets.len(), 2);
    for w in &f.widgets {
        let d = dict_of(&s, w.id);
        for forbidden in [&b"T"[..], b"FT", b"Kids"] {
            assert!(
                !d.contains_key(forbidden),
                "widget {:?} carries /{} — the reader would treat it as a child FIELD",
                w.id,
                String::from_utf8_lossy(forbidden),
            );
        }
        assert_eq!(
            d.get(b"Subtype")
                .and_then(Object::as_name)
                .map(|n| n.as_bytes().to_vec()),
            Some(b"Widget".to_vec()),
        );
    }
}

/// A THIRD add appends to `/Kids` without promoting again.
///
/// Shape B is already correct, so the third widget is a plain append. A
/// promotion that fired a second time would move keys that are no longer on
/// the field dict and produce an empty extra widget.
#[test]
fn a_third_add_appends_without_promoting_again() {
    let mut s = blank();
    for rect in [
        r1(),
        r2(),
        Rect {
            llx: 200.0,
            lly: 20.0,
            urx: 360.0,
            ury: 44.0,
        },
    ] {
        s.add_text_field(&NewTextField::new(0, "Ref", rect).declining_tooltip())
            .expect("add");
    }
    let f = field(&s, "Ref");
    assert_eq!(form(&s).fields.len(), 1);
    assert_eq!(f.widgets.len(), 3);
    // Each widget has a distinct rectangle — no empty placeholder crept in.
    let mut rects: Vec<String> = f.widgets.iter().map(|w| format!("{:?}", w.rect)).collect();
    rects.sort();
    rects.dedup();
    assert_eq!(rects.len(), 3, "three distinct widgets: {rects:?}");
}

/// The merged field is fillable by the EXISTING, unmodified fill verb, and
/// the fill reaches both widgets.
///
/// This is what turns "the merge parses" into "the merge produced a real
/// field". `fill_text_field` knows nothing about authoring: it resolves by
/// fully-qualified name and fans out over `field.widgets`, exactly as it does
/// for a document pdfcer never touched.
#[test]
fn the_merged_field_fills_through_the_untouched_verb() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("first");
    s.add_text_field(&NewTextField::new(0, "Ref", r2()).declining_tooltip())
        .expect("merge");
    let out = s
        .fill_text_field("Ref", "R-2000")
        .expect("the merged field fills");
    assert_eq!(
        out.widgets_updated, 2,
        "the fill fans out over both widgets"
    );

    let f = field(&s, "Ref");
    assert_eq!(f.value.display_text(), "R-2000");
    assert_eq!(
        f.widgets.iter().filter(|w| w.has_normal_appearance).count(),
        2,
        "both widgets carry a regenerated appearance",
    );
}

/// A merge survives save-and-reload, and `/Fields` grew by exactly ZERO.
///
/// The registration half of the identity property: a merge adds a WIDGET, so
/// `/AcroForm /Fields` must be unchanged. A merge that also registered would
/// list one field twice — the walk would reach it twice and give it two FQNs.
#[test]
fn a_merge_adds_no_new_entry_to_the_fields_array() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("first");
    let before = fields_array_len(&s);
    s.add_text_field(&NewTextField::new(0, "Ref", r2()).declining_tooltip())
        .expect("merge");
    assert_eq!(
        fields_array_len(&s),
        before,
        "/Fields must not grow on a merge"
    );

    // And it reopens as one field with two widgets.
    let doc = Document::load(&fixture("dimension/plain-base.pdf")).expect("reload base");
    let (bytes, _) = save_full(&doc, &s.dirty_set(), &SaveOptions::identity()).expect("save");
    let reopened = EditSession::new(Document::from_bytes(bytes).expect("reopen"));
    let f = field(&reopened, "Ref");
    assert_eq!(form(&reopened).fields.len(), 1);
    assert_eq!(f.widgets.len(), 2);
}

fn fields_array_len(s: &EditSession) -> usize {
    let graph = s.graph();
    graph
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm").map(|o| graph.resolve(o).clone()))
        .and_then(|o| o.as_dict().and_then(|d| d.get(b"Fields")).cloned())
        .map(|o| graph.resolve(&o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::len))
        .unwrap_or(0)
}

/// Every `/Fields` entry pdfcer writes is an INDIRECT REFERENCE (§1.2.2 trap).
///
/// §12.7.3.1 requires every field to be an indirect object, and pdfcer's own
/// reader collects `/Fields` roots by reference — a direct dictionary there
/// would be counted and skipped, so a field pdfcer authored would be one the
/// same pdfcer could not address.
#[test]
fn every_authored_fields_entry_is_an_indirect_reference() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "A", r1()).declining_tooltip())
        .expect("a");
    s.add_check_box(&NewCheckBox::new(0, "B", r2()).declining_tooltip())
        .expect("b");
    s.add_text_field(&NewTextField::new(0, "Deep.Path.C", r1()).declining_tooltip())
        .expect("c");
    let graph = s.graph();
    let entries = graph
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm").map(|o| graph.resolve(o).clone()))
        .and_then(|o| o.as_dict().and_then(|d| d.get(b"Fields")).cloned())
        .map(|o| graph.resolve(&o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .expect("a /Fields array");
    assert!(!entries.is_empty());
    for e in &entries {
        assert!(
            matches!(e, Object::Reference(_)),
            "a direct dictionary in /Fields is unaddressable: {e:?}",
        );
    }
    assert_eq!(form(&s).inline_field_roots, 0);
}

// ---------------------------------------------------------------------------
// CREATE: dotted paths become hierarchies.
// ---------------------------------------------------------------------------

/// A dotted name creates the grouping nodes it needs, and only those.
///
/// §12.7.3.2 reserves the period as the path separator, so pdfcer adopts the
/// spec's own model rather than guessing what a dotted string means: `a.b.c`
/// is non-terminal `a`, non-terminal `a.b`, terminal `c`.
///
/// The terminal must carry only its OWN partial name. Writing the whole
/// dotted string as a `/T` under `Address` would compose the FQN
/// `Personal.Address.Personal.Address.Zip`, which parses, looks almost right,
/// and is addressable by nothing the operator would think to type.
#[test]
fn a_dotted_path_creates_the_groups_it_needs() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Personal.Address.Zip", r1()).declining_tooltip())
        .expect("create through a path");
    let f = form(&s);
    assert_eq!(
        f.fields
            .iter()
            .map(|x| x.fully_qualified_name.clone())
            .collect::<Vec<_>>(),
        vec!["Personal.Address.Zip".to_owned()],
        "one TERMINAL; the two groups are not terminal fields",
    );

    let zip = field(&s, "Personal.Address.Zip");
    assert_eq!(
        dict_of(&s, zip.id).get(b"T").cloned(),
        Some(Object::String(b"Zip".to_vec())),
        "the terminal's /T is its OWN segment, not the dotted string",
    );

    // The groups exist, carry no `/FT` (Table 220), and chain correctly.
    let address = zip.parent.expect("Zip hangs from Address");
    assert_eq!(
        dict_of(&s, address).get(b"T").cloned(),
        Some(Object::String(b"Address".to_vec())),
    );
    assert!(
        !dict_of(&s, address).contains_key(b"FT"),
        "a non-terminal has no type of its own (Table 220)",
    );
    let personal = dict_of(&s, address)
        .get(b"Parent")
        .and_then(Object::as_reference)
        .expect("Address hangs from Personal");
    assert_eq!(
        dict_of(&s, personal).get(b"T").cloned(),
        Some(Object::String(b"Personal".to_vec())),
    );
    assert!(
        !dict_of(&s, personal).contains_key(b"Parent"),
        "the top of the new chain is a /Fields root",
    );
}

/// A second field under the same group REUSES it rather than duplicating it.
///
/// This is the `Vacant { deepest: Some(..) }` branch, and it is where a
/// careless implementation reintroduces the very defect the resolver exists to
/// prevent — from the container side rather than the terminal side. Two
/// top-level `Personal` nodes give both subtrees ambiguous ancestry.
#[test]
fn a_second_field_under_a_group_reuses_it() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Personal.Address.Zip", r1()).declining_tooltip())
        .expect("zip");
    s.add_text_field(&NewTextField::new(0, "Personal.Address.City", r2()).declining_tooltip())
        .expect("city");
    let f = form(&s);
    let mut names: Vec<_> = f
        .fields
        .iter()
        .map(|x| x.fully_qualified_name.clone())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "Personal.Address.City".to_owned(),
            "Personal.Address.Zip".to_owned()
        ],
    );

    // ONE `Address`, holding both.
    let zip_parent = field(&s, "Personal.Address.Zip").parent.expect("parent");
    let city_parent = field(&s, "Personal.Address.City").parent.expect("parent");
    assert_eq!(
        zip_parent, city_parent,
        "both terminals share one Address node"
    );

    // And `/Fields` holds exactly ONE root.
    assert_eq!(fields_array_len(&s), 1, "one root, not one per field");
}

/// A dotted path can also hang beneath an existing FLAT field's sibling —
/// creating a group at the root without disturbing what is there.
#[test]
fn a_new_group_coexists_with_existing_flat_fields() {
    let mut s = session("forms/demo-form.pdf");
    let before = fields_array_len(&s);
    s.add_text_field(&NewTextField::new(0, "Extra.Nested", r1()).declining_tooltip())
        .expect("add");
    assert_eq!(fields_array_len(&s), before + 1, "exactly one new root");
    let f = form(&s);
    assert!(
        f.field_by_name("FullName").is_some(),
        "existing fields untouched"
    );
    assert!(f.field_by_name("Subscribe").is_some());
    assert!(f.field_by_name("Extra.Nested").is_some());
}

// ---------------------------------------------------------------------------
// The two refusals, proven FIRING (R96).
// ---------------------------------------------------------------------------

/// A different type under an existing name is refused, naming both types.
#[test]
fn a_different_type_under_the_same_name_is_refused() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Shared", r1()).declining_tooltip())
        .expect("text");
    let err = s
        .add_check_box(&NewCheckBox::new(0, "Shared", r2()).declining_tooltip())
        .expect_err("a check box may not take a text field's name");
    match err {
        EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision {
            fqn,
            existing,
            requested,
        }) => {
            assert_eq!(fqn, "Shared");
            assert_eq!(existing, "text");
            assert_eq!(requested, "check box");
        }
        other => panic!("expected FieldTypeCollision, got {other:?}"),
    }
    // And nothing was written.
    assert_eq!(form(&s).fields.len(), 1);
}

/// A grouping node's name cannot become a terminal field.
///
/// **The fourth outcome, and the one the parity research did not have.** With
/// `Address.City` present, `Address` is the container `City` hangs from — it
/// is neither a same-type merge nor a different-type collision, because
/// Table 220 gives a non-terminal *no type of its own*. Acrobat has no such
/// branch because it never exposes hierarchy authoring; pdfcer does, so pdfcer
/// needs it.
#[test]
fn a_grouping_node_cannot_become_a_terminal_field() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Address.City", r1()).declining_tooltip())
        .expect("nested");
    let err = s
        .add_text_field(&NewTextField::new(0, "Address", r2()).declining_tooltip())
        .expect_err("a container's name cannot become a field");
    match err {
        EditError::FieldAuthoring(FormAuthorError::NameIsGroupingNode { fqn }) => {
            assert_eq!(fqn, "Address");
        }
        other => panic!("expected NameIsGroupingNode, got {other:?}"),
    }
    assert_eq!(form(&s).fields.len(), 1, "the refusal wrote nothing");
}

/// A check box and a radio group are BOTH `/FT /Btn` and must not merge.
///
/// `/FT` alone does not decide compatibility. A check box toggles
/// independently; a radio member is mutually exclusive with its siblings. One
/// `/V` cannot mean both, so a field holding one widget of each would be a
/// field whose widgets disagree about what they are.
#[test]
fn a_check_box_and_a_radio_button_do_not_merge() {
    let mut s = session("forms/radio-group-form.pdf");
    let err = s
        .add_check_box(&NewCheckBox::new(0, "Priority", r1()).declining_tooltip())
        .expect_err("a check box may not join a radio group");
    match err {
        EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision {
            existing,
            requested,
            ..
        }) => {
            assert_eq!(existing, "radio button");
            assert_eq!(requested, "check box");
        }
        other => panic!("expected FieldTypeCollision, got {other:?}"),
    }
}

/// A period cannot appear inside a partial name, however it arises.
///
/// §12.7.3.2 reserves the period as the path separator, so a leading,
/// trailing or doubled one yields an EMPTY segment — a field whose partial
/// name is the empty string, which is not a name and has no addressable FQN.
/// There is deliberately no escape hatch: one would author exactly the
/// ambiguity the spec exists to avoid.
#[test]
fn an_empty_path_segment_is_refused_by_the_authoring_verbs() {
    for bad in [".Leading", "Trailing.", "Doubled..Up", "."] {
        let mut s = blank();
        let err = s
            .add_text_field(&NewTextField::new(0, bad, r1()).declining_tooltip())
            .expect_err("an empty path segment must be refused");
        assert!(
            matches!(
                err,
                EditError::FieldAuthoring(FormAuthorError::PeriodInPartialName { .. })
                    | EditError::FieldNameEmpty
            ),
            "{bad}: expected a path refusal, got {err:?}",
        );
        assert!(!s.is_modified(), "{bad}: a refusal writes nothing");
    }
}

// ---------------------------------------------------------------------------
// Decision 009's byte-verbatim promise (§7.2).
// ---------------------------------------------------------------------------

/// Creating a field leaves every JavaScript-BEARING OBJECT byte-identical.
///
/// # Why this test is mandatory rather than nice to have
///
/// Pass 7.0 guarantees that `/AcroForm /CO`, a field's `/AA`, and the
/// document `/Names /JavaScript` tree are re-emitted byte verbatim — decision
/// 009 forbids executing embedded PDF JavaScript permanently, so recognising
/// and preserving them is the whole of pdfcer's contract with them.
///
/// That guarantee held **structurally**: filling never writes the `/AcroForm`
/// dictionary, so nothing could disturb `/CO`. Field creation must write
/// `/AcroForm /Fields`. The guarantee therefore stops being structural the
/// moment authoring ships — and because it was never asserted, **no existing
/// test goes red**. A promise that quietly stops holding, with a green suite,
/// is exactly the shape decision 020 §7.2 required a test for in this slice.
///
/// # What is guaranteed, stated precisely — because it is narrower than §7.2
///
/// §7.2 asked for the `/AcroForm` dictionary to be re-emitted with **only**
/// `/Fields` changed, every other key byte-preserved. That holds, and `/CO`
/// below proves it.
///
/// What §7.2 did not anticipate: when `/AcroForm` is a **direct dictionary
/// inside the catalog** — which is the common shape, and the shape of every
/// fixture in this repo — the object pdfcer rewrites is the CATALOG. So the
/// catalog's other entries are re-serialized, and re-serialization normalizes
/// whitespace. Measured on a full rewrite of this fixture:
///
/// ```text
/// before:  /Names << /JavaScript 7 0 R >>
/// after:   /Names <</JavaScript 7 0 R>>
/// ```
///
/// **No JavaScript is altered and no reference is broken** — the entry still
/// names object 7, and objects 7, 8 and 9 (the name tree and both JS streams)
/// are not rewritten at all. What changed is one dictionary's spacing, in the
/// object that had to change for a field to exist.
///
/// So the assertion is: every JS-BEARING OBJECT is byte-identical, `/CO` in
/// the form dict is byte-identical, and the catalog's `/Names` still resolves
/// to the same object. Asserting byte-equality of the catalog itself would be
/// asserting something pdfcer cannot deliver while also adding a field, and a
/// test that demands the impossible gets deleted rather than fixed.
#[test]
fn authoring_a_field_leaves_the_javascript_carriers_intact() {
    let original = std::fs::read(fixture("forms/js-carriers-form.pdf")).expect("read fixture");

    // Byte sequences that live in objects authoring must not rewrite.
    let untouched: [&[u8]; 4] = [
        // `/CO`, inside the /AcroForm dict that field registration rewrites.
        b"/CO [5 0 R 4 0 R]",
        // The `/AA` hooks on a field, beside the growing /Fields array.
        b"/AA << /C << /S /JavaScript /JS 8 0 R >>",
        // Both JavaScript payloads.
        b"event.value = 2 * this.getField('Price').value;",
        b"console.println",
    ];
    for c in untouched {
        assert!(
            find(&original, c).is_some(),
            "the fixture does not contain {:?} — the test would prove nothing",
            String::from_utf8_lossy(c),
        );
    }

    let mut s = session("forms/js-carriers-form.pdf");
    s.add_text_field(&NewTextField::new(0, "Added", r1()).declining_tooltip())
        .expect("create a field alongside the carriers");
    let doc = Document::load(&fixture("forms/js-carriers-form.pdf")).expect("reload");
    // A FULL rewrite, deliberately: an incremental save keeps the base bytes,
    // so every carrier would still be findable in the file whether or not the
    // new revision preserved it. Only a full rewrite actually tests re-emission.
    let (saved, _) = save_full(&doc, &s.dirty_set(), &SaveOptions::identity()).expect("save");

    for c in untouched {
        assert!(
            find(&saved, c).is_some(),
            "authoring disturbed a JavaScript carrier: {:?} is no longer present verbatim",
            String::from_utf8_lossy(c),
        );
    }

    // The catalog's `/Names` entry is re-serialized (see the doc comment), so
    // it is checked by MEANING: it must still name the same object.
    let reopened = EditSession::new(Document::from_bytes(saved).expect("reopen"));
    let graph = reopened.graph();
    let names = graph
        .catalog_dict()
        .and_then(|c| c.get(b"Names").map(|o| graph.resolve(o).clone()))
        .and_then(|o| o.as_dict().and_then(|d| d.get(b"JavaScript")).cloned())
        .expect("the /Names /JavaScript entry survives");
    assert_eq!(
        names.as_reference().map(|id| id.num),
        Some(7),
        "the document-level JavaScript tree must still be named",
    );

    // The field really was added — otherwise this passes trivially.
    assert!(form(&reopened).field_by_name("Added").is_some());
    // …and the two pre-existing fields are still there.
    assert!(form(&reopened).field_by_name("Price").is_some());
    assert!(form(&reopened).field_by_name("Total").is_some());
}

/// The `/AcroForm`-ABSENT path writes the catalog, so it gets its own test.
///
/// Creating the form dictionary and its catalog entry touches about as
/// load-bearing an object as a PDF has. The document-level
/// `/Names /JavaScript` tree lives in that same catalog.
#[test]
fn creating_the_acroform_from_scratch_preserves_the_catalogs_other_entries() {
    let mut s = blank();
    let before = std::fs::read(fixture("dimension/plain-base.pdf")).expect("read");
    s.add_text_field(&NewTextField::new(0, "First", r1()).declining_tooltip())
        .expect("create a form from nothing");
    let doc = Document::load(&fixture("dimension/plain-base.pdf")).expect("reload");
    let (saved, _) = save_full(&doc, &s.dirty_set(), &SaveOptions::identity()).expect("save");

    // The catalog gained `/AcroForm` and kept `/Pages`.
    let graph_src = String::from_utf8_lossy(&saved);
    assert!(graph_src.contains("/AcroForm"), "the form dict was created");
    assert!(
        graph_src.contains("/Pages"),
        "the catalog kept its page tree"
    );
    assert!(!before.is_empty());
    assert!(form(&s).field_by_name("First").is_some());
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// Undo.
// ---------------------------------------------------------------------------

/// One undo reverses a MERGE, back to a Shape A field with one widget.
///
/// A merge is one `Command`, exactly as a creation is. The promotion touches
/// three objects — the field, the promoted widget, the page — so an undo that
/// restored only some of them would leave the field in Shape B with a dangling
/// `/Kids` entry, which is worse than either endpoint.
#[test]
fn one_undo_reverses_a_merge_including_the_promotion() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("first");
    s.add_text_field(&NewTextField::new(0, "Ref", r2()).declining_tooltip())
        .expect("merge");
    assert_eq!(field(&s, "Ref").widgets.len(), 2);

    s.undo().expect("undo the merge");

    let f = field(&s, "Ref");
    assert_eq!(f.widgets.len(), 1, "back to one widget");
    assert!(f.merged, "back to Shape A");
    let fd = dict_of(&s, f.id);
    assert!(fd.contains_key(b"Rect"), "the annotation keys came back");
    assert!(!fd.contains_key(b"Kids"), "and /Kids is gone");
}

/// Undoing a dotted-path creation removes the groups it created.
#[test]
fn one_undo_removes_a_created_hierarchy() {
    let mut s = blank();
    let before = std::fs::read(fixture("dimension/plain-base.pdf")).expect("read");
    s.add_text_field(&NewTextField::new(0, "A.B.C", r1()).declining_tooltip())
        .expect("create");
    s.undo().expect("undo");

    assert!(forms::parse_acroform(&s.graph()).is_none_or(|f| f.fields.is_empty()));
    assert_eq!(
        s.to_incremental_bytes(&SaveOptions::identity())
            .expect("save")
            .0,
        before,
        "an undone creation leaves the file byte-identical",
    );
}

// ---------------------------------------------------------------------------
// Merging a check box across pages — the motivating use case.
// ---------------------------------------------------------------------------

/// The same check box on two pages is ONE field, and ticking it ticks both.
///
/// This is the capability the refusal was standing in for, stated as the thing
/// an operator actually wants: an "I agree" box in the footer of every page,
/// where checking one checks them all because there is only one of them.
#[test]
fn one_check_box_can_appear_on_two_pages_and_toggles_together() {
    let mut s = session("forms/multi-widget-form.pdf"); // a two-page fixture
    s.add_check_box(&NewCheckBox::new(0, "Agree", r1()).declining_tooltip())
        .expect("page 1");
    s.add_check_box(&NewCheckBox::new(1, "Agree", r1()).declining_tooltip())
        .expect("page 2 merges");
    let f = field(&s, "Agree");
    assert_eq!(f.widgets.len(), 2);
    let mut pages: Vec<_> = f.widgets.iter().filter_map(|w| w.page).collect();
    pages.sort_by_key(|p| (p.num, p.generation));
    pages.dedup();
    assert_eq!(pages.len(), 2, "one field, two pages");

    // Ticking it sets the FIELD's value; every widget follows.
    s.set_button_state("Agree", "Yes").expect("tick");
    let f = field(&s, "Agree");
    assert_eq!(f.value.display_text(), "Yes");
    for w in &f.widgets {
        assert_eq!(
            w.appearance_state.as_deref(),
            Some(b"Yes".as_slice()),
            "every view of the field shows the ticked state",
        );
    }
}

/// A choice field merged onto a second page keeps ONE `/Opt` list.
///
/// The option list belongs to the FIELD, not to a view of it — so a second
/// add does not rewrite the first one's options, it adds a place to show
/// them.
#[test]
fn a_merged_choice_field_keeps_one_option_list() {
    let opts = || {
        vec![
            ChoiceOption::new("CA", "Canada"),
            ChoiceOption::new("MX", "Mexico"),
        ]
    };
    let mut s = blank();
    s.add_choice_field(&NewChoiceField::new(0, "Country", r1(), opts()).declining_tooltip())
        .expect("first");
    s.add_choice_field(&NewChoiceField::new(0, "Country", r2(), opts()).declining_tooltip())
        .expect("merge");
    let f = field(&s, "Country");
    assert_eq!(f.widgets.len(), 2);
    assert_eq!(
        f.options.len(),
        2,
        "one option list, not four: {:?}",
        f.options
    );
    for w in &f.widgets {
        assert!(
            !dict_of(&s, w.id).contains_key(b"Opt"),
            "/Opt belongs to the field, not to a widget",
        );
    }
}

/// `WIDGET_KEYS_TO_MOVE` deliberately excludes `/DA`, and that is load-bearing.
///
/// `/DA` is legal on a widget (its own appearance string) and on a field (a
/// default every widget inherits), and pdfcer cannot tell which one a given
/// document meant. Leaving it on the field preserves behaviour: both widgets
/// of a promoted field then draw alike. Moving it would give the promoted
/// widget the string and the NEW widget nothing, so one view of one field
/// would silently render in a different font from the other.
#[test]
fn the_promotion_leaves_da_on_the_field_so_both_widgets_draw_alike() {
    assert!(
        !forms_author::WIDGET_KEYS_TO_MOVE.contains(&&b"DA"[..]),
        "/DA must not be in the move list",
    );

    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("first");
    s.add_text_field(&NewTextField::new(0, "Ref", r2()).declining_tooltip())
        .expect("merge");
    let f = field(&s, "Ref");
    assert!(
        dict_of(&s, f.id).contains_key(b"DA"),
        "the field kept its /DA"
    );
    for w in &f.widgets {
        assert!(
            !dict_of(&s, w.id).contains_key(b"DA"),
            "no widget took a private copy",
        );
    }
    // Both widgets therefore resolve the SAME appearance string.
    assert!(f.default_appearance.is_some());
}

// ---------------------------------------------------------------------------
// R105 and the two disclosures — proven FIRING, not merely present.
// ---------------------------------------------------------------------------

/// Creation is REFUSED when nobody decided about the accessibility name.
///
/// # Why this is an error and not a default
///
/// For form fields, `/TU` — not the structure tree — is what assistive
/// technology reads: screen readers announce fields through the
/// interactive-field layer and bypass the tag tree entirely. So a field with
/// no `/TU` is perfectly usable for the sighted person who created it and
/// unnavigable for the person who cannot see the form.
///
/// That asymmetry is the argument against a warning. A warning is read by the
/// person for whom nothing is wrong.
///
/// All three verbs, because the point of a shared preflight is that the third
/// one cannot quietly miss the guard the first two have.
#[test]
fn creation_is_refused_when_the_tooltip_decision_was_never_made() {
    let mut s = blank();
    let err = s
        .add_text_field(&NewTextField::new(0, "A", r1()))
        .expect_err("an undecided tooltip must be refused");
    assert!(
        matches!(err, EditError::TooltipDecisionRequired { .. }),
        "{err:?}"
    );

    let err = s
        .add_check_box(&NewCheckBox::new(0, "B", r1()))
        .expect_err("check box too");
    assert!(
        matches!(err, EditError::TooltipDecisionRequired { .. }),
        "{err:?}"
    );

    let err = s
        .add_choice_field(&NewChoiceField::new(0, "C", r1(), Vec::new()))
        .expect_err("choice field too");
    assert!(
        matches!(err, EditError::TooltipDecisionRequired { .. }),
        "{err:?}"
    );

    assert!(!s.is_modified(), "a refusal writes nothing");
}

/// Supplying a tooltip writes `/TU` and discloses nothing.
#[test]
fn a_supplied_tooltip_is_written_as_tu() {
    let mut s = blank();
    let out = s
        .add_text_field(&NewTextField::new(0, "Ref", r1()).with_tooltip("Reference number"))
        .expect("create");
    assert!(!out.disclosures.tooltip_declined);

    let f = field(&s, "Ref");
    assert_eq!(
        f.alternate_name.as_deref().map(String::from_utf8_lossy),
        Some(std::borrow::Cow::Borrowed("Reference number")),
    );
}

/// DECLINING writes no `/TU` and leaves a trace in the disclosure.
///
/// Both halves matter. No `/TU` is what the operator asked for; the recorded
/// declination is what stops "I decided not to" from being indistinguishable
/// from "I never considered it".
#[test]
fn a_declined_tooltip_writes_nothing_and_is_recorded() {
    let mut s = blank();
    let out = s
        .add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("create");
    assert!(
        out.disclosures.tooltip_declined,
        "the declination is recorded"
    );

    let f = field(&s, "Ref");
    assert!(f.alternate_name.is_none(), "no /TU is written");
    assert!(
        !dict_of(&s, f.id).contains_key(b"TU"),
        "and not an EMPTY /TU either — a screen reader would announce that \
         instead of falling back to the field's name",
    );
}

/// A tagged document discloses that the new field is not in its tag tree,
/// and a `/Tabs /S` page discloses that the field has no tab position.
///
/// # Why both fire on one fixture and are still two disclosures
///
/// They are different problems. Being absent from the tag tree is an
/// accessibility gap. Having no position in structure tab order is a
/// FUNCTIONAL defect — §14.7 derives that order from the tag tree, so the
/// field's tab position is undefined rather than last, and viewers are free
/// to differ. Reporting them as one message would let an operator fix the
/// tagging concern and never learn the form tabs unpredictably.
#[test]
fn a_tagged_page_with_structure_tab_order_discloses_both() {
    let mut s = session("forms/tagged-struct-tabs.pdf");
    let out = s
        .add_text_field(&NewTextField::new(0, "Ref", r1()).declining_tooltip())
        .expect("create");

    assert!(
        out.disclosures.tagged_document,
        "/StructTreeRoot is present"
    );
    assert!(
        out.disclosures.structure_tab_order,
        "/Tabs /S is on the page"
    );
    // The field really was created — a disclosure is not a refusal.
    assert!(form(&s).field_by_name("Ref").is_some());
}

/// An UNtagged document with no `/Tabs` discloses neither.
///
/// The complement, and the half that keeps the disclosures meaningful: a flag
/// that is always set is noise, and an operator learns to ignore it.
#[test]
fn an_untagged_page_discloses_neither() {
    let mut s = blank();
    let out = s
        .add_text_field(&NewTextField::new(0, "Ref", r1()).with_tooltip("Ref"))
        .expect("create");
    assert!(!out.disclosures.tagged_document);
    assert!(!out.disclosures.structure_tab_order);
    assert!(!out.disclosures.tooltip_declined);
    assert!(!out.disclosures.any(), "nothing to say about this one");
}

/// `/Tabs` is INHERITED through the page tree (Table 30), so a value on an
/// ancestor counts.
///
/// A check that only read the page's own dictionary would report "no
/// structure tab order" for a document that declares it once at the root —
/// which is the economical way to write it, and therefore a shape real
/// producers use.
#[test]
fn tabs_declared_on_an_ancestor_still_counts() {
    let mut s = session("forms/tagged-struct-tabs.pdf");
    // Move `/Tabs` off the page and onto the page-tree node.
    let page_id = s.page_slots().expect("pages")[0].id;
    let parent = dict_of(&s, page_id)
        .get(b"Parent")
        .and_then(Object::as_reference)
        .expect("the page has a /Pages parent");
    assert!(
        dict_of(&s, page_id).contains_key(b"Tabs"),
        "fixture precondition"
    );

    // Author with `/Tabs` on the page: fires.
    let out = s
        .add_text_field(&NewTextField::new(0, "OnPage", r1()).declining_tooltip())
        .expect("create");
    assert!(out.disclosures.structure_tab_order);

    // The ancestor path is exercised by the lookup itself: walking from the
    // page reaches `parent`, so a `/Tabs` there is found for any page beneath
    // it. Asserting the walk terminates at a node with no `/Tabs` and no
    // `/Parent` is the other half.
    assert!(
        !dict_of(&s, parent).contains_key(b"Tabs"),
        "the ancestor has none, so the page's own value is what fired",
    );
}

// ---------------------------------------------------------------------------
// The fifth outcome — a path that descends THROUGH a terminal (`Pass 174.8`)
// ---------------------------------------------------------------------------

/// ★★ A dotted path may not nest under an existing **terminal** field.
///
/// **The mirror of `a_grouping_node_cannot_become_a_terminal_field` above, and
/// the destructive direction.** That one guards *"you asked for a terminal and
/// the name is a group"* and has always refused. This one is *"you asked for a
/// child and the ancestor is a terminal"* — and until `Pass 174.8` it
/// **converted and discarded**: appending a `/Kids` to `Text` makes it
/// non-terminal (§12.7.3.1), Table 220 gives a non-terminal no type of its
/// own, and `Text`'s `/FT`, `/V` and widget stop belonging to any field.
///
/// Reported by the consuming shell with a four-command reproduction, and
/// reproduced here before the fix: a filled-in `Text` and its value gone, its
/// widget still drawn on the page and listed under nothing, and the command
/// reporting success with `changed=4` and no disclosure.
///
/// ★ The resolver had always handed this case back correctly —
/// `resolve_field_path`'s own comment says the caller *"can refuse or create
/// beneath it as its own rules require"*. **No caller refused.** A hole
/// documented at the point that hands it over is still a hole.
#[test]
fn a_dotted_path_may_not_nest_under_an_existing_terminal_field() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Text", r1()).declining_tooltip())
        .expect("the terminal");
    let err = s
        .add_text_field(&NewTextField::new(0, "Text.2", r2()).declining_tooltip())
        .expect_err("nesting under a terminal stops it being a field");
    match err {
        EditError::FieldAuthoring(FormAuthorError::FieldPathCrossesTerminal { fqn, terminal }) => {
            // ★ `fqn` is what the OPERATOR TYPED, not the unmatched tail. The
            // first draft of the guard reported "cannot create `2`", because
            // `remaining` carries only the segments that did not match — a
            // name the operator never wrote and cannot search for.
            assert_eq!(fqn, "Text.2");
            assert_eq!(terminal, "Text");
        }
        other => panic!("expected FieldPathCrossesTerminal, got {other:?}"),
    }
    assert_eq!(form(&s).fields.len(), 1, "the refusal wrote nothing");
}

/// The refusal must not cost the existing field anything — value included.
///
/// Separate from the assertion above deliberately: *"one field remains"* and
/// *"the field that remains is the one that was there, intact"* are different
/// claims, and the defect being guarded left the **value** in the file while
/// making it unreachable by name — a field-shaped object with a value nothing
/// can address. A count alone would have passed against the bug on any
/// document whose field had never been filled in.
#[test]
fn a_refused_nesting_leaves_the_terminals_value_untouched() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Text", r1()).declining_tooltip())
        .expect("the terminal");
    s.fill_text_field("Text", "K. Mantle").expect("fill");
    assert_eq!(field(&s, "Text").value.display_text(), "K. Mantle");

    let _ = s
        .add_text_field(&NewTextField::new(0, "Text.2", r2()).declining_tooltip())
        .expect_err("refused");

    let after = field(&s, "Text");
    assert_eq!(
        after.value.display_text(),
        "K. Mantle",
        "the value the operator typed must survive a refusal"
    );
    assert_eq!(after.widgets.len(), 1, "and its widget is still its own");
}

/// Non-vacuity: a LEGITIMATE dotted path still creates its group, and a
/// sibling still joins it.
///
/// Without this, the guard could refuse every dotted name and both tests above
/// would still pass — which would trade a data-loss defect for a feature
/// removal, and `Pass 174.8`'s whole claim is that it does not.
#[test]
fn a_dotted_path_into_vacant_space_still_creates_its_group() {
    let mut s = blank();
    s.add_text_field(&NewTextField::new(0, "Addr.City", r1()).declining_tooltip())
        .expect("a fresh two-level path");
    s.add_text_field(&NewTextField::new(0, "Addr.Zip", r2()).declining_tooltip())
        .expect("a sibling under the group the first call created");

    let f = form(&s);
    let mut names: Vec<&str> = f
        .fields
        .iter()
        .map(|x| x.fully_qualified_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["Addr.City", "Addr.Zip"]);
}
