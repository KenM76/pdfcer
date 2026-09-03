//! Integration tests for [`EditSession::adopt_widget`] — registering an
//! **existing** widget annotation as a form field (ISO 32000-1 §12.7.3).
//!
//! ## The fixture is a real AcroForm, and that is load-bearing
//!
//! `fixtures/external/pdfbox/.../compression/acroform.pdf` carries 12 fields
//! over 13 widgets, and — the reason it is the right subject — **both**
//! representations §12.7.3.1 permits:
//!
//! - eleven **merged field-widgets**: one dictionary that is both the field
//!   and its widget, carrying `/FT`, `/T`, `/V`, `/DA`;
//! - two **bare kids** of the `GroupOption` radio field, carrying no field
//!   keys at all — their only link to their identity is `/Parent`.
//!
//! A hand-built fixture would have exercised whichever shape the author
//! happened to write down, and the whole point of this verb is that the two
//! shapes have **different outcomes**: one adopts losslessly, the other
//! cannot be adopted at all. `examples/orphan_probe.rs` is where those
//! numbers were measured; this file pins the behaviour that follows from
//! them.
//!
//! The corpus is optional, so each test skips rather than fails when it is
//! absent — with an explicit `SKIP:` line, because a test that quietly
//! passes when it ran nothing is worse than one that fails.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::pageops::InsertPosition;
use pdfcer_core::writer::SaveOptions;

const ACROFORM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/external/pdfbox/pdfbox/src/test/resources/input/compression/acroform.pdf"
);
const BLANK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/outline/no-outline.pdf"
);

/// A session over a blank target with the AcroForm fixture's page 0 inserted
/// — i.e. **exactly the state `insert_pages` leaves a shell in**, which is
/// the situation this verb exists for.
///
/// Returns `None` when the external corpus is absent.
fn orphaned_session() -> Option<(EditSession, usize, usize)> {
    let src_bytes = std::fs::read(ACROFORM).ok()?;
    let src = Document::from_bytes(src_bytes).expect("source must parse");
    let target =
        Document::from_bytes(std::fs::read(BLANK).expect("blank target")).expect("target parses");
    let mut session = EditSession::new(target);
    let outcome = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert must succeed");
    Some((
        session,
        outcome.orphaned_widgets,
        outcome.orphaned_widgets_unrecoverable,
    ))
}

/// Every widget on the last page, in `/Annots` order, split by whether it
/// carries its own `/T`.
fn widgets(session: &EditSession) -> (Vec<ObjId>, Vec<ObjId>) {
    let view = session.view();
    let slots = session.page_slots().expect("pages");
    let last = slots.last().expect("a page");
    let Object::Dict(page) = view.resolved(last.id) else {
        panic!("page is not a dict")
    };
    let Some(Object::Array(annots)) = page.get(b"Annots").map(|a| view.resolve(a).clone()) else {
        panic!("no /Annots")
    };
    let (mut named, mut bare) = (Vec::new(), Vec::new());
    for entry in &annots {
        let Object::Reference(id) = entry else {
            continue;
        };
        let Object::Dict(d) = view.resolved(*id) else {
            continue;
        };
        if !matches!(d.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Widget") {
            continue;
        }
        if d.contains_key(b"T") {
            named.push(*id);
        } else {
            bare.push(*id);
        }
    }
    (named, bare)
}

