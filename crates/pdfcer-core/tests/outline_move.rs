//! # Moving a bookmark: reorder and re-parent (`Pass 161.0`)
//!
//! `Pass 156.0` gave the outline rename and delete. An outline in the wrong
//! **order** still had no fix short of deleting a branch and re-authoring it,
//! which discards every destination, colour and style on it. This is the
//! other half: an existing bookmark, and its whole subtree, moved somewhere
//! else in the tree.
//!
//! ## What these tests are actually guarding
//!
//! An outline is a **doubly-linked sibling chain inside a tree** (§12.3.3
//! Tables 152–153). A move unlinks from one chain and splices into another,
//! touching up to nine dictionaries: the old `/Prev`'s `/Next` (or the old
//! parent's `/First`), the old `/Next`'s `/Prev` (or the old parent's
//! `/Last`), the same four on the new side, the item's own `/Parent`,
//! `/Prev` and `/Next`, and `/Count` up **both** branches.
//!
//! Every one of those has the same failure shape: **the file still parses**.
//! A reader walking `/First` then `/Next` stops early, revisits an item, or —
//! in the cycle case — never terminates. None of that is visible to a test
//! that only checks the moved bookmark ended up in the right place.
//!
//! ## ★★ Why the flattened title list is NOT the assertion here
//!
//! `outline_edit.rs` asserts on titles in reading order, and for a move that
//! is a **coincident oracle** — `R225`, a fixture whose two candidate answers
//! agree. Making *Chapter 2* the last child of *Chapter 1* produces the
//! reading order
//!
//! ```text
//! Chapter 1, Section 1.1, Section 1.2, Chapter 2, Section 2.1
//! ```
//!
//! which is **character-for-character the order it already had** — because a
//! depth-first walk of the moved tree happens to visit the same titles in the
//! same sequence. A test asserting that list would pass whether the
//! re-parenting happened or not. So the assertions below are on **structure**
//! (whose child is whose) and on `/Count`, never on a flat list.
//!
//! ## The independent oracle
//!
//! [`Outline::visible_item_count`] is a **reader-side** implementation of the
//! root `/Count` quantity, derived from ISO 32000-1 Annex H.6's two printings
//! of the same outline and verified against every value in both. The writer's
//! `/Count` propagation is a separate implementation, written from Table 153's
//! sign convention. Asserting one against the other is a real cross-check
//! rather than the same expression measured twice (`R188`).
//!
//! ## Fixture
//!
//! `fixtures/synthetic/outline/basic-tree.pdf`, provenance in that directory's
//! `PROVENANCE.md`. Its shape, which every expected number below is derived
//! from by hand:
//!
//! ```text
//! 8  root            /Count 4        First 9   Last 10
//! 9  Chapter 1       /Count 2  OPEN  First 11  Last 12   Next 10
//! 11   Section 1.1   leaf                                Next 12
//! 12   Section 1.2   leaf                                Prev 11
//! 10 Chapter 2       /Count -1 CLOSED First 13 Last 13   Prev 9
//! 13   Section 2.1   leaf
//! ```
//!
//! Four visible items: Chapter 1, its two sections, and Chapter 2. Section 2.1
//! is **not** visible — Chapter 2 is closed — which is what makes this fixture
//! able to tell "visible items" apart from "subtree size".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, OutlinePlacement};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::outline::read_outline;
use pdfcer_core::writer::SaveOptions;

const ROOT: ObjId = ObjId {
    num: 8,
    generation: 0,
};
const CH1: ObjId = ObjId {
    num: 9,
    generation: 0,
};
const CH2: ObjId = ObjId {
    num: 10,
    generation: 0,
};
const SEC11: ObjId = ObjId {
    num: 11,
    generation: 0,
};
const SEC12: ObjId = ObjId {
    num: 12,
    generation: 0,
};
const SEC21: ObjId = ObjId {
    num: 13,
    generation: 0,
};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("outline/basic-tree.pdf")).expect("load fixture"))
}

