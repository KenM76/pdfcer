//! `EditSession::merge_document` (`Pass 106.0`) — a merge that preserves the
//! undo log, and carries the field tree `insert_pages` must leave behind.
//!
//! ★ **Commit `9f663f9` calls this "Pass 104.0" and is wrong.** `104.0` was
//! already taken by the ce-dimension group verbs, shipped the same day. The
//! ID was asserted from memory rather than grepped against the ledger, and
//! the collision was caught at filing time. The commit subject cannot be
//! rewritten; `ROADMAP.md` is authoritative.
//!
//! # What these are actually testing
//!
//! Not "does a merge happen". The two things that make this verb worth having
//! over the two merges that already existed:
//!
//! 1. **The session survives.** `pageops::insert` merges everything and
//!    returns a whole new document's bytes, so wiring it into an open editor
//!    discards the undo log. `pdfcer-gui` left their Merge button inert rather
//!    than ship that. So: undo must work, and must reverse the whole merge.
//! 2. **The arriving fields are FILLABLE.** `insert_pages` brings widgets and
//!    not their fields, giving *"boxes that draw exactly like form fields,
//!    that an operator will click on, and that nothing can fill."* A merge
//!    that produced the same thing plus more pages would be no better.
//!
//! Both are asserted through **saved bytes reparsed**, because that is the
//! only view another tool has, and `parse_acroform` is independent code — a
//! writer bug and a reader bug would have to agree to hide.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
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

fn doc(path: &str) -> Option<Document> {
    Document::from_bytes(std::fs::read(path).ok()?).ok()
}

fn blank_session() -> EditSession {
    EditSession::new(doc(BLANK).expect("the blank fixture is in-repo"))
}

/// Field names visible in the **saved** bytes.
fn saved_fields(session: &EditSession) -> Vec<String> {
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

/// Every field in the saved bytes, with how many widgets each claims.
fn saved_field_widget_counts(session: &EditSession) -> Vec<(String, usize)> {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("reparse");
    match pdfcer_core::forms::parse_acroform(&doc) {
        Some(form) => form
            .fields
            .iter()
            .map(|f| (f.fully_qualified_name.clone(), f.widgets.len()))
            .collect(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// The two properties the verb exists for
// ---------------------------------------------------------------------------

/// Would catch: a merge that carries pages and leaves the fields behind —
/// i.e. `insert_pages` with a different name.
///
/// The source has **12 fields over 13 widgets**. `insert_pages` on the same
/// file produces 13 widgets and **no `/AcroForm` at all**. This must produce
/// 12 fields, every one of them claiming its widgets, because a field that
/// claims no widget is a name in a list rather than something an operator can
/// type into.
#[test]
fn a_merged_form_arrives_fillable_not_as_orphaned_boxes() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let mut session = blank_session();
    assert!(
        saved_fields(&session).is_empty(),
        "premise: the target starts with no form"
    );

    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge must succeed");
    assert_eq!(out.pages_merged, 1);
    assert_eq!(out.fields_merged, 12, "every source field must come across");
    assert_eq!(out.fields_renamed, 0, "nothing to collide with");
    assert!(out.acroform_created, "the blank target had no /AcroForm");

    let counts = saved_field_widget_counts(&session);
    assert_eq!(counts.len(), 12, "in the SAVED bytes, not just the session");
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    assert_eq!(
        total, 13,
        "all 13 widgets must be claimed — the count insert_pages reports as ORPHANED"
    );
    for (name, widgets) in &counts {
        assert!(
            *widgets > 0,
            "field {name:?} claims no widget, so nothing can be typed into it"
        );
    }

    // The radio group is the shape that proves re-parenting worked: two
    // widgets under one field, which is only reachable through /Parent.
    assert!(
        counts.iter().any(|(n, w)| n == "GroupOption" && *w == 2),
        "the two-widget radio group must survive as ONE field: {counts:?}"
    );
}

/// Would catch: the merge being unundoable, or undo removing the pages and
/// leaving the field tree — which is the failure `pageops::insert` avoids by
/// not having a session at all, and the one this verb could plausibly ship.
#[test]
fn one_undo_reverses_the_whole_merge_pages_and_fields_together() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let mut session = blank_session();
    let pages_before = session.page_slots().expect("pages").len();
    let bytes_before = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;

    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge must succeed");
    assert_eq!(saved_fields(&session).len(), 12);

    assert!(
        session.undo().is_some(),
        "the merge must be exactly ONE undo entry"
    );
    assert_eq!(
        session.page_slots().expect("pages").len(),
        pages_before,
        "the pages must be gone"
    );
    assert!(
        saved_fields(&session).is_empty(),
        "and so must the field tree — undoing half a merge is worse than not undoing"
    );
    assert_eq!(
        session
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("save")
            .0,
        bytes_before,
        "the document must be byte-identical to before the merge"
    );
}

// ---------------------------------------------------------------------------
// Collisions
// ---------------------------------------------------------------------------

/// Would catch: a colliding field name being merged rather than renamed.
///
/// §12.7.3.1 makes the fully qualified name the field's **identity**, so two
/// top-level fields called `TextField` are not two fields — they are one
/// field with two widgets, and **filling either fills both**. Merging the
/// same document into itself is the sharpest form of the test: every one of
/// the 12 names collides.
///
/// Renaming rather than refusing is the deliberate difference from
/// `adopt_widget`, which refuses. Adopting one widget is a decision an
/// operator is making now and can be asked about; merging a 12-field document
/// is not, and refusing the whole merge over one name is worse than a suffix.
#[test]
fn merging_a_document_into_itself_renames_every_collision() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("first merge");
    let first = saved_fields(&session);
    assert_eq!(first.len(), 12);

    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("second merge");
    assert_eq!(out.fields_merged, 12);
    assert_eq!(
        out.fields_renamed, 12,
        "every name from the first merge is taken, so every arrival must be renamed"
    );
    assert!(!out.acroform_created, "the /AcroForm already existed");

    let all = saved_fields(&session);
    assert_eq!(all.len(), 24, "24 distinct fields, not 12 doubled: {all:?}");

    // The identity property, stated as the test rather than assumed: no two
    // fields may share a fully qualified name.
    let mut sorted = all.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        before,
        "two fields share a name, so filling one fills the other: {all:?}"
    );
    assert!(
        all.iter().any(|n| n == "TextField"),
        "the original keeps its name"
    );
    assert!(
        all.iter().any(|n| n == "TextField_2"),
        "and the arrival is suffixed: {all:?}"
    );
}