/// A widget dictionary, cloned so it outlives the borrow of the session.
///
/// Cloned rather than borrowed because every use here is a before/after
/// comparison across a `&mut self` call, and a borrow of `session.view()`
/// cannot survive one.
fn widget_dict(session: &EditSession, id: ObjId) -> pdfcer_core::object::Dict {
    match session.view().resolved(id) {
        Object::Dict(d) => d.clone(),
        other => panic!("object {id:?} is not a dict: {other:?}"),
    }
}
/// Field names visible in the **saved** bytes, which is what any other tool
/// sees. Nothing here inspects the session overlay.
fn saved_field_names(session: &EditSession) -> Vec<String> {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("pdfcer's own output must reparse");
    match pdfcer_core::forms::parse_acroform(&doc) {
        Some(form) => form
            .fields
            .iter()
            .map(|f| f.fully_qualified_name.clone())
            .collect(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// The measured premise
// ---------------------------------------------------------------------------

/// Would catch: the fixture changing shape under these tests, or
/// `orphaned_widgets_unrecoverable` drifting from what it counts.
///
/// Every other test here rests on "11 adoptable, 2 not", so that claim gets
/// asserted once, explicitly, rather than being an assumption spread across
/// the file. If this fails, the others are testing something else.
#[test]
fn the_fixture_carries_both_widget_shapes_and_the_counts_agree() {
    let Some((session, orphaned, unrecoverable)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    assert_eq!(orphaned, 13, "13 widgets came across");
    assert_eq!(
        unrecoverable, 2,
        "the two GroupOption radio kids lost their identity"
    );
    let (named, bare) = widgets(&session);
    assert_eq!(named.len(), 11, "merged field-widgets");
    assert_eq!(bare.len(), 2, "bare kids");
    assert_eq!(
        unrecoverable,
        bare.len(),
        "the counter must count exactly the widgets that cannot be adopted"
    );
    assert!(
        saved_field_names(&session).is_empty(),
        "insert_pages must not have merged /AcroForm — that is the premise"
    );
}

// ---------------------------------------------------------------------------
// Adoption
// ---------------------------------------------------------------------------

/// Would catch: adoption registering the field in the session but not in the
/// saved bytes, or registering it under the wrong name.
///
/// Asserted through `parse_acroform` over a **reparsed save**, because that
/// is the only view another tool has. A field that exists in the overlay and
/// not in the file is the failure this verb is most likely to have.
#[test]
fn a_merged_field_widget_adopts_losslessly() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    let widget = named[0];

    let out = session
        .adopt_widget(widget, None)
        .expect("a merged field-widget must adopt");
    assert_eq!(out.field_id, widget, "a merged field-widget IS its field");
    assert_eq!(out.name, "TextField");
    assert_eq!(out.field_type.as_deref(), Some("Tx"));
    assert!(!out.renamed, "no name was supplied, so nothing was renamed");
    assert!(
        out.acroform_created,
        "the blank target had no /AcroForm before this"
    );

    assert_eq!(saved_field_names(&session), vec!["TextField".to_owned()]);
}

/// Would catch: adoption clobbering the widget's existing appearance,
/// geometry or value — the thing the verb exists to avoid.
///
/// `add_text_field` authors a new widget; this must author nothing. So the
/// widget's dictionary is compared key-for-key before and after, and the ONLY
/// permitted difference is nothing at all when no rename was asked for.
#[test]
fn adoption_writes_no_geometry_appearance_or_value() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    let widget = named[0];
    let before = widget_dict(&session, widget);

    session.adopt_widget(widget, None).expect("must adopt");

    let after = widget_dict(&session, widget);
    assert_eq!(
        before, after,
        "adopting must not touch the widget's own dictionary at all"
    );
}

/// Would catch: a rename being reported but not written, or written but not
/// reported. Both produce a document whose field name disagrees with what the
/// shell told the operator.
#[test]
fn a_rename_is_both_written_and_reported() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    let out = session
        .adopt_widget(named[0], Some("Customer Name"))
        .expect("must adopt");
    assert!(out.renamed);
    assert_eq!(out.name, "Customer Name");
    assert_eq!(
        saved_field_names(&session),
        vec!["Customer Name".to_owned()],
        "the name in the FILE must be the name that was reported"
    );
}

/// Would catch: a second adoption dropping the first, which is what happens
/// if `/Fields` is overwritten rather than appended to — and it is invisible
/// until the second call.
#[test]
fn adopting_several_widgets_accumulates_rather_than_replacing() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    for w in named.iter().take(4) {
        session.adopt_widget(*w, None).expect("must adopt");
    }
    let names = saved_field_names(&session);
    assert_eq!(names.len(), 4, "all four must survive, got {names:?}");
    assert!(names.contains(&"TextField".to_owned()));
    assert!(names.contains(&"CheckBox1".to_owned()));
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Would catch: a bare kid being adopted under an invented name.
///
/// This is the refusal the whole design turns on. A kid carries no `/T`, so
/// anything pdfcer wrote there would be a name the source never used — and for
/// a radio group, adopting the two kids separately produces two independent
/// check boxes where there was one mutually-exclusive field. The form looks
/// right and behaves wrong, which is the worst available outcome.
#[test]
fn a_bare_kid_widget_is_refused_rather_than_named() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (_, bare) = widgets(&session);
    assert_eq!(bare.len(), 2, "fixture premise");
    for w in &bare {
        match session.adopt_widget(*w, None) {
            Err(EditError::WidgetHasNoFieldIdentity { id }) => assert_eq!(id, w.num),
            other => panic!("a bare kid must be refused, got {other:?}"),
        }
    }
    assert!(
        saved_field_names(&session).is_empty(),
        "a refusal must register nothing"
    );

    // With an explicit name the operator HAS chosen, it is allowed — the
    // refusal is about pdfcer guessing, not about the widget being unusable.
    let out = session
        .adopt_widget(bare[0], Some("RadioA"))
        .expect("an explicitly named kid must adopt");
    assert!(out.renamed);
    assert_eq!(saved_field_names(&session), vec!["RadioA".to_owned()]);
}

