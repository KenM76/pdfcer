//! The field-tree shapes no corpus file contains (decision 020's **F0**).
//!
//! ## Why a whole test file for shapes nothing produced
//!
//! `docs/ROADMAP.md`'s Pass 7.0 entry records the census: the organic corpus
//! tops out at 63 fields per file, and **no corpus file nests fields at
//! all**. Every non-terminal path in `forms::walk_field` — inheritance down
//! `/Kids`, fully-qualified-name composition, widget-kid classification — has
//! therefore never run against real data. It has only ever run on flat files,
//! where it does nothing.
//!
//! That was survivable while pdfcer only READ forms: a reader that mishandles
//! a shape no input contains mishandles nothing. Field AUTHORING ends it,
//! because authoring exists to GENERATE those shapes — a merge attaches a
//! second widget to an existing field, and Shape A→B promotion turns a merged
//! field into a `/Kids` parent. So the shapes are built first, the existing
//! verbs are run over them, and what breaks is fixed before anything depends
//! on it. Decision 019's Pass 19.0 set the precedent ("CORRECTNESS ONLY, no
//! new operator surface") and §4.3 of that decision argued the ordering.
//!
//! ## The four things under test
//!
//! 1. **FQN composition and inheritance across two levels** — a terminal
//!    three levels down reports `Personal.Address.Zip`, and picks up a
//!    `/FT` declared on its grandparent and a `/DA` declared on its parent.
//! 2. **Fan-out** — one field, three widgets, two pages: a fill paints all
//!    three, a flatten burns each onto its own page.
//! 3. **Mixed `/Kids`** — a node holding BOTH a child field and a bare
//!    widget of its own. This is the one that was broken.
//! 4. **`Field.parent`** — the pointer back into the tree that an
//!    inherited-`/V` write and a subtree rename both need.
//!
//! Fixtures come from `tools/gen-form-hierarchy-fixtures.py`, byte-authored
//! with no PDF library behind them so a fixture cannot inherit a bug from the
//! code it tests (`fixtures/synthetic/forms/PROVENANCE.md`, `LEGAL.md` §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::forms::{self, FieldType};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::writer::{SaveOptions, save_full};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms")
        .join(name)
}

fn session(name: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(name)).expect("load fixture"))
}

fn form(s: &EditSession) -> forms::AcroForm {
    forms::parse_acroform(&s.graph()).expect("fixture has an AcroForm")
}

fn named(f: &forms::AcroForm, fqn: &str) -> forms::Field {
    f.field_by_name(fqn)
        .unwrap_or_else(|| panic!("no field named {fqn}; have {:?}", names(f)))
        .clone()
}