/// Re-read the document **through the saved bytes**, never out of the session.
///
/// Every structural assertion in this file goes through here on purpose: an
/// edit that is correct in the session's object map and wrong in the update
/// section it writes is precisely the defect an in-session assertion cannot
/// see.
fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn count_of(doc: &Document, id: ObjId) -> Option<i64> {
    match dict_of(doc, id).get(b"Count") {
        Some(Object::Integer(n)) => Some(*n),
        _ => None,
    }
}

fn link(doc: &Document, id: ObjId, key: &[u8]) -> Option<ObjId> {
    match dict_of(doc, id).get(key) {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    }
}

/// `(title, [children])` for the whole tree — the **structural** view the
/// flattened title list cannot provide.
fn shape(doc: &Document) -> Vec<(String, Vec<String>)> {
    read_outline(&doc.view())
        .items
        .iter()
        .map(|i| {
            (
                i.title.clone(),
                i.children.iter().map(|c| c.title.clone()).collect(),
            )
        })
        .collect()
}

/// ★ The cross-implementation check, run after every structural move.
///
/// The root's `/Count` is written by the edit session's propagation walk; the
/// expected value is computed by the reader's own Annex H.6 formula over the
/// re-parsed tree. They are separate code written from separate readings of
/// the clause, so agreement is evidence and disagreement names which one is
/// wrong.
fn assert_root_count_matches_reader(doc: &Document) {
    let reader = read_outline(&doc.view()).visible_item_count();
    let written = count_of(doc, ROOT).expect("the root always carries /Count");
    assert_eq!(
        written,
        i64::try_from(reader).unwrap(),
        "the root /Count the writer produced disagrees with the visible-item \
         total the reader computes from the saved tree"
    );
}

/// Walk the whole sibling chain of every parent and confirm the doubly-linked
/// invariants hold, in the saved bytes.
///
/// This is the assertion that catches a half-relinked chain — the failure that
/// leaves the file parseable and the outline subtly wrong. Checked for every
/// node reachable from the root, not only the ones a test touched, because a
/// move's damage lands on the *neighbours* of what moved.
fn assert_chain_is_sound(doc: &Document) {
    fn walk(doc: &Document, parent: ObjId, seen: &mut Vec<ObjId>) {
        let first = link(doc, parent, b"First");
        let last = link(doc, parent, b"Last");
        assert_eq!(
            first.is_some(),
            last.is_some(),
            "{parent:?} has one of /First and /Last without the other"
        );
        let Some(first) = first else { return };
        assert_eq!(
            link(doc, first, b"Prev"),
            None,
            "the first child {first:?} of {parent:?} must have no /Prev"
        );
        let mut cursor = Some(first);
        let mut prev: Option<ObjId> = None;
        let mut guard = 0;
        while let Some(id) = cursor {
            guard += 1;
            assert!(guard < 64, "sibling chain under {parent:?} does not end");
            assert!(!seen.contains(&id), "{id:?} is reachable twice");
            seen.push(id);
            assert_eq!(
                link(doc, id, b"Parent"),
                Some(parent),
                "{id:?} is in {parent:?}'s chain but its /Parent says otherwise"
            );
            assert_eq!(
                link(doc, id, b"Prev"),
                prev,
                "{id:?}'s /Prev does not match the item that points /Next at it"
            );
            walk(doc, id, seen);
            prev = Some(id);
            cursor = link(doc, id, b"Next");
        }
        assert_eq!(
            prev, last,
            "{parent:?}'s /Last is not the item the chain actually ends on"
        );
    }
    let mut seen = Vec::new();
    walk(doc, ROOT, &mut seen);
}

// ---------------------------------------------------------------------------
// 1. REORDER — same parent, new position
// ---------------------------------------------------------------------------