/// Would catch: a name collision being allowed through.
///
/// §12.7.3.1 makes the fully qualified name the field's *identity*, so two
/// top-level fields called `TextField` are one field with two widgets —
/// filling either fills both. No viewer reports it; the operator finds it by
/// typing. That makes silent acceptance the worst outcome and a loud refusal
/// the right one.
#[test]
fn a_colliding_name_is_refused() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    session.adopt_widget(named[0], None).expect("first adopts");

    match session.adopt_widget(named[1], Some("TextField")) {
        Err(EditError::FieldNameTaken { name }) => assert_eq!(name, "TextField"),
        other => panic!("a colliding name must be refused, got {other:?}"),
    }
    assert_eq!(
        saved_field_names(&session).len(),
        1,
        "the refused adoption must leave nothing behind"
    );
}

/// Would catch: double-adoption listing one widget twice in `/Fields`, which
/// produces a form that reports more fields than it has controls.
#[test]
fn adopting_the_same_widget_twice_is_refused() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    session.adopt_widget(named[0], None).expect("first adopts");
    match session.adopt_widget(named[0], None) {
        Err(EditError::WidgetAlreadyOwned { id }) => assert_eq!(id, named[0].num),
        other => panic!("a second adoption must be refused, got {other:?}"),
    }
    assert_eq!(saved_field_names(&session).len(), 1);
}

/// Would catch: a non-widget object being accepted. A page and a plain
/// annotation are both dictionaries on the same page, so nothing about the
/// call site distinguishes them from a widget.
#[test]
fn a_non_widget_is_refused() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let page = session.page_slots().expect("pages")[0].id;
    match session.adopt_widget(page, Some("NotAField")) {
        Err(EditError::NotAWidget { id }) => assert_eq!(id, page.num),
        other => panic!("a page must be refused, got {other:?}"),
    }
    match session.adopt_widget(ObjId::new(99_999, 0), Some("Nothing")) {
        Err(EditError::NotAWidget { .. }) => {}
        other => panic!("a missing object must be refused, got {other:?}"),
    }
    assert!(saved_field_names(&session).is_empty());
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

/// Would catch: undo removing the registration but leaving the rename, so the
/// widget keeps a name the operator undid.
#[test]
fn undo_removes_the_registration_and_the_rename_together() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, _) = widgets(&session);
    let widget = named[0];
    let before = widget_dict(&session, widget);

    session
        .adopt_widget(widget, Some("Renamed"))
        .expect("must adopt");
    assert_eq!(saved_field_names(&session), vec!["Renamed".to_owned()]);

    assert!(session.undo().is_some(), "one undo entry must exist");
    assert!(
        saved_field_names(&session).is_empty(),
        "the registration must be gone"
    );
    let after = widget_dict(&session, widget);
    assert_eq!(
        before, after,
        "and so must the rename — the widget must be exactly as it arrived"
    );
}