/// Would catch: `/NeedAppearances` being overwritten rather than OR-ed.
///
/// It means *"the appearance streams in this file may be stale, regenerate
/// them"*. If either document says so, the merged document must — otherwise
/// the arriving fields render from appearances their own producer already
/// declared untrustworthy, and they render **plausibly**, which is why this
/// is not self-correcting.
#[test]
fn need_appearances_is_carried_as_a_logical_or() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let src_needs = src
        .catalog_id()
        .and_then(|id| src.get(id).map(|io| &io.value))
        .and_then(Object::as_dict)
        .and_then(|c| c.get(b"AcroForm").map(|o| src.resolve(o).clone()))
        .and_then(|o| o.as_dict().cloned())
        .map(|f| matches!(f.get(b"NeedAppearances"), Some(Object::Boolean(true))))
        .unwrap_or(false);

    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let out = Document::from_bytes(bytes).expect("reparse");
    let merged_needs = pdfcer_core::forms::parse_acroform(&out)
        .map(|f| f.need_appearances)
        .unwrap_or(false);
    assert_eq!(
        merged_needs, src_needs,
        "the merged document must inherit the source's NeedAppearances, not reset it"
    );
}

/// Would catch: merging an empty or form-less document exploding, or claiming
/// to have done something.
#[test]
fn merging_a_form_less_document_is_a_plain_page_merge() {
    let Some(src) = doc(BLANK) else { return };
    let mut session = blank_session();
    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge must succeed");
    assert_eq!(out.pages_merged, 1);
    assert_eq!(out.fields_merged, 0);
    assert_eq!(out.fields_renamed, 0);
    assert!(
        !out.acroform_created,
        "a document with no fields must not gain an empty /AcroForm — that \
would make a non-form document report as a form"
    );
    assert!(saved_fields(&session).is_empty());
}