/// Chapter 1 moved behind Chapter 2. The two chapters swap; nothing else in
/// the document is touched.
///
/// ★ The `/Count` assertions are the point. A reorder moves **no item between
/// branches**, so every count in the file — the root's and both chapters' —
/// must be exactly what it was. An implementation that ran its subtract and
/// add walks unconditionally would still land on the right numbers here (they
/// cancel), which is why the dirty-set assertion below exists alongside them.
#[test]
fn a_bookmark_can_be_moved_behind_its_sibling() {
    let mut s = session();
    let report = s
        .move_outline_item(CH1, OutlinePlacement::After { sibling: CH2 })
        .expect("reorder");
    assert!(report.moved);
    assert!(!report.reparented, "same parent — this is a reorder");
    assert_eq!(report.from_parent, ROOT);
    assert_eq!(report.to_parent, ROOT);

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);

    assert_eq!(
        shape(&after),
        vec![
            ("Chapter 2".to_string(), vec!["Section 2.1".to_string()]),
            (
                "Chapter 1".to_string(),
                vec!["Section 1.1".to_string(), "Section 1.2".to_string()]
            ),
        ],
        "the chapters swapped and each kept its own children"
    );
    assert_eq!(count_of(&after, ROOT), Some(4), "no item changed branch");
    assert_eq!(count_of(&after, CH1), Some(2), "Chapter 1 keeps its count");
    assert_eq!(
        count_of(&after, CH2),
        Some(-1),
        "Chapter 2 keeps its count AND its closed state"
    );
}