// ---------------------------------------------------------------------------
// `/FT` is inheritable and `/T` is not — the distinction the count turns on
// ---------------------------------------------------------------------------

/// Byte-author a minimal PDF, so a shape no corpus file happens to contain
/// can still be tested. Same construction as `tests/page_ops.rs`.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// Would catch: `orphaned_widgets_unrecoverable` testing `/FT` instead of
/// `/T`.
///
/// ## Why this needs a hand-built fixture, and why the sabotage battery
/// demanded one
///
/// `orphan_probe`'s real AcroForm cannot distinguish the two predicates: its
/// bare radio kids carry **neither** `/FT` nor `/T`, so counting by either
/// gives 2. Swapping the key in the implementation left the entire suite
/// green — the code's own comment argued for `/T` on the grounds that
/// §12.7.3.1 makes `/FT` **inheritable**, and that argument was reasoned but
/// completely unmeasured.
///
/// This document supplies the case that separates them: three widgets on one
/// page, of which
///
/// - `Named` carries `/T` **and** `/FT` — adoptable, not counted;
/// - `TypeOnly` carries `/FT` and **no** `/T` — a kid that inherits nothing
///   useful; `/FT` tells a viewer how to *draw* it but there is still no name
///   to fill, export or refer to it by, so it **must** be counted;
/// - `Naked` carries neither — counted, and the case the real corpus covers.
///
/// Expected count is therefore **2**, and counting by `/FT` yields **1**.
///
/// The wider point, worth keeping: a field with no name is unusable no matter
/// how much type information survives, because §12.7.3.1 makes the fully
/// qualified name the field's identity. `/FT` surviving is not partial
/// recovery — it is decoration on something that cannot be addressed.
#[test]
fn a_widget_with_ft_but_no_t_is_still_unrecoverable() {
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R 6 0 R] >>",
        ),
        (
            4,
            "<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] /FT /Tx /T (Named) >>",
        ),
        (
            5,
            "<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] /FT /Btn >>",
        ),
        (6, "<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] >>"),
    ]);
    let src = Document::from_bytes(source).expect("hand-built source must parse");
    let target =
        Document::from_bytes(std::fs::read(BLANK).expect("blank target")).expect("target parses");
    let mut session = EditSession::new(target);

    let outcome = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert must succeed");
    assert_eq!(outcome.orphaned_widgets, 3);
    assert_eq!(
        outcome.orphaned_widgets_unrecoverable, 2,
        "a widget with /FT but no /T has no NAME, so it is unrecoverable — \
counting by /FT would say 1"
    );

    // And the verb agrees with the counter, which is the property that makes
    // the number actionable: exactly the widgets it counts are the ones
    // `adopt_widget` refuses.
    let (named, bare) = widgets(&session);
    assert_eq!(named.len(), 1);
    assert_eq!(bare.len(), 2);
    for w in &bare {
        assert!(
            matches!(
                session.adopt_widget(*w, None),
                Err(EditError::WidgetHasNoFieldIdentity { .. })
            ),
            "widget {w:?} is counted as unrecoverable, so it must also be refused"
        );
    }
    assert!(session.adopt_widget(named[0], None).is_ok());
}

// ---------------------------------------------------------------------------
// Page labels (§12.4.2) — `Pass 103.2`
// ---------------------------------------------------------------------------

/// A document with a `/PageLabels` number tree over `pages` pages, labelled
/// `i, ii, iii…` — a shape byte-authored because no fixture carries one.
fn labelled_doc(pages: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut objects: Vec<(u32, String)> = vec![
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /r >>] >> >>".to_owned(),
        ),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {pages} /MediaBox [0 0 200 100] \