// ---------------------------------------------------------------------------
// The properties the reader cannot see — added after a sabotage run
// ---------------------------------------------------------------------------

/// The page's `/Annots` ids and every field's `/Kids` ids, from the saved
/// bytes. Raw, because the questions below are invisible to `parse_acroform`.
fn saved_annot_and_kid_ids(session: &EditSession) -> (Vec<ObjId>, Vec<ObjId>) {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("reparse");

    let mut annots = Vec::new();
    for slot in pdfcer_core::page_tree::page_slots(&doc).expect("pages") {
        let Object::Dict(page) = doc.resolved(slot.id) else {
            continue;
        };
        if let Some(Object::Array(a)) = page.get(b"Annots").map(|o| doc.resolve(o).clone()) {
            annots.extend(a.iter().filter_map(Object::as_reference));
        }
    }

    let mut kids = Vec::new();
    let form = doc
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm").map(|o| doc.resolve(o).clone()))
        .and_then(|o| o.as_dict().cloned());
    if let Some(form) = form
        && let Some(Object::Array(fields)) = form.get(b"Fields").map(|o| doc.resolve(o).clone())
    {
        for f in &fields {
            let Some(id) = f.as_reference() else { continue };
            let Object::Dict(field) = doc.resolved(id) else {
                continue;
            };
            match field.get(b"Kids").map(|o| doc.resolve(o).clone()) {
                Some(Object::Array(k)) => kids.extend(k.iter().filter_map(Object::as_reference)),
                // A merged field with no /Kids is a MERGED field-widget: it
                // is its own widget, so it is its own kid for this purpose.
                _ => kids.push(id),
            }
        }
    }
    (annots, kids)
}

/// Would catch: importing the field tree with a **fresh** mapping, which
/// duplicates every widget.
///
/// ## Why the obvious assertion misses it
///
/// `a_merged_form_arrives_fillable_not_as_orphaned_boxes` sums
/// `field.widgets.len()` and gets 13 either way — because with a second
/// mapping the fields get 13 brand-new widget copies, and `parse_acroform`
/// walks *down* from `/Fields` and counts those. The 13 the **pages** carry
/// become invisible orphans, and the document holds 26 widget objects where
/// it should hold 13.
///
/// Sabotaging the shared mapping left the whole file green. So this asserts
/// **object identity**: the widgets the fields claim must be the very objects
/// the pages reference, not equal-looking copies.
#[test]
fn the_fields_claim_the_same_widget_objects_the_pages_reference() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (annots, kids) = saved_annot_and_kid_ids(&session);
    assert_eq!(annots.len(), 13, "premise: 13 widgets arrived on the page");
    assert_eq!(kids.len(), 13, "and the fields must claim 13, not 26");

    let mut a = annots.clone();
    let mut k = kids.clone();
    a.sort_by_key(|i| i.num);
    k.sort_by_key(|i| i.num);
    assert_eq!(
        a, k,
        "the fields must claim the SAME objects the pages reference — equal \
counts of DIFFERENT objects means every widget was duplicated and half of \
them are orphans no field can reach"
    );
}