/// ★★ A reorder must not rewrite a single `/Count`-bearing object it did not
/// need to.
///
/// This is the assertion the value checks above cannot make. Both the correct
/// implementation and one that subtracts from and adds to the same ancestors
/// produce `/Count 4` on the root. Only the **dirty set** distinguishes them —
/// and the difference is real: an incremental save appends every object it is
/// handed, so the wrong one grows the file with dictionaries whose bytes are
/// unchanged, in violation of `ARCHITECTURE.md` §5.
#[test]
fn a_reorder_writes_no_count_bearing_object_that_did_not_change() {
    let mut s = session();
    s.move_outline_item(CH1, OutlinePlacement::After { sibling: CH2 })
        .expect("reorder");

    let dirty = s.dirty_set();
    // The root DOES change: its /First and /Last swapped ends.
    assert!(dirty.contains(ROOT), "the root's /First and /Last moved");
    // The sections never moved and never changed parent. Nothing about them
    // is different, and nothing may be written for them.
    for untouched in [SEC11, SEC12, SEC21] {
        assert!(
            !dirty.contains(untouched),
            "{untouched:?} was re-emitted by a reorder that did not touch it"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. RE-PARENT — and the ancestor whose two deltas cancel
// ---------------------------------------------------------------------------

/// ★★★ Section 1.1 nested under its own next sibling — the case where the
/// **root is correct either way** and only the dirty set can tell a right
/// answer from a lucky one.
///
/// Section 1.1 stays visible throughout: it leaves Chapter 1's chain and lands
/// inside Section 1.2, which is a leaf and is therefore left open. So the root
/// still sees four items and Chapter 1 still contributes two. **Both** are
/// common ancestors of the old and the new branch, and both have deltas that
/// cancel exactly — the root `-1 +1`, Chapter 1 `-1 +1`.
///
/// The root is additionally chosen because **none of its links move either**:
/// Section 1.1 is nowhere in the root's own sibling chain, so `/First` and
/// `/Last` are untouched and there is no legitimate reason to write it. That
/// is what makes this a clean discriminator, and it is the correction to a
/// first version of this test that used Chapter 2 → Chapter 1: Chapter 2 was
/// the **last** top-level item, so moving it genuinely changed the root's
/// `/Last`, and the root was rightly dirty. The counts cancelling and the
/// object being unwritten are two different claims.
///
/// `/Count 4` on the root therefore proves nothing on its own. What this test
/// discriminates is that the **net is right**: any propagation that subtracts
/// without adding, adds without subtracting, or gets either magnitude wrong
/// leaves the root holding a value that differs from the base revision, and
/// `dirty_set` — which diffs against the base, §11.1 — then reports it.
///
/// ★ What it does **not** discriminate, established by sabotage rather than
/// assumed: whether the two deltas were merged and applied **once** or applied
/// as two independent walks. Both land on 4, `dirty_set` compares values and
/// not write counts, and deleting the verb's own "skip unchanged" filter left
/// this test and all its neighbours green. That filter is a narrower undo
/// entry, not a save-time guarantee, and the comment in `edit.rs` that once
/// claimed otherwise has been corrected. The distinction matters here because
/// the obvious reading of a passing dirty-set assertion is the stronger claim.
#[test]
fn an_ancestor_common_to_both_branches_is_not_rewritten() {
    let mut s = session();
    let report = s
        .move_outline_item(
            SEC11,
            OutlinePlacement::LastChild {
                parent: Some(SEC12),
            },
        )
        .expect("nest Section 1.1 under Section 1.2");
    assert!(report.moved && report.reparented);
    assert_eq!(report.from_parent, CH1);
    assert_eq!(report.to_parent, SEC12);
    assert_eq!(report.visible_items, 1, "a leaf carries only itself");

    let dirty = s.dirty_set();
    assert!(
        !dirty.contains(ROOT),
        "the root's links did not move and its two count deltas cancel: it must \
         not appear in the update section at all"
    );
    assert!(
        dirty.contains(CH1),
        "Chapter 1's /First DID move, so it is legitimately dirty — the point \
         of this test is the root, not a blanket claim that nothing is written"
    );

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);
    assert_eq!(
        count_of(&after, ROOT),
        Some(4),
        "Section 1.1 is still visible, one level deeper"
    );
    assert_eq!(
        count_of(&after, CH1),
        Some(2),
        "Chapter 1 lost a direct child and gained a grandchild: still 2 visible"
    );
    assert_eq!(
        count_of(&after, SEC12),
        Some(1),
        "the leaf that gained a child is open"
    );
    assert_eq!(
        shape(&after),
        vec![
            ("Chapter 1".to_string(), vec!["Section 1.2".to_string()]),
            ("Chapter 2".to_string(), vec!["Section 2.1".to_string()]),
        ],
        "Section 1.1 is no longer a direct child of Chapter 1"
    );
}

/// Chapter 2 nested under Chapter 1, carrying its own closed section.
///
/// Kept separate from the dirty-set test above because here the root **is**
/// legitimately rewritten — Chapter 2 was the last top-level item, so the
/// root's `/Last` moved — while its `/Count` stays 4, since a closed Chapter 2
/// contributes exactly 1 whether the root sees it at the top level or one
/// level down inside an open Chapter 1.
#[test]
fn a_bookmark_can_be_nested_under_its_sibling() {
    let mut s = session();
    let report = s
        .move_outline_item(CH2, OutlinePlacement::LastChild { parent: Some(CH1) })
        .expect("nest Chapter 2 under Chapter 1");
    assert!(report.moved && report.reparented);
    assert_eq!(report.from_parent, ROOT);
    assert_eq!(report.to_parent, CH1);
    assert_eq!(
        report.visible_items, 1,
        "a CLOSED chapter carries 1, not its subtree size"
    );

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);
    assert_eq!(count_of(&after, ROOT), Some(4));
    assert_eq!(
        count_of(&after, CH1),
        Some(3),
        "Chapter 1 gained one visible item — Chapter 2 itself, not its section"
    );
    assert_eq!(
        count_of(&after, CH2),
        Some(-1),
        "the moved item's own expansion state is preserved"
    );
    assert_eq!(
        shape(&after),
        vec![(
            "Chapter 1".to_string(),
            vec![
                "Section 1.1".to_string(),
                "Section 1.2".to_string(),
                "Chapter 2".to_string()
            ]
        )],
        "one top-level item now, with Chapter 2 last among its children"
    );
}

/// ★★ A section moved into the **closed** chapter — the case that discriminates
/// "propagate to the root" from "stop at the first closed node".
///
/// Section 1.1 is visible today (Chapter 1 is open) and invisible afterwards
/// (Chapter 2 is closed). So the root must go 4 → **3**. An implementation that
/// walked the insertion chain past the closed node would add back what it
/// subtracted and leave the root at 4 — a file claiming to show four items
/// while a reader can see three.
#[test]
fn moving_into_a_closed_parent_hides_the_item_and_the_root_says_so() {
    let mut s = session();
    s.move_outline_item(SEC11, OutlinePlacement::LastChild { parent: Some(CH2) })
        .expect("move Section 1.1 under Chapter 2");

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);
    assert_eq!(
        count_of(&after, ROOT),
        Some(3),
        "Section 1.1 became invisible: the root must lose it"
    );
    assert_eq!(
        count_of(&after, CH1),
        Some(1),
        "Chapter 1 has one child left"
    );
    assert_eq!(
        count_of(&after, CH2),
        Some(-2),
        "a CLOSED parent's magnitude still grows — it is what WOULD be visible \
         — and the sign must survive: -2, not 0 and not 2"
    );
}