/Resources << >> >>",
                kids.join(" ")
            ),
        ),
    ];
    for i in 0..pages {
        objects.push((
            u32::try_from(i + 3).expect("small"),
            "<< /Type /Page /Parent 2 0 R >>".to_owned(),
        ));
    }
    let refs: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build(&refs)
}

/// A document with no `/PageLabels` at all.
fn unlabelled_doc(pages: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {pages} /MediaBox [0 0 200 100] \
/Resources << >> >>",
                kids.join(" ")
            ),
        ),
    ];
    for i in 0..pages {
        objects.push((
            u32::try_from(i + 3).expect("small"),
            "<< /Type /Page /Parent 2 0 R >>".to_owned(),
        ));
    }
    let refs: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build(&refs)
}

fn insert(target: Vec<u8>, source: Vec<u8>) -> (EditSession, pdfcer_core::edit::InsertOutcome) {
    let src = Document::from_bytes(source).expect("source must parse");
    let mut session = EditSession::new(Document::from_bytes(target).expect("target must parse"));
    let outcome = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert must succeed");
    (session, outcome)
}

/// Would catch: the two page-label facts being conflated, or either being
/// reported when it is not true.
///
/// They are separate because the operator's next action differs — a stale
/// tree wants renumbering, a dropped one wants creating — so all four
/// combinations of "source has labels" × "target has labels" are checked
/// rather than the one case that happens to be handy.
#[test]
fn the_two_page_label_facts_are_reported_independently() {
    let cases = [
        // (target labelled, source labelled, expect_dropped, expect_stale)
        (true, true, true, true),
        (true, false, false, true),
        (false, true, true, false),
        (false, false, false, false),
    ];
    for (t, s, want_dropped, want_stale) in cases {
        let target = if t {
            labelled_doc(2)
        } else {
            unlabelled_doc(2)
        };
        let source = if s {
            labelled_doc(1)
        } else {
            unlabelled_doc(1)
        };
        let (_, outcome) = insert(target, source);
        assert_eq!(
            outcome.source_page_labels_dropped, want_dropped,
            "target_labelled={t} source_labelled={s}: dropped flag"
        );
        assert_eq!(
            outcome.page_labels_stale, want_stale,
            "target_labelled={t} source_labelled={s}: stale flag"
        );
    }
}

/// Would catch: pdfcer acquiring Acrobat's behaviour — writing a label for the
/// inserted range.
///
/// `Acrobat_Features/core_ops__page_labels_and_bates_interaction.md` records
/// that Acrobat overwrites every inserted page with a static copy of the
/// label on the target page preceding the insertion point: a twelve-page
/// chapter labelled `10-1`…`10-12`, inserted after a page labelled `9-45`,
/// came out with all twelve showing `9-45`. That is a wrong label on every
/// inserted page, written silently, and the threads documenting it are
/// complaints.
///
/// So this asserts the target's `/PageLabels` tree is **byte-identical**
/// after the insert. Not "still present" — identical, because a writer that
/// appended a new static range for the inserted pages would leave it present
/// and changed, and that is precisely the behaviour being refused.
#[test]
fn an_insert_does_not_touch_the_targets_page_label_tree() {
    let target = labelled_doc(2);
    let before = {
        let doc = Document::from_bytes(target.clone()).expect("parse");
        let Some(Object::Dict(catalog)) = doc
            .catalog_id()
            .and_then(|id| doc.get(id).map(|io| &io.value))
        else {
            panic!("no catalog")
        };
        catalog.get(b"PageLabels").map(|o| doc.resolve(o).clone())
    };
    assert!(before.is_some(), "fixture premise: the target IS labelled");

    let (session, outcome) = insert(target, labelled_doc(1));
    assert!(outcome.source_page_labels_dropped);
    assert!(outcome.page_labels_stale);

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("pdfcer's own output must reparse");
    let Some(Object::Dict(catalog)) = doc
        .catalog_id()
        .and_then(|id| doc.get(id).map(|io| &io.value))
    else {
        panic!("no catalog")
    };
    let after = catalog.get(b"PageLabels").map(|o| doc.resolve(o).clone());
    assert_eq!(
        after, before,
        "the label tree must be untouched — pdfcer does not write a label it \
cannot justify, and specifically does not copy the anchor page's label onto \
the inserted range the way Acrobat does"
    );
}

