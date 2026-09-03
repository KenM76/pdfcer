//! # Editing an existing bookmark (`Pass 156.0`)
//!
//! Bookmarks could be **created** and not changed: no rename, no delete, no
//! reorder. Renaming one is the commonest bookmark edit there is, and it had
//! no route at all.
//!
//! ## ★★ Why deleting one is not a `remove` call
//!
//! An outline is a doubly-linked sibling chain inside a tree (§12.3.3
//! Tables 152–153). Removing one node touches at least five other
//! dictionaries: the previous sibling's `/Next` (or the parent's `/First`),
//! the next sibling's `/Prev` (or the parent's `/Last`), every open ancestor's
//! `/Count`, and the root's `/Count` — which counts a **different quantity**.
//!
//! Leave any one and the tree is still parseable but wrong: a reader walks
//! `/First` then `/Next` and either stops early or revisits. **That is the
//! failure these tests exist for**, and it is invisible to a test that only
//! checks the deleted item is gone.
//!
//! ## ★ `/Count` is two quantities, and this is where that bites
//!
//! On an **item**, `/Count` counts visible *descendants*, excluding itself,
//! and its **sign carries the open/closed state**. On the **root** it counts
//! all visible items *including* the top level and cannot be negative. The
//! spec corpus calls confusing the two the single most common error against
//! this clause.
//!
//! A **closed** item therefore contributes exactly **1** to its ancestor's
//! count no matter how large its subtree is — so deleting one must subtract
//! 1, not the subtree size.
//!
//! Fixture provenance: `fixtures/synthetic/outline/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::outline::read_outline;
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