/// A leaf that becomes a parent is left **open**, so the bookmark the operator
/// just moved is not invisible the instant it lands.
///
/// Section 2.1 has no `/Count` at all (Table 153 makes it required only when
/// there are descendants). Gaining one child gives it `/Count 1` — positive,
/// therefore open. Its own parent, Chapter 2, is closed, so the root does not
/// see the arrival and drops to 3.
#[test]
fn a_leaf_that_gains_a_child_is_left_open() {
    let mut s = session();
    assert_eq!(
        count_of(&reload(&s), SEC21),
        None,
        "fixture precondition: Section 2.1 is a leaf and carries no /Count"
    );

    s.move_outline_item(
        SEC12,
        OutlinePlacement::LastChild {
            parent: Some(SEC21),
        },
    )
    .expect("move Section 1.2 under Section 2.1");

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);
    assert_eq!(
        count_of(&after, SEC21),
        Some(1),
        "the new parent is OPEN — a positive count — not collapsed"
    );
    assert_eq!(
        count_of(&after, CH2),
        Some(-2),
        "Chapter 2's magnitude grows by the item now visible inside it"
    );
    assert_eq!(count_of(&after, CH1), Some(1));
    assert_eq!(count_of(&after, ROOT), Some(3));
}

/// ★★ Promoting the only child of a collapsed chapter: the branch where an
/// ancestor's magnitude reaches **zero** and `/Count` must be removed
/// entirely.
///
/// Section 2.1 is Chapter 2's only child. Once it leaves, Chapter 2 has no
/// descendants, and Table 153 makes `/Count` *"required if the item has any
/// descendants"* — so a leaf carrying `/Count 0` is a leaf claiming to be a
/// collapsed subtree, and `/Count -0` is not even a distinct value. The key
/// must go.
///
/// Meanwhile the root **gains**: Section 2.1 was hidden inside a closed chapter
/// and is now a visible top-level item. 4 → 5.
#[test]
fn promoting_an_only_child_out_of_a_closed_parent_clears_its_count() {
    let mut s = session();
    s.move_outline_item(SEC21, OutlinePlacement::FirstChild { parent: None })
        .expect("promote Section 2.1 to the top level");

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);
    assert_eq!(
        count_of(&after, CH2),
        None,
        "Chapter 2 is a leaf now: /Count must be absent, not 0"
    );
    assert_eq!(link(&after, CH2, b"First"), None);
    assert_eq!(link(&after, CH2, b"Last"), None);
    assert_eq!(
        count_of(&after, ROOT),
        Some(5),
        "a previously hidden item became a visible top-level one"
    );
    assert_eq!(
        shape(&after)
            .iter()
            .map(|(t, _)| t.clone())
            .collect::<Vec<_>>(),
        vec!["Section 2.1", "Chapter 1", "Chapter 2"],
        "it landed FIRST, in front of Chapter 1"
    );
}