// ---------------------------------------------------------------------------
// `adopt_preview` — `Pass 103.4`
// ---------------------------------------------------------------------------

/// Would catch: the preview and the call disagreeing.
///
/// ## The only property that matters, and why it is asserted as an identity
///
/// A preview exists so a UI can grey a control instead of failing late. Its
/// entire value rests on it giving the **same answer** the call will give —
/// a preview that says "this will work" before a refusal is worse than no
/// preview, because the shell has already told the operator it is available.
///
/// So this does not check the preview against hand-written expectations. It
/// checks it against `adopt_widget` itself, over **every widget on the page**
/// — both shapes, named and unnamed, first-adoption and collision — and
/// requires the two to match exactly in both the `Ok` and the `Err` arm.
///
/// They share one body (`adopt_plan`), so this is currently true by
/// construction. That is the point: this test is what notices if someone
/// later gives the preview its own implementation.
#[test]
fn the_preview_and_the_call_always_agree() {
    let Some((session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, bare) = widgets(&session);
    let mut all: Vec<ObjId> = named.clone();
    all.extend(bare.iter().copied());

    for widget in &all {
        for name in [None, Some("Chosen"), Some("")] {
            // A FRESH session per probe: `adopt_widget` mutates, and a
            // preview taken against a session that has already adopted
            // something is answering a different question.
            let Some((mut s, ..)) = orphaned_session() else {
                return;
            };
            let previewed = s.adopt_preview(*widget, name);
            let called = s.adopt_widget(*widget, name);
            match (&previewed, &called) {
                (Ok(p), Ok(c)) => assert_eq!(
                    p, c,
                    "widget {widget:?} name={name:?}: preview and call must return the \
same outcome"
                ),
                (Err(p), Err(c)) => assert_eq!(
                    p.to_string(),
                    c.to_string(),
                    "widget {widget:?} name={name:?}: preview and call must refuse for \
the same reason"
                ),
                _ => panic!(
                    "widget {widget:?} name={name:?}: preview and call disagreed on \
whether it works at all — preview={previewed:?} call={called:?}"
                ),
            }
        }
    }
}

/// Would catch: the preview writing something — **as a regression guard on
/// the signature, not as a live check**, and the distinction is stated because
/// a sabotage run is what established it.
///
/// Under `&self`, and with no interior mutability anywhere in `EditSession`,
/// this cannot fail today. Every mutating path — `commit`, `stage_bytes`,
/// `alloc_number` — needs `&mut self`, so making the preview write is not a
/// bug this test catches but a **whole-workspace compile error**. The
/// compiler is the stronger guarantee and it is already in force.
///
/// What this defends is the *next* change: a preview given `&mut self` for
/// some unrelated convenience, or an `EditSession` that acquires a `Cell` or
/// `RefCell`. Both would restore the possibility this asserts against, and
/// neither would look like it was touching previews.
///
/// Recorded plainly because the first attempt to sabotage it was worthless —
/// `let _ = &objects;` mutates nothing — and reading "suite stayed green" as
/// a verdict on the test rather than on the sabotage is a mistake made three
/// times in one session before it was named.
#[test]
fn the_preview_writes_nothing() {
    let Some((session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let before = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed")
        .0;

    let (named, bare) = widgets(&session);
    for w in named.iter().chain(bare.iter()) {
        let _ = session.adopt_preview(*w, None);
        let _ = session.adopt_preview(*w, Some("Whatever"));
    }

    let after = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed")
        .0;
    assert_eq!(
        before, after,
        "previewing must not change a single byte of what a save would produce"
    );
    assert!(
        saved_field_names(&session).is_empty(),
        "and must register nothing"
    );
}

/// Would catch: the preview failing to distinguish the two shapes — which is
/// the entire reason it was asked for.
///
/// `pdfcer-gui`'s row has to look different for a merged field-widget (one
/// button, *"Register as `Address`"*) and for a bare kid (a **required** name
/// box, labelled as *creating* a field rather than restoring one). This pins
/// that the preview supplies exactly the facts that distinction needs.
#[test]
fn the_preview_separates_the_two_widget_shapes_before_any_press() {
    let Some((session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, bare) = widgets(&session);

    // A merged field-widget: previewable with no name, and it hands back the
    // name that is in the FILE and not on screen.
    let out = session
        .adopt_preview(named[0], None)
        .expect("a merged field-widget must preview clean");
    assert_eq!(out.name, "TextField");
    assert_eq!(out.field_type.as_deref(), Some("Tx"));
    assert!(!out.renamed);

    // A bare kid: refuses with no name, so the shell knows the box is
    // REQUIRED before the operator types.
    assert!(
        matches!(
            session.adopt_preview(bare[0], None),
            Err(EditError::WidgetHasNoFieldIdentity { .. })
        ),
        "a bare kid must refuse in the preview, not only in the call"
    );
    // And with a name it previews clean, so the shell can label the row as
    // creating rather than restoring.
    let out = session
        .adopt_preview(bare[0], Some("RadioA"))
        .expect("a named bare kid must preview clean");
    assert_eq!(out.name, "RadioA");
    assert!(out.renamed);
}

/// Would catch: `adopt_preview(..).err()` not being usable as the refusal
/// predicate the request asked for.
///
/// `adopt_refusal(..) -> Option<EditError>` was requested as ask 1 and
/// deliberately not shipped, on the grounds that the preview subsumes it. That
/// is only true if the substitution actually works, so it is tested rather
/// than asserted in a doc comment — the substitution is stated, and here it is
/// measured.
#[test]
fn the_preview_doubles_as_the_refusal_predicate() {
    let Some((mut session, ..)) = orphaned_session() else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let (named, bare) = widgets(&session);

    assert!(session.adopt_preview(named[0], None).err().is_none());
    assert!(matches!(
        session.adopt_preview(bare[0], None).err(),
        Some(EditError::WidgetHasNoFieldIdentity { .. })
    ));

    // And it tracks state: once a name is taken, the predicate says so.
    session.adopt_widget(named[0], None).expect("adopts");
    assert!(matches!(
        session.adopt_preview(named[1], Some("TextField")).err(),
        Some(EditError::FieldNameTaken { .. })
    ));
    assert!(matches!(
        session.adopt_preview(named[0], None).err(),
        Some(EditError::WidgetAlreadyOwned { .. })
    ));
}

/// Would catch: `source_outline_dropped` reporting the wrong document's
/// outline, or being conflated with the page-label flag.
///
/// Requested as pure symmetry with `source_page_labels_dropped`, for the same
/// reason: the shell's insert sentence said *"Bookmarks and page labels from
/// that file did not come across"* unconditionally, and on a CAD drawing with
/// neither that is a paragraph about two things that never existed.
///
/// All four combinations, because a flag that reads the wrong catalog passes
/// any test where both documents happen to agree.
#[test]
fn the_source_outline_flag_is_independent_of_the_page_label_one() {
    let outlined = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/outline/basic-tree.pdf"
    );
    let bare = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/outline/no-outline.pdf"
    );
    let bytes = |p: &str| std::fs::read(p).expect("fixture");

    // source has an outline, target does not -> reported
    let (_, out) = insert(bytes(bare), bytes(outlined));
    assert!(
        out.source_outline_dropped,
        "the source's bookmarks are gone"
    );
    assert!(
        !out.source_page_labels_dropped,
        "neither fixture has page labels — the two flags must not move together"
    );

    // source has no outline -> not reported, even though the TARGET has one
    let (_, out) = insert(bytes(outlined), bytes(bare));
    assert!(
        !out.source_outline_dropped,
        "nothing was dropped; the target's own outline is untouched and is not \
this flag's subject"
    );
}