fn names(f: &forms::AcroForm) -> Vec<String> {
    f.fields
        .iter()
        .map(|x| x.fully_qualified_name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// (b) Two-level hierarchy: FQN composition and inheritance.
// ---------------------------------------------------------------------------

/// A terminal three levels down reports the dotted path §12.7.3.2 derives.
///
/// `Personal.Address.Zip` cannot be produced by a reader that concatenates
/// only one level, and cannot be produced at all by a flat reader — so this
/// asserts the composition rather than merely that the field exists.
///
/// `Personal.Name` is asserted alongside it deliberately: it sits one level
/// shallower than `Zip`, so a walk that tracked depth with a single shared
/// counter rather than per-branch state would get the wrong prefix for
/// whichever of the two it visited second.
#[test]
fn a_terminal_three_levels_down_reports_its_full_dotted_path() {
    let f = form(&session("nested-form.pdf"));
    let mut got = names(&f);
    got.sort();
    assert_eq!(
        got,
        vec![
            "Personal.Address.City".to_owned(),
            "Personal.Address.Zip".to_owned(),
            "Personal.Name".to_owned(),
        ],
    );
}

/// `/FT` and `/DA` declared on ANCESTORS reach the terminals (§12.7.3.1).
///
/// The fixture declares `/FT /Tx` on `Personal` and on neither terminal, and
/// `/DA` on `Address` and on neither of its children. A reader that only
/// consults a terminal's own keys reports `field_type: None` — and then
/// refuses to fill a perfectly fillable field, which is how this failure
/// would actually reach an operator.
#[test]
fn a_type_and_a_default_appearance_are_inherited_from_ancestors() {
    let f = form(&session("nested-form.pdf"));

    let zip = named(&f, "Personal.Address.Zip");
    assert_eq!(
        zip.field_type,
        Some(FieldType::Text),
        "/FT from grandparent"
    );
    assert!(zip.is_fillable());
    let da = zip.default_appearance.expect("/DA inherited from /Address");
    assert!(
        da.windows(4).any(|w| w == b"Helv"),
        "the ancestor's /DA, not a synthesised one: {:?}",
        String::from_utf8_lossy(&da),
    );

    // `Personal.Name` is one level up, so it inherits `/FT` but NOT the `/DA`
    // that `Address` declares — asserting that inheritance follows the PATH
    // and not merely "some ancestor said so".
    let name = named(&f, "Personal.Name");
    assert_eq!(name.field_type, Some(FieldType::Text));
    assert!(
        name.default_appearance.is_none(),
        "/DA is declared on /Address, which is not on this field's path",
    );
}

/// Every terminal names the node it hangs from, and only roots have no parent.
///
/// `Field.parent` exists so an inherited-`/V` write can reach the ancestor
/// that DECLARED the value, and so a subtree rename can name the node whose
/// `/T` actually moves. Neither is recoverable from a flat list, which is why
/// the projection carries the pointer.
#[test]
fn every_terminal_records_the_node_it_hangs_from() {
    let s = session("nested-form.pdf");
    let f = form(&s);
    let graph = s.graph();

    let zip = named(&f, "Personal.Address.Zip");
    let parent = zip.parent.expect("Zip hangs from Address");
    // Identify the parent by its `/T`, not by object number: the number is an
    // artifact of how the fixture was written, the name is the thing meant.
    let t = graph
        .resolved(parent)
        .as_dict()
        .and_then(|d| d.get(b"T").cloned())
        .expect("the parent node carries a /T");
    assert_eq!(t, Object::String(b"Address".to_vec()));

    // `Personal.Name` hangs from `Personal`, one level higher.
    let name_parent = named(&f, "Personal.Name")
        .parent
        .expect("Name has a parent");
    assert_ne!(name_parent, parent, "different depths, different parents");

    // Nothing here is a root: `/AcroForm /Fields` holds only `Personal`, and
    // `Personal` is non-terminal so it never reaches the projection.
    assert!(f.fields.iter().all(|x| x.parent.is_some()));
}

/// A single root field in a FLAT form has no parent.
///
/// The complement of the test above: `parent: None` must mean "root", not
/// "the walk forgot to populate it". Without this, a walk that never set the
/// field at all would pass every assertion above by accident.
#[test]
fn a_flat_forms_root_fields_have_no_parent() {
    let f = form(&session("demo-form.pdf"));
    assert!(!f.fields.is_empty());
    for field in &f.fields {
        assert_eq!(
            field.parent, None,
            "{} is a /Fields root",
            field.fully_qualified_name,
        );
    }
}

// ---------------------------------------------------------------------------
// (a) Fan-out: one field, three widgets, two pages.
// ---------------------------------------------------------------------------

/// One field with three `/Kids` widgets reads back as ONE field, not three.
///
/// This is the shape a merge produces, so every existing verb must handle it
/// before authoring may generate it. Three widgets rather than two, and two
/// pages rather than one, because an off-by-one (`widgets[0]` alone) and a
/// flatten that burns everything onto page 1 are the errors this shape
/// actually invites — and both would pass a 2-widget single-page fixture.
#[test]
fn three_widgets_of_one_field_read_back_as_one_field() {
    let f = form(&session("multi-widget-form.pdf"));
    assert_eq!(names(&f), vec!["Reference".to_owned()]);

    let r = named(&f, "Reference");
    assert_eq!(r.widgets.len(), 3);
    assert!(!r.merged, "Shape B: the field dict is not itself a widget");

    // The three widgets sit on two DISTINCT pages.
    let mut pages: Vec<_> = r.widgets.iter().filter_map(|w| w.page).collect();
    pages.sort_by_key(|p| (p.num, p.generation));
    pages.dedup();
    assert_eq!(pages.len(), 2, "widgets span two pages");
}

/// Filling that field paints ALL THREE widgets — one value, three appearances.
///
/// The assertion is on the count the fill reports AND on what the reloaded
/// document contains, because a fill that generated three streams but
/// attached only one would report the right number and produce the wrong
/// file.
#[test]
fn filling_a_three_widget_field_regenerates_every_widget() {
    let mut s = session("multi-widget-form.pdf");
    let out = s.fill_text_field("Reference", "R-2000").expect("fill");
    assert_eq!(out.widgets_updated, 3);

    let f = form(&s);
    let r = named(&f, "Reference");
    assert_eq!(r.value.display_text(), "R-2000");
    assert_eq!(
        r.widgets.iter().filter(|w| w.has_normal_appearance).count(),
        3,
        "every widget carries a regenerated /AP /N",
    );
}

/// Flatten burns all three widgets, onto the two pages they actually sit on.
///
/// `pages_touched = 2` is the load-bearing half. A flatten that burned every
/// widget onto the first page would still report `widgets_burned = 3` and
/// would still remove the field — the document would simply have two copies
/// of the value on page 1 and a blank page 2.
#[test]
fn flatten_burns_every_widget_onto_its_own_page() {
    let mut s = session("multi-widget-form.pdf");
    s.fill_text_field("Reference", "R-2000").expect("fill");
    let out = s.flatten_fields(None).expect("flatten");

    assert_eq!(out.fields_flattened, 1);
    assert_eq!(out.widgets_burned, 3);
    assert_eq!(out.pages_touched, 2);

    // The form is empty afterwards: flatten removes what it burned.
    let f = forms::parse_acroform(&s.graph());
    assert!(f.is_none_or(|f| f.fields.is_empty()), "no fields survive");
}

// ---------------------------------------------------------------------------
// (d) Mixed `/Kids` — the shape that was silently dropping data.
// ---------------------------------------------------------------------------

/// A node with BOTH a child field and its own widget kid keeps both.
///
/// # The defect this pins
///
/// §12.7.3.1's merge rule classifies each kid INDIVIDUALLY: a kid with its
/// own `/T` is a child field, a `/T`-less widget kid is one of the parent's
/// own appearances. Nothing in the spec says a node must pick one KIND.
///
/// `walk_field` used to pick one. It partitioned `/Kids`, and if any kid was
/// a field it recursed into those and RETURNED — so a mixed node's own
/// widgets were never modelled and the node never reached the projection. Its
/// `/V` and its rectangle vanished from `list-fields`, from
/// `regenerate-appearances`, from `export-data` and from `flatten`, while the
/// page's `/Annots` still referenced the widget, so a viewer painted a field
/// the form did not contain.
///
/// Measured on this fixture before the fix: `list-fields` reported
/// `Order.Qty` alone, `fields=1`, and `Order` not at all.
///
/// No corpus file has the shape, which is why it survived — and the merge
/// primitive can GENERATE it, by attaching a widget to a node that already
/// has a child field.
#[test]
fn a_node_with_both_a_child_field_and_its_own_widget_keeps_both() {
    let f = form(&session("mixed-kids-form.pdf"));

    let mut got = names(&f);
    got.sort();
    assert_eq!(got, vec!["Order".to_owned(), "Order.Qty".to_owned()]);

    // The mixed node itself: its value, its type and its on-page presence all
    // survive. A test that only counted fields would pass on a `Order` that
    // came back typeless and widgetless.
    let order = named(&f, "Order");
    assert_eq!(order.value.display_text(), "ORD-77");
    assert_eq!(order.field_type, Some(FieldType::Text));
    assert_eq!(order.widgets.len(), 1, "its own /T-less widget kid");
    assert!(order.is_fillable());

    // The child field is unaffected, and knows its parent.
    let qty = named(&f, "Order.Qty");
    assert_eq!(qty.value.display_text(), "3");
    assert_eq!(qty.parent, Some(order.id));
}

/// The mixed node's own widget is the widget KID, never the node dictionary.
///
/// `Order` has `/Kids`, so it is Shape B and the node dict is not itself an
/// annotation. Modelling the node as its own widget would point every
/// appearance write at a dictionary the page's `/Annots` does not reference —
/// the field would fill, and nothing would draw.
#[test]
fn the_mixed_nodes_widget_is_the_kid_not_the_node_itself() {
    let f = form(&session("mixed-kids-form.pdf"));
    let order = named(&f, "Order");
    assert!(!order.merged);
    assert_ne!(order.widgets[0].id, order.id);
}

/// Both fields of the mixed node fill and regenerate independently.
///
/// The end-to-end proof that the recovered field is a REAL field rather than
/// a dict that happens to parse: the untouched fill verb accepts it, and the
/// child field beside it is not disturbed.
#[test]
fn both_halves_of_a_mixed_node_fill_independently() {
    let mut s = session("mixed-kids-form.pdf");
    assert_eq!(
        s.fill_text_field("Order", "ORD-99")
            .unwrap()
            .widgets_updated,
        1
    );
    assert_eq!(
        s.fill_text_field("Order.Qty", "7").unwrap().widgets_updated,
        1
    );

    let f = form(&s);
    assert_eq!(named(&f, "Order").value.display_text(), "ORD-99");
    assert_eq!(named(&f, "Order.Qty").value.display_text(), "7");
}

// ---------------------------------------------------------------------------
// (c) Radio group, and (e) the XFA hybrid.
// ---------------------------------------------------------------------------

/// A `/Kids` radio group selects exclusively: one `/V`, one `/AS` on.
///
/// §12.7.4.2.3. Every widget needs an `/Off` stream to switch TO, which is
/// why the fixture gives each kid a two-key `/AP /N` — a group modelled as
/// "one appearance per widget" cannot deselect.
#[test]
fn selecting_one_radio_member_turns_the_others_off() {
    let mut s = session("radio-group-form.pdf");
    // The fixture starts on `Green`.
    assert_eq!(
        form(&s)
            .field_by_name("Priority")
            .unwrap()
            .value
            .display_text(),
        "Green"
    );

    s.set_button_state("Priority", "Blue").expect("select Blue");

    let f = form(&s);
    let p = named(&f, "Priority");
    assert_eq!(p.value.display_text(), "Blue");
    let on: Vec<_> = p
        .widgets
        .iter()
        .filter(|w| w.appearance_state.as_deref() != Some(b"Off".as_slice()))
        .collect();
    assert_eq!(on.len(), 1, "exactly one widget is on");
    assert_eq!(on[0].appearance_state.as_deref(), Some(b"Blue".as_slice()));
}

/// A static-XFA hybrid's AcroForm half still reads and fills.
///
/// Field CREATION is refused there (decision 020 §3.2.2) because pdfcer can
/// write the AcroForm half and not the XFA half, so a one-sided add makes the
/// two halves disagree. Reading is a different question: the AcroForm half is
/// a real form, and refusing it would refuse a capability pdfcer has.
#[test]
fn an_xfa_hybrids_acroform_half_still_reads_and_fills() {
    let mut s = session("xfa-hybrid-form.pdf");
    let f = form(&s);
    assert!(
        f.xfa.is_present(),
        "the fixture is a hybrid, not a plain form"
    );
    assert_eq!(named(&f, "Applicant").value.display_text(), "J. Doe");

    s.fill_text_field("Applicant", "R. Roe")
        .expect("fill is allowed");
    assert_eq!(
        form(&s)
            .field_by_name("Applicant")
            .unwrap()
            .value
            .display_text(),
        "R. Roe"
    );
}

// ---------------------------------------------------------------------------
// The `/Fields` entries a walk cannot descend into.
// ---------------------------------------------------------------------------

/// A conformant file reports zero inline `/Fields` roots.
///
/// §12.7.3.1 requires every field to be an indirect object. `inline_field_roots`
/// exists so that when a file breaks that rule, `fields.len()` understating
/// the true count is REPORTED rather than silent — the difference between
/// tolerating damage and hiding it.
///
/// Asserting `0` across every fixture is the half that matters day to day: it
/// pins that the counter does not fire on well-formed input, which is what
/// would make it noise.
#[test]
fn well_formed_fixtures_report_no_inline_field_roots() {
    for name in [
        "demo-form.pdf",
        "nested-form.pdf",
        "multi-widget-form.pdf",
        "mixed-kids-form.pdf",
        "radio-group-form.pdf",
        "xfa-hybrid-form.pdf",
    ] {
        assert_eq!(
            form(&session(name)).inline_field_roots,
            0,
            "{name} is well-formed",
        );
    }
}

// ---------------------------------------------------------------------------
// Deleting the last child of a grouping node deletes the node.
// ---------------------------------------------------------------------------

/// Flattening every child of a grouping node removes the node too, and
/// cascades to ITS parent when that empties as well.
///
/// # Why an empty grouping node is not merely untidy
///
/// A node left with `/Kids []` still has a name, and a name still OCCUPIES
/// its slot in §12.7.3.2's FQN space. So `Personal.Address` would still be
/// taken — and a later request to create a terminal field called
/// `Personal.Address` would be refused as a grouping-node collision, by a
/// node that exists only because a deletion did not finish. The operator sees
/// a name they cannot use and no field it belongs to.
///
/// The cascade is recursive because emptying a parent can empty ITS parent:
/// `Zip` and `City` are all of `Address`, and `Address` plus `Name` are all
/// of `Personal`. Flattening the whole form must leave nothing behind, and a
/// single pass would leave two dead nodes.
#[test]
fn flattening_a_grouping_nodes_last_child_removes_the_node() {
    let mut s = session("nested-form.pdf");
    // Give the fields appearances so flatten has something to burn.
    s.regenerate_appearances().expect("regen");

    // Flatten ONLY the two fields under `Address`, leaving `Personal.Name`.
    // `Address` therefore empties and must go; `Personal` still has `Name`
    // and must NOT.
    s.flatten_fields(Some(&["Personal.Address.Zip", "Personal.Address.City"]))
        .expect("flatten the Address subtree");

    let f = form(&s);
    assert_eq!(names(&f), vec!["Personal.Name".to_owned()]);

    // `Name`'s parent is still `Personal`, and `Personal` is still a root —
    // the cascade stopped where it should, rather than unwinding the tree.
    let name = named(&f, "Personal.Name");
    let parent = name.parent.expect("Personal survives");
    let graph = s.graph();
    let kids = graph
        .resolved(parent)
        .as_dict()
        .and_then(|d| d.get(b"Kids").map(|o| graph.resolve(o).clone()))
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .expect("Personal still has /Kids");
    assert_eq!(kids.len(), 1, "Address was pruned, Name kept: {kids:?}");
}

/// Flattening the WHOLE nested form leaves no field tree at all.
///
/// The cascade's terminating case: with `Name` gone as well, `Personal` has
/// nothing left and is removed from `/AcroForm /Fields` in the same pass.
#[test]
fn flattening_every_field_empties_the_whole_tree() {
    let mut s = session("nested-form.pdf");
    s.regenerate_appearances().expect("regen");
    s.flatten_fields(None).expect("flatten everything");

    let f = forms::parse_acroform(&s.graph());
    assert!(
        f.as_ref().is_none_or(|f| f.fields.is_empty()),
        "fields survived: {:?}",
        f.as_ref().map(names),
    );
    // And nothing is left dangling in `/Fields` — not even the two grouping
    // nodes, which no `field_ids` list ever named.
    let graph = s.graph();
    let roots = graph
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm").map(|o| graph.resolve(o).clone()))
        .and_then(|o| o.as_dict().and_then(|d| d.get(b"Fields")).cloned())
        .map(|o| graph.resolve(&o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default();
    assert!(roots.is_empty(), "/Fields still holds {roots:?}");
}

/// Flatten really empties `/AcroForm /Fields` in the SAVED BYTES.
///
/// # Why this assertion is on bytes and not on the projection
///
/// Every other forms test asserts through `parse_acroform`, which resolves
/// each `/Fields` entry and drops the ones that no longer resolve. That makes
/// it structurally blind to this defect: with the field objects deleted, the
/// projection reports an empty form whether or not `/Fields` still names
/// them. The model looked right while the file was wrong.
///
/// What was wrong: `acroform_id` returns the object that HOLDS the form, and
/// when `/AcroForm` is a direct dictionary that object is the **catalog** —
/// one level above the dict `/Fields` lives in. The removal path read
/// `/Fields` off the catalog, found nothing, and its guard never fired. So
/// flatten deleted the field objects and left `/AcroForm /Fields` referencing
/// them. Measured on this very fixture with the shipped code:
///
/// ```text
/// flatten ... fields_flattened=2
/// /AcroForm << /Fields [4 0 R 5 0 R] ...
/// ```
///
/// `demo-form.pdf` is the shipped Pass 7 fixture and has a direct
/// `/AcroForm`, so this was reachable from the very first flatten pdfcer ever
/// performed.
#[test]
fn flatten_clears_the_fields_array_in_the_saved_bytes() {
    let mut s = session("demo-form.pdf");
    s.flatten_fields(None).expect("flatten");
    let doc = Document::load(&fixture("demo-form.pdf")).expect("reload for the writer");
    let (bytes, _) = save_full(&doc, &s.dirty_set(), &SaveOptions::identity()).expect("save");

    let text = String::from_utf8_lossy(&bytes);
    let at = text.find("/AcroForm").expect("the form dict survives");
    let window = &text[at..(at + 200).min(text.len())];
    let fields_at = window.find("/Fields").expect("/Fields is still present");
    let rest = &window[fields_at + "/Fields".len()..];
    let open = rest.find('[').expect("/Fields holds an array");
    let close = rest.find(']').expect("the array closes");
    assert!(
        rest[open + 1..close].trim().is_empty(),
        "/Fields still names the deleted fields: {}",
        &window[..120.min(window.len())],
    );
}

// ===========================================================================
// F6 — rename (decision 020 §6, `rename-field`)
// ===========================================================================

/// Renaming a GROUPING node re-derives every descendant's fully-qualified
/// name — while writing exactly one dictionary.
///
/// # Why this is the test that matters, and why it asserts on BYTES
///
/// §12.7.3.2 builds the FQN by walking DOWN and appending each node's `/T`.
/// So the whole subtree's identity changes off one write, and the descendants'
/// own objects are never touched. That is a fact about the FILE, and
/// `parse_acroform` cannot witness it: the projection would report the new
/// names just as happily if the rename had been applied only to the in-memory
/// model and never reached a saved byte. **R159** — the shipped `flatten`
/// defect hid behind exactly that projection, so the assertion is on the
/// serialized `/T`.
///
/// The count is the disclosure. `Personal.Address` → `Personal.Location`
/// renames two fields the operator did not name, and an operator not told so
/// has silently broken every FDF that referenced them. (Button actions
/// naming them are repaired since `Pass 184.0`; an FDF is not.)
#[test]
fn renaming_a_grouping_node_renames_its_whole_subtree() {
    let mut s = session("nested-form.pdf");
    let before = names(&form(&s));
    assert!(
        before.contains(&"Personal.Address.Zip".to_owned()),
        "fixture precondition: the subtree exists; have {before:?}",
    );

    let outcome = s
        .rename_field("Personal.Address", "Location")
        .expect("renaming a grouping node is allowed");

    assert_eq!(outcome.from, "Personal.Address");
    assert_eq!(outcome.to, "Personal.Location");
    assert_eq!(
        outcome.descendants_renamed, 2,
        "Zip and City are renamed by a request that named neither",
    );

    let after = names(&form(&s));
    assert!(
        after.contains(&"Personal.Location.Zip".to_owned())
            && after.contains(&"Personal.Location.City".to_owned()),
        "the subtree did not follow its parent: {after:?}",
    );
    assert!(
        after.contains(&"Personal.Name".to_owned()),
        "a sibling outside the renamed subtree must not move: {after:?}",
    );

    // The byte-level half. `/T (Location)` must be IN THE FILE, not merely in
    // the projection — and `Address` must be gone from it.
    let doc = Document::load(&fixture("nested-form.pdf")).expect("reload for the writer");
    let (bytes, _) = save_full(&doc, &s.dirty_set(), &SaveOptions::identity()).expect("save");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/T (Location)"),
        "the new partial name never reached the saved bytes",
    );
    assert!(
        !text.contains("/T (Address)"),
        "the old partial name survives in the saved bytes",
    );
}

/// A rename onto an occupied name is REFUSED, not merged.
///
/// The asymmetry with `add-*` is deliberate and is the whole content of this
/// test: a same-type `add-text-field` MERGES into an existing name, because
/// §12.7.3.2 makes same-FQN nodes representations of one field and the caller
/// asked for a field of that name. A rename did not — it named an existing
/// field and a new name — so fusing two identities would destroy one the
/// operator never offered.
#[test]
fn renaming_onto_an_occupied_name_is_refused() {
    let mut s = session("nested-form.pdf");
    let err = s
        .rename_field("Personal.Address.City", "Zip")
        .expect_err("Personal.Address.Zip is taken");
    let msg = err.to_string();
    assert!(
        msg.contains("already bears that name"),
        "a collision must say the name is taken, got: {msg}",
    );

    // And the refusal must be BEFORE any mutation — a half-applied rename is
    // worse than a refused one.
    let after = names(&form(&s));
    assert!(
        after.contains(&"Personal.Address.City".to_owned()),
        "the field moved despite the refusal: {after:?}",
    );
}

/// A dotted name and a malformed one are DIFFERENT refusals.
///
/// `A.B` is a well-formed two-level path that is simply not a *partial* name;
/// `A..B` has no valid reading at all. Telling the first operator that their
/// input "contains an empty name segment" describes a defect it does not
/// have, and sends them looking for a typo that is not there.
#[test]
fn a_dotted_partial_name_and_a_malformed_one_refuse_differently() {
    let mut s = session("nested-form.pdf");

    let dotted = s
        .rename_field("Personal.Name", "A.B")
        .expect_err("a partial name is one segment")
        .to_string();
    assert!(
        dotted.contains("is a path, not a partial name"),
        "a well-formed path needs its own refusal, got: {dotted}",
    );

    let malformed = s
        .rename_field("Personal.Name", "A..B")
        .expect_err("an empty segment is malformed")
        .to_string();
    assert!(
        malformed.contains("empty name segment"),
        "a malformed name keeps the segment refusal, got: {malformed}",
    );

    assert_ne!(
        dotted, malformed,
        "the two cases must not share a message; that is the point",
    );
}

// ---------------------------------------------------------------------------
// Widget geometry: moving one appearance without regenerating it.
// ---------------------------------------------------------------------------

/// A move lands in the SAVED BYTES, not merely in the model.
///
/// Asserted on the file rather than through `parse_acroform` deliberately
/// (R159): the projection would report the new rectangle just as happily if
/// the write never reached the document, which is exactly how a shipped
/// flatten defect hid — the model looked right while the file was wrong.
#[test]
fn moving_a_widget_writes_the_new_rect_into_the_file() {
    let mut s = session("demo-form.pdf");
    let before = named(&form(&s), "FullName");
    let rect = before.widgets[0]
        .rect
        .expect("the fixture's widget has a /Rect");

    let moved = s
        .move_widget("FullName", 0, 25.0, -10.0)
        .expect("move applies");
    assert_eq!(moved.from, rect, "the outcome reports where it started");
    assert!(
        (moved.to.llx - (rect.llx + 25.0)).abs() < 1e-9
            && (moved.to.lly - (rect.lly - 10.0)).abs() < 1e-9,
        "the rectangle translated by the requested delta, got {:?}",
        moved.to,
    );
    assert!(
        (moved.to.width() - rect.width()).abs() < 1e-9
            && (moved.to.height() - rect.height()).abs() < 1e-9,
        "a MOVE must not change the extent — that would make §12.5.5's scale \
         factors differ from 1 and stretch the appearance",
    );

    let doc = Document::load(&fixture("demo-form.pdf")).expect("reload for the writer");
    let (bytes, _) = save_full(&doc, &s.dirty_set(), &SaveOptions::identity()).expect("save");
    let text = String::from_utf8_lossy(&bytes);
    // The writer always emits a `.` for a Real (`serialize.rs`: "Object::Real(4.0)
    // emits `4.0`, not `4`", so a re-parse yields Real rather than Integer). The
    // assertion spells the number the way the FILE spells it — Rust's `{}` would
    // print `45` and silently never match.
    let wanted = format!("{:.1} {:.1}", rect.llx + 25.0, rect.lly - 10.0);
    assert!(
        text.contains(&wanted),
        "the moved lower-left corner {wanted:?} is not in the saved bytes",
    );
}

/// The appearance stream is NOT rewritten by a move.
///
/// This is the assertion that makes the "no regeneration needed" claim
/// checkable rather than merely stated. §12.5.5 step b derives its scale
/// from `Rect_extent / box_extent`, so an unchanged extent leaves the
/// artwork alone — and if some later change starts regenerating here, the
/// object count moves and this test says so.
#[test]
fn a_move_rewrites_one_object_and_leaves_the_appearance_alone() {
    let mut s = session("demo-form.pdf");
    let before = named(&form(&s), "FullName");
    let ap_before = before.widgets[0].id;

    s.move_widget("FullName", 0, 5.0, 5.0)
        .expect("move applies");

    let dirty = s.dirty_set();
    assert_eq!(
        dirty.len(),
        1,
        "a move is one dictionary write; {} objects were touched, which means \
         something is regenerating that does not need to be",
        dirty.len(),
    );
    assert!(
        dirty.contains(ap_before),
        "the one object written is the widget whose /Rect changed",
    );
}

/// Moving one widget of a multi-widget field leaves the others, and SAYS SO.
///
/// A field with widgets on several pages is one thing to an operator and
/// several to the format. Moving one silently would be a partial result that
/// reads as a bug later, so the count is part of the outcome.
#[test]
fn moving_one_widget_discloses_the_siblings_it_left_behind() {
    let mut s = session("multi-widget-form.pdf");
    let field = named(&form(&s), "Reference");
    assert!(
        field.widgets.len() > 1,
        "this test needs a multi-widget field; got {}",
        field.widgets.len(),
    );
    let untouched = field.widgets[1].rect.expect("sibling has a /Rect");

    let moved = s
        .move_widget("Reference", 0, 40.0, 0.0)
        .expect("move applies");
    assert_eq!(
        moved.siblings_left_behind,
        field.widgets.len() - 1,
        "the outcome must disclose every sibling left standing",
    );

    let after = named(&form(&s), "Reference");
    assert_eq!(
        after.widgets[1].rect.expect("sibling still has a /Rect"),
        untouched,
        "moving widget 0 must not have moved widget 1",
    );
}

/// A widget with no usable `/Rect` is refused, not given one.
#[test]
fn a_widget_without_a_rect_is_refused_rather_than_placed() {
    let mut s = session("demo-form.pdf");
    let err = s
        .move_widget("FullName", 99, 1.0, 1.0)
        .expect_err("index 99 does not exist");
    let msg = err.to_string();
    assert!(
        msg.contains("widget 99"),
        "the refusal names the index the operator gave; got {msg:?}",
    );
}

// ---------------------------------------------------------------------------
// `AcroForm::descendants_of` — the blast radius, and the separator that
// bounds it (Pass 53.0 extracted it out of `rename_field`)
// ---------------------------------------------------------------------------
//
// The function is a one-line filter, and that is exactly why it is a shared
// function: the line contains a subtlety that is invisible once written and
// wrong once forgotten. It now has two consumers — `rename_field`'s
// `descendants_renamed` disclosure, and the GUI's re-keying of the per-field
// value drafts it holds under the old names — and a definition that drifted
// between them would make the count the operator is shown disagree with the
// set of drafts that actually moved.

/// **A node's descendants are the fields BENEATH it, by path.**
///
/// `nested-form.pdf` is `Personal.Name`, `Personal.Address.City`,
/// `Personal.Address.Zip`.
#[test]
fn descendants_of_reports_the_fields_beneath_a_node() {
    let f = form(&session("nested-form.pdf"));

    let mut under_personal: Vec<String> = f
        .descendants_of("Personal")
        .map(|d| d.fully_qualified_name.clone())
        .collect();
    under_personal.sort();
    assert_eq!(
        under_personal,
        vec![
            "Personal.Address.City".to_owned(),
            "Personal.Address.Zip".to_owned(),
            "Personal.Name".to_owned(),
        ],
        "renaming `Personal` re-derives all three terminals' names"
    );

    let mut under_address: Vec<String> = f
        .descendants_of("Personal.Address")
        .map(|d| d.fully_qualified_name.clone())
        .collect();
    under_address.sort();
    assert_eq!(
        under_address,
        vec![
            "Personal.Address.City".to_owned(),
            "Personal.Address.Zip".to_owned(),
        ],
        "`Personal.Name` is a sibling of `Address`, not beneath it"
    );

    assert_eq!(
        f.descendants_of("Personal.Name").count(),
        0,
        "a leaf has nothing beneath it — the common case, and the one where \
         the operator's mental model and the effect coincide"
    );
}

/// **A node is not its own descendant.**
///
/// A rename writes the target's dictionary, so the target is the subject of
/// the operation rather than a consequence of it. Counting it would inflate
/// every disclosure by one and make a leaf rename claim it changed a field
/// "beneath" itself.
#[test]
fn a_node_is_not_its_own_descendant() {
    let f = form(&session("nested-form.pdf"));
    assert!(
        f.descendants_of("Personal.Address")
            .all(|d| d.fully_qualified_name != "Personal.Address"),
        "the node itself must not appear in its own descendant list"
    );
}

/// **★ A name that merely SHARES A PREFIX is not a descendant.**
///
/// The assertion the separator exists for, and the one a correct-looking
/// `starts_with(fqn)` fails. `Address.` matches `Address.City`; a bare
/// `Address` would also match `Addressed`, which is a different field
/// entirely — so renaming `Address` would claim to have renamed it, and the
/// GUI would move its half-typed value onto a name that was never created.
///
/// No fixture contains the near miss (nothing would; it is the shape nobody
/// thinks to build), so the sibling is synthesised here by cloning a real
/// parsed field and renaming it. That tests the FUNCTION, which is where the
/// subtlety lives, without perturbing a fixture three other tests assert the
/// exact contents of.
#[test]
fn a_shared_prefix_without_the_separator_is_not_a_descendant() {
    let mut f = form(&session("nested-form.pdf"));

    let near_miss = {
        let mut clone = f
            .field_by_name("Personal.Name")
            .expect("fixture has Personal.Name")
            .clone();
        // `Personal.Addressed` — one character past `Personal.Address`, and
        // NOT beneath it.
        clone.fully_qualified_name = "Personal.Addressed".to_owned();
        clone
    };
    f.fields.push(near_miss);

    let got: Vec<String> = f
        .descendants_of("Personal.Address")
        .map(|d| d.fully_qualified_name.clone())
        .collect();
    assert!(
        !got.contains(&"Personal.Addressed".to_owned()),
        "`Personal.Addressed` is a sibling that happens to start with the same \
         letters; got {got:?}"
    );
    assert_eq!(got.len(), 2, "still exactly City and Zip; got {got:?}");
}

/// **The count `rename_field` discloses is this function's count.**
///
/// The two must not drift: the number the operator reads and the set of
/// fields whose names actually changed are the same claim, and a disclosure
/// that overstates its blast radius trains the operator to discount it.
#[test]
fn the_renames_disclosed_count_matches_the_descendant_list() {
    let mut s = session("nested-form.pdf");
    let expected = form(&s).descendants_of("Personal.Address").count();
    assert_eq!(
        expected, 2,
        "the fixture's shape, pinned so this test is real"
    );

    let outcome = s
        .rename_field("Personal.Address", "Location")
        .expect("renaming a grouping node is allowed");
    assert_eq!(outcome.to, "Personal.Location");
    assert_eq!(
        outcome.descendants_renamed, expected,
        "the disclosure and the descendant set are one claim, not two"
    );

    // And the effect is real: the terminals' names re-derive with no object
    // of theirs written.
    let mut after = names(&form(&s));
    after.sort();
    assert_eq!(
        after,
        vec![
            "Personal.Location.City".to_owned(),
            "Personal.Location.Zip".to_owned(),
            "Personal.Name".to_owned(),
        ],
    );
}

// ---------------------------------------------------------------------------
// `AcroForm::groups` — the field-name tree's INTERIOR (Pass 53.1)
// ---------------------------------------------------------------------------
//
// `fields` is a projection of TERMINAL fields, so a pure grouping node —
// child fields, no widgets of its own, no presence and no type under Table
// 220 — is deliberately absent from it. That projection is right for every
// consumer that fills, flattens or paints.
//
// It is wrong for exactly one: the node still owns a `/T`, and renaming it
// re-derives the fully-qualified name of everything beneath it.
// `rename_field` has always accepted a grouping node's FQN
// (`FieldPath::Grouping`), so the capability existed while the NAME of the
// thing to address was unreachable from any reader.

/// **The interior nodes are reported, with their own partial names.**
#[test]
fn grouping_nodes_are_reported_with_their_own_partial_names() {
    let f = form(&session("nested-form.pdf"));

    let got: Vec<(String, String)> = f
        .groups
        .iter()
        .map(|g| {
            (
                g.fully_qualified_name.clone(),
                g.partial_name
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();

    assert_eq!(
        got,
        vec![
            ("Personal.Address".to_owned(), "Address".to_owned()),
            ("Personal".to_owned(), "Personal".to_owned()),
        ],
        "both interior nodes, each carrying its OWN /T rather than a slice of \
         a descendant's path — and DEEPEST FIRST, because a node is recorded \
         at the early return that is reached only after recursing into its \
         children; got {got:?}"
    );
}

/// **A grouping node is never also a terminal.**
///
/// The two projections partition the tree — a node is in exactly one. If
/// they ever overlapped, a shell rendering both would show the same node
/// twice and offer two Rename controls for one `/T`.
#[test]
fn the_two_projections_do_not_overlap() {
    for fixture in ["nested-form.pdf", "demo-form.pdf", "mixed-kids-form.pdf"] {
        let f = form(&session(fixture));
        for g in &f.groups {
            assert!(
                f.field_by_name(&g.fully_qualified_name).is_none(),
                "{fixture}: {:?} is reported as BOTH a grouping node and a \
                 terminal field",
                g.fully_qualified_name
            );
        }
    }
}

/// **A flat form reports NO grouping nodes.**
///
/// This is the common case — Pass 7.0's census found no corpus file nests
/// fields at all — and it is what lets a shell render nothing rather than an
/// empty section (R124). Asserted so the emptiness is a guarantee a UI can
/// rely on, not an accident of one fixture.
#[test]
fn a_flat_form_has_an_empty_group_list() {
    let f = form(&session("demo-form.pdf"));
    assert!(!f.fields.is_empty(), "the fixture does have fields");
    assert!(
        f.groups.is_empty(),
        "a flat form's field-name tree has no interior; got {:?}",
        f.groups
            .iter()
            .map(|g| &g.fully_qualified_name)
            .collect::<Vec<_>>()
    );
}

/// **A MIXED node is a terminal, not a grouping node.**
///
/// A node holding both child fields and its own widget kids has presence, so
/// `walk_field` does not take the pure-non-terminal early return and it lands
/// in `fields`. It is renameable through its terminal row like any other —
/// which is why it must NOT also appear as an interior node, or the operator
/// would meet two controls for one name.
#[test]
fn a_mixed_kids_node_is_a_terminal_and_not_a_grouping_node() {
    let f = form(&session("mixed-kids-form.pdf"));
    assert!(
        f.field_by_name("Order").is_some(),
        "the mixed node is a terminal; have {:?}",
        names(&f)
    );
    assert!(
        f.groups.is_empty(),
        "and therefore NOT an interior node; got {:?}",
        f.groups
            .iter()
            .map(|g| &g.fully_qualified_name)
            .collect::<Vec<_>>()
    );
}

/// **Every reported grouping node can actually be renamed by the name
/// reported.**
///
/// The whole point of the accessor: the string it hands out is the string
/// `rename_field` takes. If those ever diverged, a shell would render a
/// control whose every press errors.
#[test]
fn every_reported_grouping_node_is_renameable_by_that_name() {
    let mut s = session("nested-form.pdf");
    let names: Vec<String> = form(&s)
        .groups
        .iter()
        .map(|g| g.fully_qualified_name.clone())
        .collect();
    assert_eq!(names.len(), 2);

    // The deepest first, so renaming one does not invalidate the other's path.
    let outcome = s
        .rename_field("Personal.Address", "Location")
        .expect("the reported name resolves");
    assert_eq!(outcome.to, "Personal.Location");
    assert_eq!(outcome.descendants_renamed, 2);

    let outcome = s
        .rename_field("Personal", "Applicant")
        .expect("the reported name resolves");
    assert_eq!(outcome.to, "Applicant");
    assert_eq!(
        outcome.descendants_renamed, 3,
        "renaming the subtree ROOT reaches all three terminals — City and Zip \
         beneath Location, and Name directly beneath it. The count is of \
         terminals, not of tree levels."
    );

    let mut after = names_sorted(&form(&s));
    after.sort();
    assert_eq!(
        after,
        vec![
            "Applicant.Location.City".to_owned(),
            "Applicant.Location.Zip".to_owned(),
            "Applicant.Name".to_owned(),
        ],
    );
}

fn names_sorted(f: &forms::AcroForm) -> Vec<String> {
    let mut v = names(f);
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// (h) Grouping-node DELETION — `delete_field_group` and its preview.
// ---------------------------------------------------------------------------
//
// Deleting a terminal removes what the operator pointed at. Deleting a
// grouping node removes fields they did NOT name, which is why the preview
// exists and why these tests assert names rather than counts.
//
// `nested-form.pdf` is the only fixture with the shape that makes the
// interesting case testable: `Personal` holds BOTH an intermediate node
// (`Personal.Address`, with two terminals) and a terminal of its own
// (`Personal.Name`). Deleting `Personal.Address` must therefore leave
// `Personal` standing — a fixture where every terminal hung off one leaf
// node could not tell a correct ancestor-prune from one that removes every
// ancestor unconditionally.

/// Deleting an intermediate node leaves an ancestor that still has a child.
///
/// The load-bearing assertion is `Personal.Name` SURVIVING. `Personal` is an
/// ancestor of the deleted node, so a cascade that pruned ancestors without
/// checking whether they still had descendants would take it — and with it a
/// field in a different branch that the operator never named.
#[test]
fn deleting_an_intermediate_node_spares_an_ancestor_that_still_has_a_child() {
    let mut s = session("nested-form.pdf");

    let preview = s
        .field_group_deletion_preview("Personal.Address")
        .expect("Personal.Address is a grouping node");
    assert_eq!(preview.group_name, "Personal.Address");
    let mut got = preview.terminals.clone();
    got.sort();
    assert_eq!(
        got,
        vec!["Personal.Address.City", "Personal.Address.Zip"],
        "the preview must NAME the terminals, not just count them"
    );
    assert_eq!(preview.widgets_removed, 2);
    assert_eq!(
        preview.nodes_removed, 1,
        "only Address goes; Personal survives because Personal.Name remains"
    );
    // The nodes are NAMED, not just counted: a shell holding per-FQN state
    // (an open rename draft, a selection) must invalidate the intermediate
    // node too, and deriving that set itself is how it drifts from core.
    assert_eq!(
        preview.nodes,
        vec!["Personal.Address"],
        "Personal must NOT be listed — it survives"
    );

    let done = s
        .delete_field_group("Personal.Address")
        .expect("the deletion must succeed where the preview did");
    assert_eq!(done.terminals, preview.terminals);
    assert_eq!(done.nodes_removed, preview.nodes_removed);

    assert_eq!(
        names_sorted(&form(&s)),
        vec!["Personal.Name"],
        "the sibling branch must be untouched"
    );
}

/// Deleting the root grouping node takes the whole subtree, ancestors and all.
///
/// The `nodes_removed` of 2 is the assertion that distinguishes this from a
/// terminal-only sweep: `Personal` and `Personal.Address` are both objects
/// with names and no type of their own, and both must go. Leaving either
/// behind would keep its name occupying the §12.7.3.2 FQN space, refusing a
/// later field that wanted it.
#[test]
fn deleting_the_root_group_removes_every_terminal_and_every_node() {
    let mut s = session("nested-form.pdf");

    let preview = s
        .field_group_deletion_preview("Personal")
        .expect("Personal is a grouping node");
    assert_eq!(preview.terminals.len(), 3, "{:?}", preview.terminals);
    assert_eq!(preview.widgets_removed, 3);
    assert_eq!(
        preview.nodes_removed, 2,
        "Personal and Personal.Address both vanish"
    );
    // Deepest-first, with the named node last — the order a shell wants for
    // invalidation, and the order `AcroForm::groups` already uses.
    assert_eq!(preview.nodes, vec!["Personal.Address", "Personal"]);

    let done = s.delete_field_group("Personal").expect("deletion succeeds");
    assert_eq!(done.nodes_removed, 2);
    assert!(
        names_sorted(&form(&s)).is_empty(),
        "every field lived under Personal: {:?}",
        names_sorted(&form(&s))
    );
}

/// A TERMINAL's name is refused, not silently redirected to `delete_field`.
///
/// The two verbs remove different amounts. Accepting a terminal here would
/// mean a caller that mistyped a group name got a single-field deletion and
/// no signal that it had asked for something else — the sneakiness rule 4
/// forbids, on a destructive verb.
///
/// The variant matters as much as the refusal. A terminal name and an absent
/// name are opposite problems — a wrong verb on a sound document versus a
/// wrong name — and both are asserted here so a later simplification that
/// collapses them back into one has to break a test. It reached the CLI once
/// already, telling an operator that a field `list-fields` had just printed
/// did not exist.
#[test]
fn a_terminal_name_is_not_a_grouping_node() {
    let mut s = session("nested-form.pdf");
    assert!(
        matches!(
            s.field_group_deletion_preview("Personal.Name"),
            Err(pdfcer_core::edit::EditError::NotAGroupingNode { .. })
        ),
        "a terminal must be refused AS a terminal, not as a missing field"
    );
    assert!(
        matches!(
            s.field_group_deletion_preview("Nope.Nothing"),
            Err(pdfcer_core::edit::EditError::FieldNotFound { .. })
        ),
        "an absent name is still FieldNotFound"
    );
    // And neither refusal may have changed anything on the way out.
    assert_eq!(names_sorted(&form(&s)).len(), 3);
}

/// The whole subtree removal is ONE undoable command.
///
/// Three terminals, three widgets and two grouping nodes go together, so one
/// undo must bring all eight back. A per-field loop would need three undos
/// and would expose two intermediate states the operator never asked for —
/// which is the third reason `delete_field_group` is not `delete_field` in a
/// loop.
#[test]
fn deleting_a_group_undoes_as_a_single_command() {
    let mut s = session("nested-form.pdf");
    let before = names_sorted(&form(&s));

    s.delete_field_group("Personal").expect("deletion succeeds");
    assert!(names_sorted(&form(&s)).is_empty());

    s.undo().expect("one undo must restore the whole subtree");
    assert_eq!(
        names_sorted(&form(&s)),
        before,
        "a single undo must restore every terminal, widget and node"
    );
}
