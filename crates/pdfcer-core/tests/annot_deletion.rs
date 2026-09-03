//! `EditSession::delete_annotation` — the general annotation-deletion verb
//! and its three cascades (`Pass 38.5`, ISO 32000-1 §12.5).
//!
//! ## What is actually under test here, and why it is not "does it delete"
//!
//! Removing the named annotation is the easy half and would be pinned by
//! one assertion. Almost everything that can go wrong in this verb goes
//! wrong on some **other** object:
//!
//! 1. a `/Popup` companion that must go too (§12.5.6.14 — it *"shall not
//!    appear alone"*), or must **not** be assumed to exist;
//! 2. every `/IRT` referrer, which must survive with its dangling link
//!    removed — and must be counted in the right one of two buckets,
//!    because a `/RT /Group` subordinate suffers a materially different
//!    consequence from a `/RT /R` reply (§12.5.6.2 group attributes);
//! 3. appearance streams, which must go **only when unshared** — a
//!    producer stamping "DRAFT" on forty pages from one form XObject is
//!    doing something entirely legal, and a deletion that took its
//!    target's `/AP` unconditionally would blank the other thirty-nine.
//!
//! Each of those has a "looks right, is wrong" implementation that a
//! single-annotation fixture would let through, which is why
//! `fixtures/synthetic/annot/thread.pdf` carries seven annotations
//! arranged so that deleting one has consequences for five.
//!
//! ## The certification pair, and why BOTH files are needed
//!
//! `delete_annotation` is the **first** pdfcer operation that a `/DocMDP`
//! `/P` value positively permits. §12.8.2.2 Table 254 `P = 3`: *"Permitted
//! changes shall be the same as for 2, as well as **annotation creation,
//! deletion, and modification**"*. Until this Pass, the strict gate was
//! right for everything, exactly as
//! `SignatureCensus::forbids_structural_change`'s own doc comment said.
//!
//! Testing that at `/P 3` alone would be **R162** — an assertion that
//! cannot come out false. "Deletion is permitted here" is equally
//! consistent with the verb never reading the certification at all. So
//! `certified-p2-annot.pdf` is the same file with one digit changed, and
//! the pair is what makes the gate assertable. A third case — the form
//! field on the *same* `/P 3` document, which must still be refused —
//! stops the fix from being "the annotation verb dropped its gate".
//!
//! ## Assertions are on the SAVED BYTES where the claim is about bytes (R159)
//!
//! An `/IRT` removed only in the session overlay and never written is a
//! passing test over a broken file. Where the claim is *"the surviving
//! annotation's dictionary no longer names the deleted one"*, the file is
//! saved and re-loaded and the reloaded dictionary is what is checked.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot::{ReplyType, page_annotations};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{AnnotationDeletionRoute, EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::writer::SaveOptions;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// The annotation at `index` on page 1, by the same walk `list-annotations`
/// uses — so a test addresses an annotation the way an operator does.
fn annot_id_at(s: &EditSession, index: usize) -> ObjId {
    let slots = s.page_slots().expect("page slots");
    page_annotations(&s.graph(), slots[0].id)
        .get(index)
        .and_then(|a| a.id)
        .expect("annotation with an object identity at that index")
}

/// Serialise through the **incremental** path and re-parse the result, so
/// every assertion downstream is about bytes a different program would read
/// (R159) rather than about the session's own in-memory overlay.
///
/// The `_name` parameter is retained as documentation of which case each
/// call is exercising; nothing touches the filesystem, because a
/// round-trip through `Vec<u8>` proves the same property and leaves no
/// temp files to collide between parallel test binaries.
fn save_and_reload(s: &EditSession, _name: &str) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse the saved bytes")
}