/// Would catch: not writing `/Parent` back onto each merged widget.
///
/// ## Why every other test in this file is blind to it
///
/// `parse_acroform` walks **downward** — `/Fields` → `/Kids` → widget — and
/// never reads `/Parent` at all. So a merge that produced fields claiming
/// widgets, with no widget claiming a field, passes every assertion routed
/// through it, including the widget counts and the radio-group check.
/// Deleting the `/Parent` write left the file green.
///
/// ## Why `/Parent` nonetheless matters
///
/// §12.7.3.2 makes `/FT`, `/Ff`, `/V` and `/DA` **inheritable**, and
/// inheritance is resolved by walking *up*. A viewer that hit-tests a click
/// lands on the **widget** and must find its field from there. Without
/// `/Parent` the control draws, accepts a click, and belongs to nothing —
/// which is precisely the orphaned-widget failure this whole verb exists to
/// avoid, reproduced one level further in.
///
/// This is the third property today whose only witness is the raw bytes
/// (after `/Prev` and `/QuadPoints`). The pattern is worth naming: **a reader
/// that normalises or ignores a field cannot be the oracle for a writer that
/// sets it.**
#[test]
fn every_merged_widget_points_back_at_its_field() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let out = Document::from_bytes(bytes).expect("reparse");
    let form = out
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm").map(|o| out.resolve(o).clone()))
        .and_then(|o| o.as_dict().cloned())
        .expect("the merged document must have an /AcroForm");
    let Some(Object::Array(fields)) = form.get(b"Fields").map(|o| out.resolve(o).clone()) else {
        panic!("no /Fields")
    };

    let mut checked = 0usize;
    for f in &fields {
        let Some(field_id) = f.as_reference() else {
            continue;
        };
        let Object::Dict(field) = out.resolved(field_id) else {
            continue;
        };
        let Some(Object::Array(kids)) = field.get(b"Kids").map(|o| out.resolve(o).clone()) else {
            continue; // merged field-widget: no kids, nothing to point back
        };
        for kid in &kids {
            let Some(kid_id) = kid.as_reference() else {
                continue;
            };
            let Object::Dict(widget) = out.resolved(kid_id) else {
                panic!("kid {kid_id:?} is not a dictionary")
            };
            assert_eq!(
                widget.get(b"Parent").and_then(Object::as_reference),
                Some(field_id),
                "widget {kid_id:?} does not point back at its field {field_id:?}; \
a viewer hit-testing a click on it would find no field, so /FT, /Ff, /V and \
/DA cannot inherit"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no kid widgets were checked — the fixture must contain at least the \
two-widget radio group, or this test proves nothing"
    );
}

/// Would catch: the merge being labelled as an insert in the undo log.
///
/// Not cosmetic. Undoing a merge removes the pages **and** the merged field
/// tree; an entry reading "insert pages" understates what is about to be
/// reversed, and an operator deciding whether to undo is reading exactly that
/// label. The sabotage that swapped the two `CommandKind`s left every other
/// test green, because the behaviour is identical and only the name differs.
#[test]
fn the_undo_entry_says_merge_not_insert() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");
    assert!(
        matches!(
            session.undo_kind(),
            Some(pdfcer_core::edit::CommandKind::MergeDocument { count: 1 })
        ),
        "the undo entry must name the merge and its page count, got {:?}",
        session.undo_kind()
    );
}

/// Byte-author a minimal PDF, so a shape no corpus file happens to contain can
/// still be tested. Same construction as `tests/page_ops.rs`.
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

/// A one-page form whose `/AcroForm` sets `/NeedAppearances true`.
fn needs_appearances_doc() -> Vec<u8> {
    build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
             /NeedAppearances true >> >>",
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Annots [4 0 R] >>"),
        (
            4,
            "<< /Type /Annot /Subtype /Widget /Rect [10 10 100 30] /FT /Tx /T (Stale) >>",
        ),
    ])
}

/// Would catch: `/NeedAppearances` not being carried from the source.
///
/// ## Why the first version of this test proved nothing
///
/// It asserted `merged == source`, and the corpus form reports
/// `need_appearances=0` — so both sides were `false` and the assertion held no
/// matter what the merge did. Inverting the source's flag in the
/// implementation left it green. **A fixture that cannot express the
/// distinction makes a test that cannot fail**, which is the third distinct
/// reason a sabotage has stayed green today.
///
/// So this uses a byte-authored form that genuinely sets the flag.
///
/// ## Why the flag matters enough to test
///
/// `/NeedAppearances true` means *"the appearance streams in this file may be
/// stale — regenerate them before display."* Dropping it on a merge makes the
/// arriving fields render from appearances **their own producer already
/// declared untrustworthy**, and they render *plausibly*: an old value, an old
/// font, an old size. Nothing looks broken, so nothing gets reported.
#[test]
fn need_appearances_survives_a_merge_from_a_source_that_sets_it() {
    let src = Document::from_bytes(needs_appearances_doc()).expect("hand-built source parses");
    assert!(
        pdfcer_core::forms::parse_acroform(&src)
            .map(|f| f.need_appearances)
            .unwrap_or(false),
        "premise: the SOURCE sets /NeedAppearances — without this the test is vacuous"
    );

    let mut session = blank_session();
    assert!(
        !pdfcer_core::forms::parse_acroform(&session.graph())
            .map(|f| f.need_appearances)
            .unwrap_or(false),
        "premise: the TARGET does not set it, so carrying it is a real change"
    );

    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let out = Document::from_bytes(bytes).expect("reparse");
    assert!(
        pdfcer_core::forms::parse_acroform(&out)
            .map(|f| f.need_appearances)
            .unwrap_or(false),
        "the merged document must inherit /NeedAppearances — otherwise the \
arriving fields render from appearances their producer called stale"
    );
}