/// The subtree travels with the item, and `visible_items` reports what moved.
///
/// Chapter 1 is open with two sections, so it carries **3** visible items into
/// a closed Chapter 2 — where all three become invisible. The root drops 4 → 1.
#[test]
fn the_subtree_travels_and_the_report_says_how_much() {
    let mut s = session();
    let report = s
        .move_outline_item(CH1, OutlinePlacement::LastChild { parent: Some(CH2) })
        .expect("nest Chapter 1 under Chapter 2");
    assert_eq!(
        report.visible_items, 3,
        "an OPEN chapter carries itself plus its two visible sections"
    );

    let after = reload(&s);
    assert_chain_is_sound(&after);
    assert_root_count_matches_reader(&after);
    assert_eq!(
        count_of(&after, ROOT),
        Some(1),
        "only Chapter 2 is visible now"
    );
    assert_eq!(count_of(&after, CH2), Some(-4));
    let tree = read_outline(&after.view());
    let ch1 = tree.items[0]
        .children
        .iter()
        .find(|c| c.title == "Chapter 1");
    assert_eq!(
        ch1.map(|c| c.children.len()),
        Some(2),
        "Chapter 1's sections came with it"
    );
}

// ---------------------------------------------------------------------------
// 3. REFUSALS
// ---------------------------------------------------------------------------

/// Moving a bookmark under one of its own descendants would make `/Parent` a
/// **cycle** — a file that still parses and that a reader without a depth
/// guard walks forever. Refused by name.
#[test]
fn a_bookmark_cannot_be_moved_into_its_own_subtree() {
    let mut s = session();
    let err = s
        .move_outline_item(
            CH1,
            OutlinePlacement::LastChild {
                parent: Some(SEC11),
            },
        )
        .expect_err("a cycle must be refused");
    match err {
        EditError::OutlineMoveIntoOwnSubtree { item, target } => {
            assert_eq!(item, CH1);
            assert_eq!(target, SEC11);
        }
        other => panic!("wrong error: {other:?}"),
    }
    assert!(
        s.dirty_set().is_empty(),
        "a refusal must leave the session byte-for-byte untouched"
    );
}

/// The degenerate cycle: moving an item under **itself**.
#[test]
fn a_bookmark_cannot_be_made_its_own_child() {
    let mut s = session();
    assert!(matches!(
        s.move_outline_item(CH1, OutlinePlacement::FirstChild { parent: Some(CH1) }),
        Err(EditError::OutlineMoveIntoOwnSubtree { .. })
    ));
}

/// The outline root is not an item: it has no `/Parent`, no `/Title`, and no
/// siblings to be positioned among.
#[test]
fn the_outline_root_cannot_be_moved() {
    let mut s = session();
    assert!(matches!(
        s.move_outline_item(ROOT, OutlinePlacement::FirstChild { parent: None }),
        Err(EditError::OutlineRootIsNotAnItem { id }) if id == ROOT
    ));
}

/// A destination that is not in this document's outline is refused rather than
/// silently re-homed to the root — see `is_under_outline_root` for the
/// page-tree bug that made reachability, not key-presence, the test.
#[test]
fn a_destination_outside_the_outline_is_refused() {
    let mut s = session();
    // Object 2 is the page tree root: a dictionary, with a /Parent-shaped
    // neighbourhood, and nothing to do with the outline.
    let page_tree = ObjId {
        num: 2,
        generation: 0,
    };
    assert!(matches!(
        s.move_outline_item(
            CH1,
            OutlinePlacement::LastChild {
                parent: Some(page_tree)
            }
        ),
        Err(EditError::OutlineItemNotFound { id: 2 })
    ));
    assert!(s.dirty_set().is_empty());
}

// ---------------------------------------------------------------------------
// 4. THE NO-OP, WHICH MUST COST NOTHING
// ---------------------------------------------------------------------------

/// "Put it where it already is" is a legitimate request with a legitimate
/// answer: nothing. A shell rebuilding an outline top-down issues redundant
/// moves **by construction**, so this must not be an error — and it must not
/// create an undo entry that undoes nothing either.
#[test]
fn moving_a_bookmark_to_where_it_already_is_writes_nothing() {
    for placement in [
        OutlinePlacement::Before { sibling: CH2 },
        OutlinePlacement::FirstChild { parent: None },
        OutlinePlacement::After { sibling: CH1 },
    ] {
        let mut s = session();
        let report = s
            .move_outline_item(CH1, placement)
            .expect("a redundant move is not an error");
        assert!(!report.moved, "{placement:?} is where Chapter 1 already is");
        assert!(
            s.dirty_set().is_empty(),
            "{placement:?} produced a dirty object"
        );
        assert!(
            s.undo().is_none(),
            "{placement:?} created an undo entry for an edit that did not happen"
        );
    }
}