/// Every title in the outline, in reading order, from the SAVED bytes through
/// the ordinary reader — never from the edit session.
fn titles(doc: &Document) -> Vec<String> {
    fn walk(items: &[pdfcer_core::outline::OutlineItem], out: &mut Vec<String>) {
        for it in items {
            out.push(it.title.clone());
            walk(&it.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&read_outline(&doc.view()).items, &mut out);
    out
}

/// The object id of the first top-level item, the way a shell gets one.
fn first_item(s: &EditSession) -> ObjId {
    let outline = read_outline(&s.graph());
    outline
        .items
        .first()
        .map(|i| i.id)
        .expect("the fixture has a top-level item")
}

fn dict_of(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("not a dictionary: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. RENAME
// ---------------------------------------------------------------------------

#[test]
fn a_bookmark_can_be_renamed() {
    let mut s = session("outline/basic-tree.pdf");
    let id = first_item(&s);
    let before = titles(&reload(&s));

    s.set_outline_title(id, "Renamed chapter")
        .expect("rename must work");

    let after = titles(&reload(&s));
    assert_eq!(
        after.len(),
        before.len(),
        "renaming must not change the tree's shape"
    );
    assert_eq!(after[0], "Renamed chapter");
    assert_eq!(
        &after[1..],
        &before[1..],
        "no other title may move or change"
    );
}

/// Titles are §7.9.2 text strings, so a name with an em dash or an accent has
/// to survive. `Pass 150.0` shipped a defect from two encoders disagreeing
/// about PDFDocEncoding; this asserts the round trip rather than assuming it.
#[test]
fn a_renamed_bookmark_round_trips_non_ascii() {
    let mut s = session("outline/basic-tree.pdf");
    let id = first_item(&s);
    s.set_outline_title(id, "Prüfung — Kapitel ‘eins’")
        .expect("rename");
    assert_eq!(titles(&reload(&s))[0], "Prüfung — Kapitel ‘eins’");
}

#[test]
fn undo_restores_the_previous_title() {
    let mut s = session("outline/basic-tree.pdf");
    let id = first_item(&s);
    let before = titles(&reload(&s))[0].clone();
    s.set_outline_title(id, "Temporary").expect("rename");
    assert!(s.undo().is_some());
    assert_eq!(titles(&reload(&s))[0], before);
}

// ---------------------------------------------------------------------------
// 2. DELETE — and the linkage nobody sees until a reader walks it
// ---------------------------------------------------------------------------

/// ★ The claim that matters. After deleting the first top-level item, the
/// tree must still be **walkable**: the reader follows `/First` then `/Next`,
/// so a stale `/First` or a dangling `/Prev` shows up as a missing or
/// repeated title — not as a parse error.
#[test]
fn deleting_the_first_item_relinks_the_chain() {
    let mut s = session("outline/basic-tree.pdf");
    let before = titles(&reload(&s));
    let id = first_item(&s);
    let doomed = before[0].clone();

    let removed = s.delete_outline_item(id).expect("delete must work");
    assert!(removed >= 1, "at least the item itself was removed");

    // ★ EXACTLY this list, not merely a shorter one.
    //
    // "Fewer than before" is the assertion three separate sabotages walked
    // straight through — dropping the parent's `/First` update, skipping the
    // `/Count` decrement, and mis-counting a closed subtree all leave a list
    // that is shorter. One of them leaves it EMPTY, which is "shorter" too.
    //
    // The fixture is Chapter 1 (+ two sections) and Chapter 2 (+ one), so
    // deleting Chapter 1 must leave precisely its sibling and that sibling's
    // child, in order.
    let after = titles(&reload(&s));
    assert_eq!(
        after,
        vec!["Chapter 2".to_owned(), "Section 2.1".to_owned()],
        "the surviving tree must be exactly Chapter 2 and its section - a stale /First leaves this EMPTY, and a stale /Next repeats an entry"
    );
    assert!(!after.contains(&doomed));
    assert!(after.len() < before.len());
}

/// The root's `/Count` counts a **different quantity** from an item's, and it
/// *"cannot be negative"* (Table 152). A delete that subtracted the wrong
/// number leaves a root claiming more or fewer visible items than exist.
#[test]
fn the_root_count_stays_consistent_with_what_a_reader_sees() {
    let mut s = session("outline/basic-tree.pdf");
    let id = first_item(&s);
    s.delete_outline_item(id).expect("delete");

    let doc = reload(&s);
    let root_id = match doc
        .view()
        .catalog_dict()
        .and_then(|c| c.get(b"Outlines").cloned())
    {
        Some(Object::Reference(r)) => r,
        other => panic!("the fixture has an /Outlines reference, got {other:?}"),
    };
    let root = dict_of(&doc, root_id);
    if let Some(Object::Integer(n)) = root.get(b"Count") {
        assert!(
            *n >= 0,
            "Table 152: the root's /Count cannot be negative, got {n}"
        );
    }
}

#[test]
fn deleting_a_parent_deletes_its_children() {
    let mut s = session("outline/basic-tree.pdf");
    let before = titles(&reload(&s));
    let outline = read_outline(&s.graph());
    let parent = outline
        .items
        .iter()
        .find(|i| !i.children.is_empty())
        .expect("the fixture has a parent with children");
    let child_titles: Vec<String> = parent.children.iter().map(|c| c.title.clone()).collect();
    let id = parent.id;

    s.delete_outline_item(id).expect("delete");

    let after = titles(&reload(&s));
    for child in &child_titles {
        assert!(
            !after.contains(child),
            "a child of the deleted item survived: {child:?} in {after:?}"
        );
    }
    assert!(after.len() < before.len());
}

// ---------------------------------------------------------------------------
// 3. REFUSALS
// ---------------------------------------------------------------------------

/// The outline ROOT is not an item: no `/Parent`, no `/Title`, and a `/Count`
/// that counts a different quantity. Deleting it means deleting the whole
/// outline, which is a different act.
#[test]
fn the_outline_root_is_refused_by_name() {
    let mut s = session("outline/basic-tree.pdf");
    let root = match s
        .graph()
        .catalog_dict()
        .and_then(|c| c.get(b"Outlines").cloned())
    {
        Some(Object::Reference(r)) => r,
        other => panic!("expected an /Outlines reference, got {other:?}"),
    };
    match s.delete_outline_item(root) {
        Err(EditError::OutlineRootIsNotAnItem { id }) => assert_eq!(id, root),
        other => panic!("the root must be refused, got {other:?}"),
    }
}

/// ★★★ The `/Count` arithmetic, asserted as a NUMBER — added after two
/// sabotages walked through every other test in this file.
///
/// Nothing above can see `/Count`: `read_outline` reconstructs the tree from
/// `/First`/`/Next`, so skipping the decrement entirely, or mis-counting a
/// closed subtree, leaves every title exactly where it was. A reader still
/// walks the tree; a **bookmarks panel** that trusts `/Count` to size its
/// scrollbar or decide what is collapsed does not.
///
/// The fixture is:
///
/// ```text
///   root                /Count  4   (Ch1 + its 2 visible sections + Ch2)
///     Chapter 1         /Count  2   OPEN, two visible descendants
///       Section 1.1
///       Section 1.2
///     Chapter 2         /Count -1   CLOSED, one descendant that would show
///       Section 2.1
/// ```
///
/// Deleting Chapter 1 removes **3 visible items** — itself plus its two
/// visible descendants — so the root must go 4 → 1. Chapter 2 stays closed
/// and untouched at `-1`.
///
/// ★ And this is where the two-quantities trap bites: had Chapter 1 been
/// CLOSED it would contribute exactly **1**, not 3, however large its subtree.
/// A delete that subtracted the subtree size would take the root to a wrong
/// number — or negative, which Table 152 forbids outright.
#[test]
fn deleting_an_open_subtree_subtracts_its_visible_items_and_no_more() {
    let mut s = session("outline/basic-tree.pdf");

    let before = reload(&s);
    let root_id = match before
        .view()
        .catalog_dict()
        .and_then(|c| c.get(b"Outlines").cloned())
    {
        Some(Object::Reference(r)) => r,
        other => panic!("expected /Outlines, got {other:?}"),
    };
    assert_eq!(
        dict_of(&before, root_id).get(b"Count"),
        Some(&Object::Integer(4)),
        "fixture precondition: the root starts at 4"
    );

    let id = first_item(&s);
    s.delete_outline_item(id).expect("delete Chapter 1");

    let after = reload(&s);
    assert_eq!(
        dict_of(&after, root_id).get(b"Count"),
        Some(&Object::Integer(1)),
        "Chapter 1 was OPEN with two visible descendants, so 3 visible items \
         went: the root must read 1, not 3 (decrement skipped) and not -1 \
         (subtree size subtracted)"
    );

    // Chapter 2 is a sibling, not an ancestor — its own /Count must not move,
    // and it must stay NEGATIVE, because it is still closed.
    let ch2 = read_outline(&after.view())
        .items
        .first()
        .map(|i| i.id)
        .expect("Chapter 2 survives");
    assert_eq!(
        dict_of(&after, ch2).get(b"Count"),
        Some(&Object::Integer(-1)),
        "a sibling's /Count and its closed state must both be untouched"
    );
}

/// ★★ Deleting the CLOSED chapter — the case that discriminates "visible
/// items" from "subtree size", and the one the test above cannot see.
///
/// The test above deletes Chapter 1, which is **open**, where the two
/// quantities coincide (3 visible items, 3 nodes). Chapter 2 is **closed**
/// with one hidden descendant: it contributes exactly **1** to the root's
/// count, not 2, because its section was never visible.
///
/// So the root must go 4 → 3. A delete that subtracted the subtree size would
/// give 2 — and on a deeper closed subtree it would drive the root negative,
/// which Table 152 forbids outright.
///
/// Added after a sabotage changing `/Count > 0` to `/Count != 0` in the
/// visible-count helper left every other test in this file green.
#[test]
fn deleting_a_closed_subtree_subtracts_one_not_its_size() {
    let mut s = session("outline/basic-tree.pdf");

    let closed = read_outline(&s.graph())
        .items
        .get(1)
        .map(|i| i.id)
        .expect("Chapter 2");
    let doc0 = reload(&s);
    assert_eq!(
        dict_of(&doc0, closed).get(b"Count"),
        Some(&Object::Integer(-1)),
        "fixture precondition: Chapter 2 is CLOSED with one hidden descendant"
    );

    let root_id = match doc0
        .view()
        .catalog_dict()
        .and_then(|c| c.get(b"Outlines").cloned())
    {
        Some(Object::Reference(r)) => r,
        other => panic!("expected /Outlines, got {other:?}"),
    };

    s.delete_outline_item(closed).expect("delete Chapter 2");

    let after = reload(&s);
    assert_eq!(
        dict_of(&after, root_id).get(b"Count"),
        Some(&Object::Integer(3)),
        "a CLOSED item contributes 1 to its ancestor's count however large its \
         subtree, so the root must read 3 — 2 means the subtree size was \
         subtracted instead"
    );
    assert_eq!(
        titles(&after),
        vec![
            "Chapter 1".to_owned(),
            "Section 1.1".to_owned(),
            "Section 1.2".to_owned()
        ],
        "and its hidden child must go with it"
    );
}