/// Would catch: `/SigFlags` (Table 219) not surviving a merge.
///
/// ## Found by reading output, not by a test
///
/// `pdfcer list-fields` reports `sig_flags=0x1` for the corpus form and
/// reported `0x0` for the merged result. Nothing failed; the number was just
/// wrong, and a viewer would not have offered its signing UI for a document
/// that does contain a `/Sig` field.
///
/// Bit 1 is `SignaturesExist`, bit 2 is `AppendOnly`, and the merged document
/// contains the union of both inputs' fields — so it must declare the union
/// of both flags. Carried as a bitwise OR.
///
/// ★ It claims structure, not validity. A signature covers a byte range and
/// the merge renumbers and re-emits every object, so any signature VALUE that
/// came across is already broken by arithmetic. The flag says the document
/// HAS signature fields, which is true.
#[test]
fn sig_flags_survive_a_merge() {
    let Some(src) = doc(ACROFORM) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let src_flags = pdfcer_core::forms::parse_acroform(&src)
        .map(|f| f.sig_flags)
        .unwrap_or(0);
    assert_ne!(
        src_flags, 0,
        "premise: the source declares /SigFlags — without this the test is vacuous"
    );

    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let out = Document::from_bytes(bytes).expect("reparse");
    let merged_flags = pdfcer_core::forms::parse_acroform(&out)
        .map(|f| f.sig_flags)
        .unwrap_or(0);
    assert_eq!(
        merged_flags & src_flags,
        src_flags,
        "every bit the source set must survive; got {merged_flags:#x} from {src_flags:#x}"
    );
}

// ---------------------------------------------------------------------------
// Navigation structures — named destinations and outlines
// ---------------------------------------------------------------------------

const OUTLINED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/outline/basic-tree.pdf"
);

/// Would catch: the source's bookmarks not arriving, or arriving nested under
/// an invented heading.
///
/// `basic-tree.pdf` has 5 items over 2 top-level chapters. Merged into a
/// document with no outline, all 5 must appear and the 2 must be **top
/// level** — pdfcer does not invent a "merged document" heading to nest them
/// under, because that heading would be a bookmark pdfcer authored sitting in
/// the panel beside bookmarks the authors wrote.
#[test]
fn a_merged_documents_bookmarks_arrive_as_top_level_siblings() {
    let src = Document::from_bytes(std::fs::read(OUTLINED).expect("fixture")).expect("parse");
    let mut session = blank_session();
    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");
    assert_eq!(out.outline_items_carried, 5, "every item, at every level");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let merged = Document::from_bytes(bytes).expect("reparse");
    let outline = pdfcer_core::outline::read_outline(&merged);
    assert_eq!(
        outline.items.len(),
        2,
        "the two chapters must be TOP LEVEL, not under an invented heading"
    );
    assert_eq!(outline.items[0].title, "Chapter 1");
    assert_eq!(outline.items[0].children.len(), 2);
    assert_eq!(outline.items[1].title, "Chapter 2");
    assert_eq!(outline.diagnostics.items, 5);
    assert!(
        outline.diagnostics.is_faithful(),
        "a carried outline must not introduce a diagnostic: {:?}",
        outline.diagnostics
    );
}