// ---------------------------------------------------------------------------
// 5. UNDO — the §11.1 contract
// ---------------------------------------------------------------------------

/// A move followed by an undo must leave the document **byte-identical**, not
/// merely equivalent.
///
/// `ARCHITECTURE.md` §11.1: the dirty set is a diff against the base at save
/// time, never a log of what was touched. A move rewrites up to nine
/// dictionaries; undoing it must return every one of them to its base value so
/// the incremental save has nothing to append.
#[test]
fn undoing_a_move_restores_the_file_byte_for_byte() {
    let original = std::fs::read(fixture("outline/basic-tree.pdf")).expect("read fixture");
    let mut s = session();
    s.move_outline_item(CH2, OutlinePlacement::FirstChild { parent: Some(CH1) })
        .expect("move");
    assert!(!s.dirty_set().is_empty(), "the move did something");

    s.undo().expect("the move is on the undo stack");
    assert!(
        s.dirty_set().is_empty(),
        "after undo, nothing differs from the base revision"
    );
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    assert_eq!(
        bytes, original,
        "edit -> undo -> save must reproduce the input exactly"
    );
}

/// Two moves in a row, then two undos, restores the original structure —
/// the ordering check a single move cannot make.
#[test]
fn two_moves_undo_in_order() {
    let mut s = session();
    let before = shape(&reload(&s));
    s.move_outline_item(CH1, OutlinePlacement::After { sibling: CH2 })
        .expect("reorder");
    s.move_outline_item(SEC11, OutlinePlacement::LastChild { parent: Some(CH2) })
        .expect("re-parent");
    s.undo().expect("undo the re-parent");
    s.undo().expect("undo the reorder");
    assert_eq!(shape(&reload(&s)), before);
    assert!(s.dirty_set().is_empty());
}

// ---------------------------------------------------------------------------
// 6. EXPAND / COLLAPSE — the other answer to the collapsed-parent question
// ---------------------------------------------------------------------------

/// Expanding a closed chapter reveals its descendants to every ancestor that
/// can see them: the root gains the **magnitude**, not one.
#[test]
fn expanding_a_bookmark_reveals_its_count_to_the_root() {
    let mut s = session();
    assert!(s.set_outline_open(CH2, true).expect("expand Chapter 2"));

    let after = reload(&s);
    assert_root_count_matches_reader(&after);
    assert_eq!(count_of(&after, CH2), Some(1), "the sign flipped to open");
    assert_eq!(
        count_of(&after, ROOT),
        Some(5),
        "Section 2.1 became visible: the root gains 1, the magnitude that was hidden"
    );
}

/// Collapsing is the same operation with the sign the other way, and the pair
/// round-trips to the original bytes.
#[test]
fn collapsing_then_expanding_restores_the_file() {
    let original = std::fs::read(fixture("outline/basic-tree.pdf")).expect("read fixture");
    let mut s = session();
    assert!(s.set_outline_open(CH1, false).expect("collapse Chapter 1"));
    let mid = reload(&s);
    assert_eq!(count_of(&mid, CH1), Some(-2), "closed, magnitude preserved");
    assert_eq!(
        count_of(&mid, ROOT),
        Some(2),
        "two sections went out of view"
    );
    assert_root_count_matches_reader(&mid);

    assert!(s.set_outline_open(CH1, true).expect("expand it again"));
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    assert_eq!(
        bytes, original,
        "collapse then expand is a round trip to the base revision"
    );
}

/// A leaf has no expansion state, and a bookmark already in the requested
/// state has nothing to do. Both report `false` rather than erroring: a
/// "collapse all" sweep walks every row and must not have to filter first.
#[test]
fn expanding_a_leaf_or_an_already_open_item_does_nothing() {
    let mut s = session();
    assert!(
        !s.set_outline_open(SEC11, true)
            .expect("a leaf is not an error"),
        "a leaf carries no /Count and has nothing to expand"
    );
    assert!(
        !s.set_outline_open(CH1, true).expect("already open"),
        "Chapter 1 is open already"
    );
    assert!(s.dirty_set().is_empty(), "neither call may write anything");
}