fn dict_of(doc: &Document, id: ObjId) -> pdfcer_core::object::Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("object {id} is not a dictionary: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Cascade 1 + 2: deleting the primary
// ---------------------------------------------------------------------------

/// **The headline case.** Deleting one annotation removes its pop-up, and
/// un-links three others without deleting any of them.
#[test]
fn deleting_a_primary_takes_its_popup_and_unlinks_but_never_deletes_its_replies() {
    let mut s = session("annot/thread.pdf");
    let primary = annot_id_at(&s, 0);

    let gone = s.delete_annotation(primary).expect("delete the primary");

    assert_eq!(gone.route, AnnotationDeletionRoute::General);
    assert_eq!(gone.subtype, "Square");
    assert!(
        gone.popup_removed,
        "the primary carries /Popup, and 12.5.6.14 says a pop-up \"shall \
         not appear alone\" — leaving it behind is not untidiness, it is a \
         clause violation",
    );
    assert_eq!(
        gone.replies_orphaned, 2,
        "objects 6 and 7 both reply to the primary. If this is 1, the \
         IMPLICIT reply (object 7, which carries /IRT and NO /RT) was not \
         recognised — Table 170's default value for /RT is R, so an absent \
         key is a reply, and treating it as 'not a reply' is wrong in the \
         ordinary case rather than in an exotic one",
    );
    assert_eq!(
        gone.group_members_promoted, 1,
        "object 8 is /RT /Group, and it must NOT be counted with the \
         replies: while the primary existed, 12.5.6.2 required a reader to \
         IGNORE this annotation's own /Contents and /T in favour of the \
         primary's, so what the operator sees change is different in kind",
    );
    assert_eq!(
        gone.appearance_streams_removed, 1,
        "the primary's own /AP /N (object 11) is referenced by nothing else",
    );

    // Five annotations survive: two replies, the ex-subordinate, two stamps.
    let doc = save_and_reload(&s, "primary-deleted.pdf");
    let slots = pdfcer_core::page_tree::pages(&doc).expect("pages");
    let after = page_annotations(&doc, slots[0].id);
    assert_eq!(
        after.len(),
        5,
        "7 annotations minus the primary and its pop-up. A different number \
         means either a reply was deleted (it must not be — that is somebody \
         else's text) or the pop-up survived",
    );
    assert!(
        after.iter().all(|a| !a.is_popup),
        "the pop-up must be gone from /Annots, not merely orphaned",
    );
}

/// The saved bytes carry no reference to the deleted object. This is the
/// structural half of the `/SeparationInfo` posture: repair the invariant,
/// refuse to guess the semantics.
#[test]
fn a_surviving_reply_no_longer_names_the_deleted_annotation_in_the_saved_bytes() {
    let mut s = session("annot/thread.pdf");
    let primary = annot_id_at(&s, 0);
    // Captured before the delete — afterwards these indices shift.
    let reply_explicit = annot_id_at(&s, 2);
    let reply_implicit = annot_id_at(&s, 3);
    let subordinate = annot_id_at(&s, 4);

    s.delete_annotation(primary).expect("delete the primary");
    let doc = save_and_reload(&s, "irt-cleared.pdf");

    for (id, label) in [
        (reply_explicit, "the explicit /RT /R reply"),
        (reply_implicit, "the implicit reply (no /RT)"),
        (subordinate, "the /RT /Group subordinate"),
    ] {
        let d = dict_of(&doc, id);
        assert!(
            d.get(b"IRT").is_none(),
            "{label} still carries /IRT after its target was deleted. A live \
             reference to a dead object is the structural invariant this verb \
             DOES repair — the semantic question (should this outlive its \
             thread?) is the one it refuses to answer",
        );
        assert!(
            d.get(b"RT").is_none(),
            "{label} still carries /RT. Table 170 makes /RT meaningful only \
             alongside /IRT, so a surviving /RT /Group would declare \
             membership of nothing — inert to a reader, but exactly the kind \
             of residue that reads as a bug to the next tool",
        );
        assert!(
            d.get(b"Contents").is_some(),
            "{label} lost its note text. Un-linking must not touch anything \
             but the two relationship keys",
        );
        assert!(
            d.get(b"T").is_some(),
            "{label} lost its author. Same rule as /Contents",
        );
    }
}

/// The subordinate's own text is what a reader will now display, and the
/// model must say so — `is_group_subordinate` goes false, and the keys the
/// group rule suppressed are still present to be shown.
#[test]
fn a_group_subordinate_becomes_readable_on_its_own_after_the_primary_goes() {
    let mut s = session("annot/thread.pdf");
    let primary = annot_id_at(&s, 0);

    let before = page_annotations(&s.graph(), s.page_slots().unwrap()[0].id);
    let sub_before = before.iter().find(|a| a.id == Some(annot_id_at(&s, 4)));
    let sub_before = sub_before.expect("the subordinate is on page 1");
    assert!(
        sub_before.is_group_subordinate(),
        "the fixture's object 8 must read as a /RT /Group subordinate before \
         the delete, or the assertion after it proves nothing",
    );
    assert_eq!(sub_before.effective_reply_type(), Some(ReplyType::Group));

    s.delete_annotation(primary).expect("delete the primary");

    let after = page_annotations(&s.graph(), s.page_slots().unwrap()[0].id);
    let sub = after
        .iter()
        .find(|a| a.contents.as_deref() == Some("suppressed while the primary lives"))
        .expect("the subordinate survives — it is a separate annotation");
    assert!(
        !sub.is_group_subordinate(),
        "with /IRT gone it is no longer in a group, so 12.5.6.2's \
         'shall be ignored' no longer applies to its own /Contents and /T",
    );
    assert_eq!(
        sub.effective_reply_type(),
        None,
        "no /IRT means the question 'what relationship is this' has no answer",
    );
    assert_eq!(sub.title.as_deref(), Some("Dave"));
}

// ---------------------------------------------------------------------------
// Cascade 1, the other direction: deleting the pop-up itself
// ---------------------------------------------------------------------------

/// Deleting a window does **not** delete the comment it belongs to. The
/// obligation in §12.5.6.14 constrains the pop-up's existence, not the
/// parent's — so this cascade is deliberately one-directional.
#[test]
fn deleting_a_popup_clears_the_parents_link_and_keeps_the_parent() {
    let mut s = session("annot/thread.pdf");
    let primary = annot_id_at(&s, 0);
    let popup = annot_id_at(&s, 1);

    let gone = s.delete_annotation(popup).expect("delete the pop-up");
    assert_eq!(gone.subtype, "Popup");
    assert!(
        gone.parent_popup_cleared,
        "the parent still named this object; leaving its /Popup pointing at a \
         deleted object is the dangling reference this verb exists to avoid",
    );
    assert!(
        !gone.popup_removed,
        "popup_removed is about a COMPANION going with its parent — reporting \
         it here would say two annotations went when one did",
    );

    let doc = save_and_reload(&s, "popup-deleted.pdf");
    let parent = dict_of(&doc, primary);
    assert!(
        parent.get(b"Popup").is_none(),
        "the parent's /Popup key must be gone from the SAVED bytes",
    );
    assert!(
        parent.get(b"Contents").is_some(),
        "the parent annotation itself must survive intact — deleting a \
         window is not deleting the comment",
    );
}

// ---------------------------------------------------------------------------
// Cascade 3: shared appearance streams
// ---------------------------------------------------------------------------

/// **The forty-stamps case.** The first user of a shared appearance stream
/// is deleted and the stream stays; the last user takes it.
///
/// Both halves are needed. "Never delete an /AP" passes the first
/// assertion; "always delete the /AP" passes the second.
#[test]
fn a_shared_appearance_stream_survives_its_first_user_and_dies_with_its_last() {
    let mut s = session("annot/thread.pdf");
    let stamp_a = annot_id_at(&s, 5);

    let first = s
        .delete_annotation(stamp_a)
        .expect("delete the first stamp");
    assert_eq!(
        first.appearance_streams_removed, 0,
        "stream 12 is still referenced by the second stamp. Deleting it here \
         is the bug that blanks thirty-nine other pages in a document whose \
         producer stamped them all from one form XObject — entirely legal \
         under 12.5.5, which maps the same /BBox into a different /Rect per \
         annotation",
    );

    // Indices shift by one once the first stamp is gone.
    let stamp_b = annot_id_at(&s, 5);
    let second = s
        .delete_annotation(stamp_b)
        .expect("delete the second stamp");
    assert_eq!(
        second.appearance_streams_removed, 1,
        "nothing references stream 12 any more, and leaving it behind orphans \
         a stream in every subsequent save",
    );
}

// ---------------------------------------------------------------------------
// The preview query — the fact a shell warns with
// ---------------------------------------------------------------------------

/// **The preview and the deletion must not be able to disagree.** A tooltip
/// promising "2 replies will be kept" over a verb that deletes them is
/// worse than no tooltip, and nothing in the running program compares the
/// two — so this test is the comparison.
///
/// Run over EVERY deletable annotation in the fixture, not one, because the
/// interesting divergences are per-shape: the primary has all three
/// cascades, a stamp has none, the pop-up has the reverse cascade.
#[test]
fn the_preview_agrees_with_the_deletion_for_every_annotation_in_the_fixture() {
    let probe = session("annot/thread.pdf");
    let count = page_annotations(&probe.graph(), probe.page_slots().unwrap()[0].id).len();

    for index in 0..count {
        // A fresh session per index: a preview is about the document as it
        // stands, and reusing one session would compare each preview against
        // a document the previous deletion had already changed.
        let mut s = session("annot/thread.pdf");
        let id = annot_id_at(&s, index);

        let predicted = s
            .annotation_deletion_preview(id)
            .unwrap_or_else(|e| panic!("index {index}: preview refused: {e}"));
        let actual = s
            .delete_annotation(id)
            .unwrap_or_else(|e| panic!("index {index}: preview said yes but the delete said {e}"));

        assert_eq!(
            predicted.subtype, actual.subtype,
            "index {index}: subtype disagrees",
        );
        assert_eq!(predicted.route, actual.route, "index {index}: route");
        assert_eq!(
            predicted.popup_removed, actual.popup_removed,
            "index {index}: popup_removed",
        );
        assert_eq!(
            predicted.parent_popup_cleared, actual.parent_popup_cleared,
            "index {index}: parent_popup_cleared",
        );
        assert_eq!(
            predicted.replies_orphaned, actual.replies_orphaned,
            "index {index}: replies_orphaned — this is the number a shell puts \
             in a warning BEFORE the click, so a mismatch means the operator \
             was told one thing and given another",
        );
        assert_eq!(
            predicted.group_members_promoted, actual.group_members_promoted,
            "index {index}: group_members_promoted",
        );
        // `appearance_streams_removed` is deliberately NOT compared: the
        // preview reports 0 by documented design (no operator-facing
        // meaning, and computing it costs a document-wide reachability
        // scan for a number nothing would display).
    }
}

/// The preview is a **pure query**: it must leave the session able to save
/// nothing. A preview that dirtied an object would turn a hover into an
/// edit.
#[test]
fn previewing_changes_nothing() {
    let s = session("annot/thread.pdf");
    let before = s.dirty_set().len();
    for index in 0..7 {
        let id = annot_id_at(&s, index);
        let _ = s.annotation_deletion_preview(id);
    }
    assert_eq!(
        s.dirty_set().len(),
        before,
        "a preview must not dirty anything — a UI calls this every frame \
         while the pointer rests on a row",
    );
}

/// The preview raises the same refusals as the verb, so a shell that
/// disables on `Err` is right by construction.
#[test]
fn the_preview_refuses_exactly_what_the_deletion_refuses() {
    let mut s = session("annot/undeletable.pdf");
    let locked = annot_id_at(&s, 0);
    let trapnet = annot_id_at(&s, 2);

    assert!(matches!(
        s.annotation_deletion_preview(locked),
        Err(EditError::AnnotationLocked { .. })
    ));
    assert!(matches!(
        s.annotation_deletion_preview(trapnet),
        Err(EditError::AnnotationIsTrapNet { .. })
    ));
    // And the deletable one previews clean.
    let ok = annot_id_at(&s, 1);
    assert!(s.annotation_deletion_preview(ok).is_ok());
    assert!(s.delete_annotation(ok).is_ok());

    let p2 = session("annot/certified-p2-annot.pdf");
    let annot = annot_id_at(&p2, 0);
    assert!(
        matches!(
            p2.annotation_deletion_preview(annot),
            Err(EditError::CertificationForbidsChange { permission: 2 })
        ),
        "the preview must run the certification gate too — otherwise a shell \
         enables a control whose every press returns this error",
    );
}

// ---------------------------------------------------------------------------
// Routing and refusals
// ---------------------------------------------------------------------------

/// A `/Widget` is refused **by name**, and the message names both verbs
/// plus the field, because widget-or-whole-field is the caller's choice.
#[test]
fn a_widget_is_refused_and_the_refusal_names_the_field_and_both_verbs() {
    let mut s = session("forms/demo-form.pdf");
    let widget = annot_id_at(&s, 0);

    let err = s
        .delete_annotation(widget)
        .expect_err("a widget must not be deletable through the general verb");
    let EditError::AnnotationIsWidget { name, .. } = &err else {
        panic!("expected AnnotationIsWidget, got {err:?}");
    };
    assert_eq!(
        name, "FullName",
        "the refusal must carry the fully-qualified name so the caller can \
         pass it straight to the verb the message names",
    );
    let msg = err.to_string();
    assert!(
        msg.contains("delete_widget") && msg.contains("delete_field"),
        "the message must name BOTH verbs — routing would have to guess \
         which was meant, and guessing wrong deletes a field that appears on \
         other pages. Got: {msg}",
    );
    assert!(
        msg.contains("/AcroForm"),
        "and it must say what would break, not merely that it refused: {msg}",
    );
}

/// A stale id — one whose object was deleted a moment ago — is refused, not
/// silently applied to whatever now bears that number.
#[test]
fn a_stale_id_is_refused_by_name() {
    let mut s = session("annot/thread.pdf");
    let stamp = annot_id_at(&s, 5);
    s.delete_annotation(stamp).expect("first delete succeeds");

    let err = s
        .delete_annotation(stamp)
        .expect_err("the object is no longer an annotation on any page");
    assert!(
        matches!(err, EditError::AnnotationNotFound { .. }),
        "expected AnnotationNotFound, got {err:?}",
    );
}

/// **Table 165 bit 8 `Locked` is the only refusal here the STANDARD
/// requires**, and bit 10 `LockedContents` is not it.
///
/// Both assertions are needed and neither is decorative. Without the
/// first, pdfcer silently deletes annotations a `shall` forbids deleting —
/// invisibly, because a locked annotation looks like any other. Without
/// the second, "refuse when a lock-shaped flag is set" and an off-by-two
/// bit index both pass, and pdfcer would refuse deletions the standard
/// explicitly permits: `LockedContents`' own Table 165 row says it *"does
/// not restrict deletion"*.
#[test]
fn the_locked_flag_refuses_and_locked_contents_does_not() {
    let mut s = session("annot/undeletable.pdf");
    let locked = annot_id_at(&s, 0);
    let locked_contents = annot_id_at(&s, 1);

    let err = s
        .delete_annotation(locked)
        .expect_err("Table 165 bit 8 says this annotation shall not be deleted");
    let EditError::AnnotationLocked { subtype, .. } = &err else {
        panic!("expected AnnotationLocked, got {err:?}");
    };
    assert_eq!(subtype, "Square");
    assert!(
        err.to_string().contains("LockedContents"),
        "the message must name the flag it is NOT, because the two are \
         indistinguishable from a comments list and an operator told only \
         \"locked\" will go looking for the wrong bit: {err}",
    );

    let gone = s.delete_annotation(locked_contents).expect(
        "bit 10 LockedContents 'does not restrict deletion' — Table 165 says \
         so in as many words. Refusing here means the gate is matching on \
         lock-shaped flags rather than on bit 8",
    );
    assert_eq!(gone.subtype, "Square");
}

/// A `/TrapNet` is prepress output state with a positional `shall`
/// (§12.5.6.21), not markup, and is refused rather than silently removed
/// from a comment surface.
#[test]
fn a_trapnet_is_refused_by_name() {
    let mut s = session("annot/undeletable.pdf");
    let trapnet = annot_id_at(&s, 2);

    let err = s
        .delete_annotation(trapnet)
        .expect_err("a trap network is not a comment");
    assert!(
        matches!(err, EditError::AnnotationIsTrapNet { .. }),
        "expected AnnotationIsTrapNet, got {err:?}",
    );
    assert!(
        err.to_string().contains("trapped"),
        "the message must say what removing it would MEAN — that the page \
         would claim it was never trapped — not merely that it refused: {err}",
    );
}

// ---------------------------------------------------------------------------
// The certification gate — the first pdfcer operation any /P value permits
// ---------------------------------------------------------------------------

/// `/P 3` permits annotation deletion (§12.8.2.2 Table 254) and still
/// forbids everything else. All three assertions are on **one** document,
/// so none of them can pass because of a difference between two fixtures.
#[test]
fn p3_permits_deleting_a_comment_and_still_refuses_deleting_a_field() {
    let mut s = session("annot/certified-p3.pdf");

    assert!(
        s.annotation_deletion_refusal().is_none(),
        "/P 3 is 'the same as for 2, as well as annotation creation, \
         deletion, and modification'. This is the first pdfcer operation any \
         P value permits, and refusing it here turns a document certified \
         SPECIFICALLY for comment review into a read-only one",
    );
    assert!(
        s.deletion_refusal().is_some(),
        "field deletion takes the STRICT gate and must still refuse: /P 3 is \
         /P 2 plus annotations, and a form field is not an annotation change. \
         If this is None, the fix widened the wrong gate",
    );

    let annot = annot_id_at(&s, 0);
    let gone = s
        .delete_annotation(annot)
        .expect("the gate said yes, so the verb must too");
    assert_eq!(gone.subtype, "Square");
    assert!(
        matches!(
            s.delete_field("FullName"),
            Err(EditError::CertificationForbidsChange { permission: 3 })
        ),
        "and the field refusal must report the permission it read, so the \
         operator can tell a P=1 lockdown from a P=3 comment-review document",
    );
}

/// **`/P 3` permits annotation CREATION too, and the gate had to be
/// widened for that as well** (§12.8.2.2 Table 254: *"annotation
/// creation, deletion, and modification"* — all three words).
///
/// Found while filing the deletion half: `add_markup`,
/// `add_text_annotation` and `add_redaction` all took the STRICT gate, so
/// pdfcer could delete a comment on a `/P 3` document and not write one —
/// a split with no basis in Table 254. Asserted here rather than in a
/// markup test file because the claim is about the GATE, and this file is
/// where the gate's fixtures live.
#[test]
fn p3_permits_authoring_an_annotation_as_well_as_deleting_one() {
    use pdfcer_core::annot_author::{Color, MarkupSpec};

    let mut s = session("annot/certified-p3.pdf");
    let spec = MarkupSpec::Square {
        rect: pdfcer_core::page_tree::Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 60.0,
            ury: 40.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    };
    s.add_markup(0, &spec).expect(
        "/P 3 permits ANNOTATION CREATION. Refusing here means pdfcer can \
         delete a comment on this document but not write one, which Table \
         254 does not say anywhere",
    );

    // And the falsifier, on the same axis: at /P 2 it must still refuse.
    let mut p2 = session("annot/certified-p2-annot.pdf");
    assert!(
        matches!(
            p2.add_markup(0, &spec),
            Err(EditError::CertificationForbidsChange { permission: 2 })
        ),
        "/P 2 permits form filling, template instantiation and signing — \
         not annotations. If this succeeds, the widening went too far and \
         the gate is no longer reading /P",
    );
}

/// The falsifier. The same file with one digit changed refuses, which is
/// what makes the test above an assertion about `/P` rather than about
/// nothing.
#[test]
fn p2_refuses_deleting_a_comment() {
    let s = session("annot/certified-p2-annot.pdf");

    let refusal = s.annotation_deletion_refusal().expect(
        "/P 2 permits form filling, template instantiation and signing — \
         annotations are NOT on that list. If this is None, the annotation \
         gate is not reading /P at all and the /P 3 test above is vacuous",
    );
    assert!(
        matches!(
            refusal,
            EditError::CertificationForbidsChange { permission: 2 }
        ),
        "the refusal must name the permission it read: {refusal:?}",
    );
}

/// The three refusal queries are three different answers, and a shell that
/// reused one for another would get it wrong in both directions.
#[test]
fn the_three_refusal_queries_disagree_on_purpose() {
    let p3 = session("annot/certified-p3.pdf");
    assert!(p3.fill_refusal().is_none(), "P>=2 permits filling");
    assert!(p3.deletion_refusal().is_some(), "structural change refused");
    assert!(
        p3.annotation_deletion_refusal().is_none(),
        "annotations permitted at P=3 — this row is the whole reason the \
         third query exists",
    );

    let p2 = session("annot/certified-p2-annot.pdf");
    assert!(p2.fill_refusal().is_none());
    assert!(p2.deletion_refusal().is_some());
    assert!(
        p2.annotation_deletion_refusal().is_some(),
        "at P=2 the annotation query must agree with the strict one — \
         without this, 'the annotation query is just laxer' explains the P=3 \
         result equally well",
    );
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

/// One command, so one undo — including the pop-up, the appearance stream
/// and all three `/IRT` patches.
#[test]
fn the_whole_cascade_undoes_as_one_command() {
    let mut s = session("annot/thread.pdf");
    let primary = annot_id_at(&s, 0);
    let before = page_annotations(&s.graph(), s.page_slots().unwrap()[0].id).len();

    s.delete_annotation(primary).expect("delete");
    assert_eq!(
        page_annotations(&s.graph(), s.page_slots().unwrap()[0].id).len(),
        before - 2,
    );

    s.undo().expect("undo the deletion");
    let restored = page_annotations(&s.graph(), s.page_slots().unwrap()[0].id);
    assert_eq!(
        restored.len(),
        before,
        "a single undo must restore the annotation AND its pop-up. If this is \
         off by one, the cascade was committed as two commands and the \
         operator has to press undo twice for one action",
    );
    let reply = restored
        .iter()
        .find(|a| a.contents.as_deref() == Some("first reply"))
        .expect("the reply is back on the page");
    assert_eq!(
        reply.in_reply_to,
        Some(primary),
        "the /IRT patches must undo with the rest — a reply left un-linked \
         after an undo is a silent, permanent loss of the thread's shape",
    );
}