/// Would catch: carried bookmarks pointing at the SOURCE's page numbers, or
/// at the target's.
///
/// This is the property a merge exists to preserve and the easiest to get
/// silently wrong: the copy remaps object numbers, so a destination that was
/// not remapped still *resolves* — to whatever object now holds that number in
/// the target. The bookmark works, and points at the wrong page.
///
/// Merging into a **one-page** target puts the source's pages at index 1
/// onward, so every carried destination must have shifted by exactly one.
#[test]
fn carried_bookmarks_point_at_the_pages_that_actually_arrived() {
    let src = Document::from_bytes(std::fs::read(OUTLINED).expect("fixture")).expect("parse");
    let before = pdfcer_core::outline::read_outline(&src);
    let src_pages: Vec<Option<usize>> = before.items.iter().map(|i| i.page_index()).collect();
    assert_eq!(
        src_pages,
        vec![Some(0), Some(2)],
        "premise: the source's chapters point at pages 0 and 2"
    );

    let mut session = blank_session();
    let target_pages = session.page_slots().expect("pages").len();
    assert_eq!(target_pages, 1, "premise: the target has one page");
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let merged = Document::from_bytes(bytes).expect("reparse");
    let after = pdfcer_core::outline::read_outline(&merged);
    let got: Vec<Option<usize>> = after.items.iter().map(|i| i.page_index()).collect();
    assert_eq!(
        got,
        vec![Some(1), Some(3)],
        "every destination must shift by the target's page count; unshifted \
values would mean the copy's remapping was bypassed and the bookmarks now \
point at the TARGET's pages"
    );
    assert_eq!(
        after.diagnostics.unresolved_names, 0,
        "no carried bookmark may be left pointing at nothing"
    );
}

/// Would catch: a colliding destination key being silently merged, or being
/// renamed **without** the carried bookmarks following it.
///
/// ## ★ The first version of this test could not fail
///
/// It used `basic-tree.pdf`, whose bookmarks carry **explicit** `/Dest`
/// arrays — so the source defined no named destinations, nothing collided,
/// nothing was renamed, and the rewrite path never ran. Three separate
/// sabotages (drop the rewrite, reverse the carry order, let a collision
/// overwrite) all left it green. Fixture too narrow to express the
/// distinction, for the fourth time this session.
///
/// `named-dests.pdf` is the fixture that exercises it: its bookmarks point at
/// keys in both §12.3.2.3 namespaces.
///
/// ## Why the carry ORDER is what this really pins
///
/// Destinations are carried before outlines precisely so a suffixed key can
/// be rewritten onto the bookmarks that name it. Run it the other way and
/// the bookmark keeps a key nothing defines — which renders as a bookmark
/// that does nothing, the exact failure `add_outline_item` refuses a
/// forward reference to avoid.
#[test]
fn a_renamed_destination_takes_its_bookmarks_with_it() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/outline/named-dests.pdf"
    );
    let src = Document::from_bytes(std::fs::read(path).expect("fixture")).expect("parse");
    let src_names = pdfcer_core::pageops::references::DestinationResolver::new(&src).named_count();
    assert!(
        src_names > 0,
        "premise: the source DEFINES named destinations — without this the \
test cannot fail"
    );
    let src_resolved = pdfcer_core::outline::read_outline(&src)
        .items
        .iter()
        .filter(|i| i.page_index().is_some())
        .count();
    assert!(
        src_resolved > 0,
        "premise: at least one bookmark RESOLVES through a named destination"
    );

    let mut session = blank_session();
    let first = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("first merge");
    assert_eq!(
        first.named_destinations_renamed, 0,
        "nothing to collide with yet"
    );

    // The same document again: every key now collides with the first
    // merge's arrivals, so every one must be suffixed AND its bookmarks
    // rewritten.
    let second = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("second merge");
    assert_eq!(
        second.named_destinations_renamed, src_names,
        "every key from the first merge is taken, so every arrival must be renamed"
    );

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let merged = Document::from_bytes(bytes).expect("reparse");
    let outline = pdfcer_core::outline::read_outline(&merged);

    // ★ The property. Both merges' bookmarks must still resolve to a page.
    let resolved = outline
        .items
        .iter()
        .filter(|i| i.page_index().is_some())
        .count();
    assert_eq!(
        resolved,
        src_resolved * 2,
        "every bookmark from BOTH merges must still resolve; a shortfall means \
a suffixed key left its bookmarks behind pointing at a name nothing defines"
    );

    // And the two merges point at DIFFERENT pages — the second merge's
    // bookmarks must not have been re-aimed at the first merge's copies.
    let pages: Vec<usize> = outline
        .items
        .iter()
        .filter_map(|i| i.page_index())
        .collect();
    let mut uniq = pages.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert!(
        uniq.len() > src_resolved,
        "the second merge's bookmarks resolve to the same pages as the first, \
so a colliding key overwrote rather than renamed: {pages:?}"
    );

    // No two destinations may share a key.
    let keys: Vec<Vec<u8>> = pdfcer_core::pageops::references::DestinationResolver::new(&merged)
        .iter()
        .map(|(k, _)| k.to_vec())
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    let n = sorted.len();
    sorted.dedup();
    assert_eq!(sorted.len(), n, "two destinations share a key");
    assert_eq!(
        n,
        src_names * 2,
        "both merges' destinations must survive as distinct keys"
    );
}