/// The root's `/Count` is the *other* quantity and cannot be negative, so it
/// has no open/closed state to set.
#[test]
fn the_root_has_no_expansion_state() {
    let mut s = session();
    assert!(matches!(
        s.set_outline_open(ROOT, false),
        Err(EditError::OutlineRootIsNotAnItem { .. })
    ));
}

// ---------------------------------------------------------------------------
// 7. THE CYCLE CHECK AT DEPTH — the property the first implementation could
//    not guarantee
// ---------------------------------------------------------------------------

/// ★★ A cycle must be refused for a destination **anywhere** under the item,
/// not just a shallow one.
///
/// This exists because the first implementation asked the question
/// **downward** — build the item's subtree with `outline_subtree`, test
/// membership — and `outline_subtree` carries a **breadth** guard
/// (`MAX_OUTLINE_ITEMS`, 200 000) as well as a depth one. A subtree wider than
/// that truncates, so a destination beyond the cut tests as *not in the
/// subtree* and the move is allowed, **authoring the cycle the check exists to
/// refuse**.
///
/// The fix asks it **upward** instead — walk the destination's `/Parent` chain
/// looking for the item — which is bounded by `MAX_OUTLINE_DEPTH` and cannot
/// be defeated by breadth at all.
///
/// A 200 000-item fixture is not something to check in (project rule 7, and it
/// would dominate the suite's runtime). What *is* testable, and is the other
/// half of the completeness claim, is that the upward walk reaches the **full
/// depth**: `deep.pdf` is a 32-level chain, so refusing a move of the root-most
/// item under the deepest one exercises the walk to its cap. A walk that
/// stopped early would allow it.
#[test]
fn a_cycle_is_refused_at_the_full_depth_of_the_walk() {
    let deep =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/outline/deep.pdf");
    let mut s = EditSession::new(Document::load(&deep).expect("load deep.pdf"));

    let chain: Vec<ObjId> = read_outline(&s.graph())
        .flatten()
        .iter()
        .map(|i| i.id)
        .collect();
    assert!(
        chain.len() >= 30,
        "fixture precondition: deep.pdf is a long chain, got {}",
        chain.len()
    );
    let top = chain[0];

    // Every descendant, from the shallowest to the deepest the reader can see.
    for (depth, &target) in chain.iter().enumerate().skip(1) {
        let err = s
            .move_outline_item(
                top,
                OutlinePlacement::LastChild {
                    parent: Some(target),
                },
            )
            .expect_err("moving the top item under its own descendant is a cycle");
        assert!(
            matches!(err, EditError::OutlineMoveIntoOwnSubtree { .. }),
            "at depth {depth} the refusal was {err:?}, not a cycle refusal — the \
             ancestry walk stopped before reaching the item"
        );
    }
    assert!(
        s.dirty_set().is_empty(),
        "every one of those was a refusal; none may have written anything"
    );
}

/// The mirror: a legal move within that same deep chain still succeeds, so the
/// test above is refusing cycles rather than refusing everything.
///
/// Without this, an implementation that returned `OutlineMoveIntoOwnSubtree`
/// unconditionally would pass the test above perfectly.
#[test]
fn a_legal_move_in_the_same_deep_chain_still_succeeds() {
    let deep =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/outline/deep.pdf");
    let mut s = EditSession::new(Document::load(&deep).expect("load deep.pdf"));
    let chain: Vec<ObjId> = read_outline(&s.graph())
        .flatten()
        .iter()
        .map(|i| i.id)
        .collect();
    let deepest = *chain.last().expect("the chain is not empty");

    let report = s
        .move_outline_item(deepest, OutlinePlacement::FirstChild { parent: None })
        .expect("promoting the deepest item to the top level is legal");
    assert!(report.moved && report.reparented);
    let after = reload(&s);
    assert_chain_is_sound(&after);
}