/// Would catch: carried outline items not being re-pointed at the target's
/// outline root.
///
/// ## Why every other outline test here is blind to it
///
/// `read_outline` walks **downward** from the root's `/First`, following
/// `/First` and `/Next`. It never reads an item's `/Parent`. So a carried
/// subtree whose items still name the SOURCE's outline root — an object that
/// does not exist in this document — reads back perfectly.
///
/// §12.3.3 requires `/Parent` on every item, and a viewer walking upward from
/// a selected bookmark (to collapse its ancestor, or to find its siblings)
/// lands on a dangling reference. This is the **fourth** property today whose
/// only witness is the raw bytes, after `/Prev`, `/QuadPoints` and the
/// widgets' own `/Parent`.
#[test]
fn carried_outline_items_point_at_this_documents_root() {
    let src = Document::from_bytes(std::fs::read(OUTLINED).expect("fixture")).expect("parse");
    let mut session = blank_session();
    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let merged = Document::from_bytes(bytes).expect("reparse");
    let Some(Object::Reference(root_id)) = merged
        .catalog_dict()
        .and_then(|c| c.get(b"Outlines").cloned())
    else {
        panic!("the merged document must have an /Outlines reference")
    };

    // Every TOP-LEVEL item must name this root.
    let Object::Dict(root) = merged.resolved(root_id) else {
        panic!("outline root is not a dict")
    };
    let mut cursor = match root.get(b"First") {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    };
    let mut checked = 0usize;
    while let Some(id) = cursor {
        let Object::Dict(item) = merged.resolved(id) else {
            panic!("outline item {id:?} is not a dict")
        };
        assert_eq!(
            item.get(b"Parent").and_then(Object::as_reference),
            Some(root_id),
            "carried item {id:?} does not name this document's outline root; a \
viewer walking up from it lands on an object that is not here"
        );
        checked += 1;
        cursor = match item.get(b"Next") {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        };
    }
    assert_eq!(checked, 2, "both carried chapters must have been checked");
}
/// Would catch: the merge reporting counts it did not achieve.
///
/// Cheap, and it is the number a shell will put in front of an operator — a
/// disclosure that overstates is worse than none, because it is believed.
#[test]
fn the_reported_counts_match_what_landed() {
    let src = Document::from_bytes(std::fs::read(OUTLINED).expect("fixture")).expect("parse");
    let mut session = blank_session();
    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");

    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let merged = Document::from_bytes(bytes).expect("reparse");

    assert_eq!(
        out.outline_items_carried,
        pdfcer_core::outline::read_outline(&merged)
            .diagnostics
            .items,
        "outline_items_carried must equal what the reader finds"
    );
    assert_eq!(
        out.named_destinations_carried,
        pdfcer_core::pageops::references::DestinationResolver::new(&merged).named_count(),
        "named_destinations_carried must equal what the merged document defines"
    );
    assert_eq!(out.named_destinations_renamed, 0, "nothing to collide with");
    assert_eq!(out.pages_merged, 5, "basic-tree.pdf has five pages");
}

/// Would catch: undo leaving the carried navigation behind.
///
/// The `/AcroForm` half is already covered; this pins that adding two more
/// document-level writes to the same command did not break its atomicity —
/// which is the specific risk of growing a command by writing the catalog
/// from three places.
#[test]
fn undo_removes_the_carried_navigation_too() {
    let src = Document::from_bytes(std::fs::read(OUTLINED).expect("fixture")).expect("parse");
    let mut session = blank_session();
    let before = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;

    session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");
    assert!(session.undo().is_some(), "exactly one undo entry");

    let after = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    assert_eq!(
        before, after,
        "pages, fields, destinations and outline must all reverse together"
    );
}
