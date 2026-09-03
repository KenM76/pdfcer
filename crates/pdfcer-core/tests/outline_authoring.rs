//! Integration tests for the document-outline **writer**
//! ([`EditSession::add_outline_item`], ISO 32000-1 §12.3.3 / §12.3.2.2).
//!
//! ## Why these run through whole saved files
//!
//! The sibling `outline.rs` makes the case for reading parsed bytes rather
//! than a hand-built graph. Authoring has a second reason on top of it, and
//! it is the stronger one: **the only thing that matters about a bookmark is
//! what a viewer's bookmark panel shows**, and that panel is driven by
//! `/First`/`/Last`/`/Prev`/`/Next`/`/Count` in the *saved* bytes. A test
//! that asserted on the dictionary keys pdfcer just inserted would pass for a
//! writer that produced a tree no viewer could walk — it would be checking
//! that the code did what the code does.
//!
//! So every test here follows the same three steps: **add, save
//! incrementally, reparse, then read the outline back with the reader** and
//! assert on the tree an operator would see. The reader is independent code
//! with its own test suite, which makes it a usable oracle: a writer bug and
//! a reader bug would have to agree with each other to hide.
//!
//! ## The fixture, and why this one
//!
//! `basic-tree.pdf` (synthetic, byte-authored — `docs/LEGAL.md` §5 category
//! (a), provenance in `fixtures/synthetic/outline/PROVENANCE.md`) is the
//! only fixture that carries **both** an open and a closed item, which is
//! precisely the distinction `/Count` propagation turns on. Its shape:
//!
//! ```text
//! /Outlines  /Count 4          <- visible items at EVERY level
//!   Chapter 1  /Count +2       <- OPEN, two children
//!     Section 1.1
//!     Section 1.2
//!   Chapter 2  /Count -1       <- CLOSED, one child (not visible)
//!     Section 2.1
//! ```
//!
//! Five items exist; four are visible. Those numbers are pinned by
//! `outline.rs::a_well_formed_outline_reads_faithfully`, so if this file's
//! premise ever breaks, that test says so first and names the fixture.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::outline::{DestView, Destination, Outline, RemoteTarget, read_outline};
use pdfcer_core::writer::SaveOptions;

const OUTLINE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/outline/"
);

fn session(name: &str) -> EditSession {
    let bytes = std::fs::read(format!("{OUTLINE_DIR}{name}"))
        .unwrap_or_else(|error| panic!("fixture {name} unreadable: {error}"));
    EditSession::new(
        Document::from_bytes(bytes)
            .unwrap_or_else(|error| panic!("fixture {name} did not parse: {error}")),
    )
}

/// Save incrementally and read the outline back out of the **saved bytes**.
///
/// The whole oracle in one function: nothing below inspects the session's
/// object overlay, because a viewer cannot.
fn saved_outline(session: &EditSession) -> Outline {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("pdfcer's own output must reparse");
    read_outline(&doc)
}

/// A destination pointing at page 0 with no view parameters.
fn page0() -> Destination {
    Destination::Page {
        page_index: 0,
        view: DestView::Fit,
    }
}

// ---------------------------------------------------------------------------
// The tree a viewer walks
// ---------------------------------------------------------------------------

/// Would catch: appending an item without wiring `/Prev` on the newcomer or
/// `/Next` on the previous last sibling — the single most likely splice bug,
/// and one that is **invisible to a forward walk**.
///
/// This test covers the **forward** half only, and says so because the
/// distinction cost a sabotage run to find: pdfcer's reader walks `/First`
/// then `/Next` and never reads `/Prev`, so no assertion routed through it
/// can see a missing back-link. The backward half is
/// [`the_sibling_chain_is_walkable_backward_from_last_to_first`], which reads
/// raw dictionaries for exactly that reason.
#[test]
fn a_new_top_level_item_lands_last_and_the_chain_links_both_ways() {
    let mut s = session("basic-tree.pdf");
    let id = s
        .add_outline_item(None, "Appendix", Some(page0()))
        .expect("adding a top-level bookmark must succeed");
    assert!(id.num > 0, "a real object number must be allocated");

    let outline = saved_outline(&s);
    assert_eq!(
        outline.items.len(),
        3,
        "the two existing chapters plus the newcomer"
    );
    assert_eq!(outline.items[0].title, "Chapter 1");
    assert_eq!(outline.items[1].title, "Chapter 2");
    assert_eq!(
        outline.items[2].title, "Appendix",
        "appended LAST, not first"
    );
    assert_eq!(outline.items[2].children.len(), 0);
    assert_eq!(outline.items[2].level, 0);

    // The existing items must be undisturbed — a splice that rebuilt the
    // chain instead of extending it would reorder or drop them.
    assert_eq!(outline.items[0].children.len(), 2);
    assert_eq!(outline.items[1].children.len(), 1);
    assert_eq!(outline.diagnostics.items, 6);
    assert!(
        outline.diagnostics.is_faithful(),
        "the writer must not introduce a diagnostic: {:?}",
        outline.diagnostics
    );
}

/// Would catch: the destination being written with its parameters in the
/// wrong slot, or the page reference pointing at the page *object number*
/// pdfcer happened to allocate rather than the real page.
///
/// The reader maps a destination back to a **0-based page index**, so this
/// asserts the round trip closes on the number the caller passed rather than
/// on an object id, which is the form the bug would take.
#[test]
fn an_explicit_destination_round_trips_to_the_page_the_caller_named() {
    let mut s = session("basic-tree.pdf");
    s.add_outline_item(
        None,
        "Figure 3",
        Some(Destination::Page {
            page_index: 1,
            view: DestView::Xyz {
                left: Some(72.0),
                top: Some(700.0),
                zoom: None,
            },
        }),
    )
    .expect("adding must succeed");

    let outline = saved_outline(&s);
    let added = outline.items.last().expect("the item must be there");
    match &added.destination {
        Some(Destination::Page { page_index, view }) => {
            assert_eq!(*page_index, 1, "the page the caller named, not object 1");
            match view {
                DestView::Xyz { left, top, zoom } => {
                    assert_eq!(*left, Some(72.0));
                    assert_eq!(*top, Some(700.0));
                    assert_eq!(
                        *zoom, None,
                        "a None parameter must survive as null, not vanish and shift the others"
                    );
                }
                other => panic!("view changed type on the round trip: {other:?}"),
            }
        }
        other => panic!("destination did not round-trip: {other:?}"),
    }
}

/// Would catch: nesting silently landing at the top level. `parent` is an
/// `Option<ObjId>` and `None` already means top-level, so a verb that
/// mishandled `Some` would produce a *plausible* outline — the bookmark
/// exists, it just sits in the wrong place, which reads as the operator's
/// mistake rather than pdfcer's.
#[test]
fn a_child_lands_under_its_parent_and_not_at_the_top() {
    let mut s = session("basic-tree.pdf");
    let chapter1 = first_item_id(&s);
    s.add_outline_item(Some(chapter1), "Section 1.3", Some(page0()))
        .expect("nesting must succeed");

    let outline = saved_outline(&s);
    assert_eq!(
        outline.items.len(),
        2,
        "still two top-level chapters — the child must NOT have surfaced"
    );
    let chapter = &outline.items[0];
    assert_eq!(chapter.children.len(), 3);
    assert_eq!(chapter.children[0].title, "Section 1.1");
    assert_eq!(chapter.children[2].title, "Section 1.3", "appended last");
    assert_eq!(chapter.children[2].level, 1);
}

/// The outline root's object id for the fixture's first chapter, read the
/// way a shell would: from the reader, not from a hard-coded object number.
fn first_item_id(session: &EditSession) -> ObjId {
    let outline = read_outline(&session.view());
    outline.items[0].id
}

// ---------------------------------------------------------------------------
// /Count — the half the spec digest opens with
// ---------------------------------------------------------------------------

/// Would catch: incrementing every ancestor unconditionally.
///
/// This is *the* propagation bug, and the fixture is built to expose it.
/// Chapter 2 is **closed** (`/Count -1`), so a section added under it is not
/// visible and the root's count must stay at 4. A writer that walked to the
/// root regardless would produce 5 — a number that is not obviously wrong
/// from the outside, and that makes every viewer's bookmark panel disagree
/// with the file about how many rows it has.
///
/// The closed chapter's own magnitude **must** still grow, to 2: a closed
/// item's count is what it *would* be if opened, which is how the file
/// preserves the collapsed subtree's size.
#[test]
fn adding_under_a_closed_parent_grows_that_parent_and_stops_there() {
    let mut s = session("basic-tree.pdf");
    let outline = read_outline(&s.view());
    let chapter2 = outline.items[1].id;
    assert!(
        !outline.items[1].open,
        "fixture premise: Chapter 2 is CLOSED"
    );

    s.add_outline_item(Some(chapter2), "Section 2.2", Some(page0()))
        .expect("adding must succeed");

    let after = saved_outline(&s);
    let closed = &after.items[1];
    assert_eq!(closed.children.len(), 2);
    assert_eq!(
        closed.declared_count,
        Some(-2),
        "magnitude grows, sign preserved — the subtree stays collapsed"
    );
    assert!(!closed.open, "adding a child must not silently expand it");
    assert_eq!(
        after.diagnostics.declared_root_count,
        Some(4),
        "the new item is INVISIBLE, so the root count must not move"
    );
    assert_eq!(after.visible_item_count(), 4);
    assert!(
        !after.diagnostics.root_count_disagreement,
        "the written root count must agree with the reader's own tally"
    );
}

/// Would catch: the root's `/Count` being treated as an item's — counting
/// only descendants below the top level, or carrying a sign.
///
/// The two quantities differ by exactly the number of top-level items, so a
/// writer that used the item rule on the root produces a count that is too
/// small by one here and correct in a document with no top-level additions.
#[test]
fn adding_at_the_top_level_grows_the_root_count_by_one() {
    let mut s = session("basic-tree.pdf");
    s.add_outline_item(None, "Appendix", None)
        .expect("adding must succeed");

    let after = saved_outline(&s);
    assert_eq!(
        after.diagnostics.declared_root_count,
        Some(5),
        "the root counts top-level items too"
    );
    assert_eq!(after.visible_item_count(), 5);
    assert!(!after.diagnostics.root_count_disagreement);
}

/// Would catch: the ancestor walk stopping at the immediate parent, so a
/// grandparent and the root keep stale counts.
///
/// Chapter 1 is open and so is the whole chain above the new item, meaning
/// every level must grow: Section 1.1 from leaf to `/Count 1`, Chapter 1
/// from 2 to 3, and the root from 4 to 5.
#[test]
fn an_open_chain_grows_at_every_level_up_to_the_root() {
    let mut s = session("basic-tree.pdf");
    let outline = read_outline(&s.view());
    let section11 = outline.items[0].children[0].id;
    assert_eq!(
        outline.items[0].children[0].declared_count, None,
        "fixture premise: Section 1.1 is a leaf"
    );

    s.add_outline_item(Some(section11), "Note", Some(page0()))
        .expect("adding must succeed");

    let after = saved_outline(&s);
    let chapter1 = &after.items[0];
    let section = &chapter1.children[0];
    assert_eq!(section.children.len(), 1);
    assert_eq!(
        section.declared_count,
        Some(1),
        "a leaf that gains a child becomes an OPEN parent, not a closed one"
    );
    assert!(
        section.open,
        "the operator must be able to see what they made"
    );
    assert_eq!(chapter1.declared_count, Some(3), "grandparent grew too");
    assert!(chapter1.open);
    assert_eq!(after.diagnostics.declared_root_count, Some(5));
    assert!(!after.diagnostics.root_count_disagreement);
    assert_eq!(after.diagnostics.count_disagreements, 0);
}

/// Would catch: a document with no `/Outlines` at all being refused, or the
/// created root being wired into the file but not into the catalog — which
/// produces a saved file whose outline object exists, is reachable by object
/// number, and is invisible to every viewer.
#[test]
fn a_document_with_no_outline_gets_one() {
    let mut s = session("no-outline.pdf");
    assert!(
        read_outline(&s.view()).items.is_empty(),
        "fixture premise: no outline"
    );

    s.add_outline_item(None, "First bookmark", Some(page0()))
        .expect("creating the outline must succeed");
    s.add_outline_item(None, "Second bookmark", None)
        .expect("the second must join the first");

    let after = saved_outline(&s);
    assert_eq!(after.items.len(), 2);
    assert_eq!(after.items[0].title, "First bookmark");
    assert_eq!(after.items[1].title, "Second bookmark");
    assert_eq!(after.diagnostics.declared_root_count, Some(2));
    assert!(
        after.diagnostics.is_faithful(),
        "a freshly created outline must be faithful: {:?}",
        after.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Would catch: an unauthorable destination being silently dropped, leaving
/// a bookmark that appears in the panel, looks clickable and does nothing.
///
/// Each variant is checked separately rather than as a class, because they
/// reach the refusal by two different routes — `Remote` is rejected on the
/// `Destination` enum, `Unknown`/`Absent` inside the array builder — and
/// testing one of each pair would leave the other route unmeasured.
///
/// `Named` is **not** in this list any more: `Pass 103.3` made it authorable.
/// Its own refusal — an *undefined* key — is
/// [`a_bookmark_naming_an_undefined_destination_is_refused`].
#[test]
fn destinations_pdfcer_cannot_author_are_refused_by_name() {
    let cases: &[(Destination, &str)] = &[
        (
            Destination::Remote {
                file: Some(b"other.pdf".to_vec()),
                target: RemoteTarget::PageNumber(0),
                view: DestView::Fit,
                new_window: None,
            },
            "remote",
        ),
        (
            Destination::Page {
                page_index: 0,
                view: DestView::Unknown {
                    fit: pdfcer_core::object::Name::from(b"FitSomething"),
                },
            },
            "unknown-fit",
        ),
        (
            Destination::Page {
                page_index: 0,
                view: DestView::Absent,
            },
            "no-fit-style",
        ),
    ];
    for (destination, expected) in cases {
        let mut s = session("basic-tree.pdf");
        let before = saved_outline(&s).items.len();
        match s.add_outline_item(None, "Nowhere", Some(destination.clone())) {
            Err(EditError::UnsupportedDestination { kind }) => {
                assert_eq!(kind, *expected, "the refusal must name which kind");
            }
            other => panic!("{expected} must be refused, got {other:?}"),
        }
        assert_eq!(
            saved_outline(&s).items.len(),
            before,
            "a refusal must leave no bookmark behind"
        );
    }
}

/// Would catch: a stale or foreign `parent` being re-parented to the root.
///
/// That failure mode is the dangerous one precisely because it *succeeds*:
/// the shell gets an `Ok`, the bookmark exists, and it sits at the top level
/// where the operator will read it as their own mis-click.
#[test]
fn an_unknown_parent_is_refused_rather_than_re_parented() {
    let mut s = session("basic-tree.pdf");
    let bogus = ObjId::new(9999, 0);
    match s.add_outline_item(Some(bogus), "Orphan", None) {
        Err(EditError::OutlineItemNotFound { id }) => assert_eq!(id, 9999),
        other => panic!("an unknown parent must be refused, got {other:?}"),
    }
    // A page object is a real object that is emphatically not an outline
    // item — the near-miss case a `contains_key` check could wave through.
    let page = s.page_slots().expect("pages must read")[0].id;
    match s.add_outline_item(Some(page), "Orphan", None) {
        Err(EditError::OutlineItemNotFound { .. }) => {}
        other => panic!("a page must not be accepted as a parent, got {other:?}"),
    }
    assert_eq!(
        saved_outline(&s).items.len(),
        2,
        "neither refusal may leave a bookmark behind"
    );
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

/// Would catch: the add being more than one undo entry, or an undo that
/// removes the item but leaves the ancestors' `/Count` inflated — which
/// produces a file that disagrees with itself about how many bookmarks it
/// has, and that no amount of further undoing repairs.
#[test]
fn undo_removes_the_item_and_restores_every_count() {
    let mut s = session("basic-tree.pdf");
    let before = saved_outline(&s);
    let chapter1 = before.items[0].id;

    s.add_outline_item(Some(chapter1), "Section 1.3", Some(page0()))
        .expect("adding must succeed");
    assert_eq!(saved_outline(&s).items[0].children.len(), 3);

    assert!(s.undo().is_some(), "exactly one undo entry must exist");
    let after = saved_outline(&s);
    assert_eq!(after.items[0].children.len(), 2, "the item is gone");
    assert_eq!(
        after.items[0].declared_count, before.items[0].declared_count,
        "the parent's count must return to what it was"
    );
    assert_eq!(
        after.diagnostics.declared_root_count, before.diagnostics.declared_root_count,
        "and so must the root's"
    );
    assert_eq!(after.diagnostics.items, before.diagnostics.items);
    assert!(!after.diagnostics.root_count_disagreement);
}

/// Walk a sibling chain **backward** from `/Last` through `/Prev`, returning
/// the titles in the order encountered.
///
/// ## Why this reads raw dictionaries when everything else here uses the
/// reader
///
/// The reader walks `/First` then `/Next` and never touches `/Prev`, so it is
/// **blind to a broken back-link by construction** — and blind in the
/// direction that matters, because a missing `/Prev` leaves a bookmark panel
/// looking perfect. Sabotaging the `/Prev` write left this file's entire
/// suite green, which is what put this helper here: an oracle that cannot see
/// a field cannot vouch for it, no matter how many assertions run through it.
///
/// §12.3.3 Table 153 requires `/Prev` on every item except the first, and
/// viewers use it — Acrobat's previous-bookmark navigation and its
/// drag-to-reorder both walk backward. A one-way chain is a real defect, not
/// a tidiness question.
fn back_walk(doc: &Document, parent: ObjId) -> Vec<String> {
    let Some(Object::Dict(p)) = doc.get(parent).map(|io| &io.value) else {
        panic!("parent {parent:?} is not a dictionary in the saved file");
    };
    let mut cursor = match p.get(b"Last") {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    };
    let mut titles = Vec::new();
    for _ in 0..64 {
        let Some(id) = cursor else { break };
        let Some(Object::Dict(d)) = doc.get(id).map(|io| &io.value) else {
            panic!("sibling {id:?} is not a dictionary");
        };
        let title = match d.get(b"Title") {
            Some(Object::String(bytes)) => {
                pdfcer_core::edit::decode_text_string(bytes).text.clone()
            }
            _ => panic!("sibling {id:?} has no /Title"),
        };
        titles.push(title);
        cursor = match d.get(b"Prev") {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        };
    }
    titles
}

/// Would catch: `/Prev` not being written on the newcomer — a chain that a
/// forward-walking viewer renders perfectly and a backward-walking one loses.
///
/// This is the gap the sabotage battery found: with the `/Prev` write
/// deleted, every other test in this file still passed, because the reader
/// only ever walks forward. See [`back_walk`].
#[test]
fn the_sibling_chain_is_walkable_backward_from_last_to_first() {
    let mut s = session("basic-tree.pdf");
    s.add_outline_item(None, "Appendix A", None)
        .expect("adding must succeed");
    s.add_outline_item(None, "Appendix B", None)
        .expect("adding must succeed");

    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("pdfcer's own output must reparse");
    let Some(Object::Dict(catalog)) = doc
        .catalog_id()
        .and_then(|id| doc.get(id).map(|io| &io.value))
    else {
        panic!("no catalog");
    };
    let Some(Object::Reference(root)) = catalog.get(b"Outlines") else {
        panic!("no /Outlines");
    };

    assert_eq!(
        back_walk(&doc, *root),
        vec![
            "Appendix B".to_owned(),
            "Appendix A".to_owned(),
            "Chapter 2".to_owned(),
            "Chapter 1".to_owned(),
        ],
        "the back-link chain must reach every sibling, newest first"
    );

    // And the second addition must not have orphaned the first one's
    // back-link — the bug where each add rewires /Prev to the same node.
    let forward: Vec<String> = read_outline(&doc)
        .items
        .iter()
        .map(|i| i.title.clone())
        .collect();
    let mut reversed = back_walk(&doc, *root);
    reversed.reverse();
    assert_eq!(
        forward, reversed,
        "walking forward and walking backward must visit the same items"
    );
}

/// The **raw** `/Dest` value of the last top-level bookmark in the saved
/// bytes, resolved one reference level but not interpreted.
///
/// ## Why the reader cannot answer the question this exists for
///
/// `read_outline` **resolves** a defined name through §12.3.2.3 and reports
/// it as `Destination::Page` — deliberately, because a shell wants to know
/// which page a bookmark reaches. `Destination::Named` in reader output means
/// the key resolved to *nothing*; it is exactly what
/// `OutlineDiagnostics::unresolved_names` counts.
///
/// So a writer that **resolved the name at author time** and baked in
/// `[page /Fit]`, and a writer that correctly wrote the key, produce
/// **identical reader output**. The distinction is invisible to the oracle —
/// the same blindness as the `/Prev` case earlier in this file, found the same
/// way (a sabotage that stayed green), and the reason two tests below read
/// dictionaries directly instead.
///
/// The distinction is not academic. Baking the destination defeats precisely
/// what §12.3.2.3 exists for: the indirection is what lets a document's links
/// survive a page reorder. A baked bookmark is correct today and silently
/// wrong after the next one, which is the worst failure timing available.
fn raw_last_bookmark_dest(session: &EditSession) -> Object {
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
    let Some(Object::Dict(root)) = catalog.get(b"Outlines").map(|o| doc.resolve(o)) else {
        panic!("no /Outlines")
    };
    let Some(Object::Reference(last)) = root.get(b"Last") else {
        panic!("outline root has no /Last")
    };
    let Some(Object::Dict(item)) = doc.get(*last).map(|io| &io.value) else {
        panic!("last bookmark is not a dict")
    };
    item.get(b"Dest")
        .map(|d| doc.resolve(d).clone())
        .unwrap_or(Object::Null)
}

// ---------------------------------------------------------------------------
// Named destinations (§12.3.2.3) — `Pass 103.3`
// ---------------------------------------------------------------------------

/// Would catch: a bookmark being written with a key nothing defines.
///
/// The result would appear in the panel, look clickable and do nothing —
/// which an operator reads as a viewer bug rather than as a document
/// assembled in the wrong order. Refusing makes the ordering constraint
/// explicit at the moment it is violated.
#[test]
fn a_bookmark_naming_an_undefined_destination_is_refused() {
    let mut s = session("basic-tree.pdf");
    match s.add_outline_item(
        None,
        "Appendix",
        Some(Destination::Named {
            name: b"nowhere".to_vec(),
        }),
    ) {
        Err(EditError::NamedDestinationNotFound { name }) => assert_eq!(name, "nowhere"),
        other => panic!("an undefined key must be refused, got {other:?}"),
    }
    assert_eq!(
        saved_outline(&s).items.len(),
        2,
        "a refusal must leave no bookmark behind"
    );
}

/// Would catch: **the whole point of named destinations being defeated** —
/// the key being resolved to a page at authoring time and baked in as an
/// explicit `[page /Fit]` array.
///
/// That version passes every "does the bookmark reach page 2" test and is
/// wrong in the way that matters: §12.3.2.3's indirection exists so that
/// moving pages does not break every referrer. A baked destination is correct
/// today and silently wrong after the next reorder.
///
/// So this asserts the **shape** as well as the target: the saved `/Dest`
/// must still be `Destination::Named`, not `Destination::Page`, and it must
/// resolve to the right page through the tree.
#[test]
fn a_named_destination_stays_a_name_rather_than_being_resolved_and_baked() {
    let mut s = session("basic-tree.pdf");
    s.add_named_destination(
        b"appendix-a",
        Destination::Page {
            page_index: 2,
            view: DestView::Fit,
        },
    )
    .expect("defining the name must succeed");
    s.add_outline_item(
        None,
        "Appendix A",
        Some(Destination::Named {
            name: b"appendix-a".to_vec(),
        }),
    )
    .expect("a defined name must be accepted");

    // The SHAPE, read raw — see `raw_last_bookmark_dest` for why the reader
    // cannot answer this question at all.
    match raw_last_bookmark_dest(&s) {
        Object::String(written) => assert_eq!(
            written, b"appendix-a",
            "the key must be written byte-for-byte, and as a STRING — that type \
IS the PDF 1.2 tree namespace; a name object would select the PDF 1.1 \
dictionary instead"
        ),
        Object::Array(_) => panic!(
            "the destination was RESOLVED at author time and baked in as an explicit \
array; it must stay a name so a later page reorder does not break it"
        ),
        other => panic!("unexpected raw /Dest: {other:?}"),
    }

    // And it must actually reach the page — a key that survives but resolves
    // to nothing is the other half of the failure, and only the reader can
    // confirm that half.
    let outline = saved_outline(&s);
    let added = outline.items.last().expect("the bookmark must be there");
    assert_eq!(
        added.page_index(),
        Some(2),
        "the name must resolve through the tree to the page it was defined for"
    );
    assert_eq!(
        outline.diagnostics.unresolved_names, 0,
        "no bookmark may be left naming something the document does not define"
    );
}

/// Would catch: keys being written unsorted.
///
/// §7.9.6 requires `Names` *"sorted in lexical order"*, byte-by-byte, shorter
/// keys before longer ones with the same prefix. A reader doing binary
/// descent over an unsorted array silently **misses keys that are present** —
/// and pdfcer's own reader does a linear scan (its `NT-A1` tolerance policy),
/// so pdfcer would not notice its own bad output. Another viewer would.
///
/// So this asserts the raw array order in the **saved bytes**, not through
/// any reader. The insertion order is deliberately reverse-sorted, and
/// includes a prefix pair (`ch` before `chapter`) to pin the tie-break rule
/// rather than just alphabetical order.
#[test]
fn name_tree_keys_are_written_in_the_order_7_9_6_requires() {
    let mut s = session("basic-tree.pdf");
    for key in [&b"zulu"[..], b"chapter", b"ch", b"alpha"] {
        s.add_named_destination(
            key,
            Destination::Page {
                page_index: 0,
                view: DestView::Fit,
            },
        )
        .expect("each name must be definable");
    }

    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let doc = Document::from_bytes(bytes).expect("pdfcer's own output must reparse");
    let Some(Object::Dict(catalog)) = doc
        .catalog_id()
        .and_then(|id| doc.get(id).map(|io| &io.value))
    else {
        panic!("no catalog")
    };
    let Some(Object::Dict(names)) = catalog.get(b"Names").map(|o| doc.resolve(o)) else {
        panic!("no /Names")
    };
    let Some(Object::Dict(dests)) = names.get(b"Dests").map(|o| doc.resolve(o)) else {
        panic!("no /Dests")
    };
    assert!(
        !dests.contains_key(b"Limits"),
        "Table 36 scopes /Limits to intermediate and leaf nodes; a root with \
one is the malformed shape NT-A1 records"
    );
    let Some(Object::Array(arr)) = dests.get(b"Names").map(|o| doc.resolve(o)) else {
        panic!("no /Names array")
    };
    let keys: Vec<Vec<u8>> = arr
        .chunks_exact(2)
        .filter_map(|p| match &p[0] {
            Object::String(k) => Some(k.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            b"alpha".to_vec(),
            b"ch".to_vec(),
            b"chapter".to_vec(),
            b"zulu".to_vec(),
        ],
        "keys must be byte-sorted, with the shorter prefix first"
    );
}

/// Would catch: a second definition silently replacing the first, or a
/// collision being allowed with a key that exists but currently resolves
/// nowhere.
///
/// The second half is the subtle one. `resolve_destination` folds
/// "undefined", "dangling" and "remote" together into `None` — correct for
/// its own callers — and a writer that used it as the collision check would
/// happily overwrite a key whose target page had been deleted. The check has
/// to be membership, not reachability.
#[test]
fn defining_the_same_name_twice_is_refused() {
    let mut s = session("basic-tree.pdf");
    let dest = || Destination::Page {
        page_index: 0,
        view: DestView::Fit,
    };
    s.add_named_destination(b"intro", dest())
        .expect("first definition must succeed");
    match s.add_named_destination(b"intro", dest()) {
        Err(EditError::NamedDestinationTaken { name }) => assert_eq!(name, "intro"),
        other => panic!("a duplicate key must be refused, got {other:?}"),
    }
}

/// Would catch: keys being round-tripped through UTF-8.
///
/// §7.9.6 imposes **no** encoding on name-tree keys — *"any encoding of the
/// keys may be used as long as it is self-consistent"* — and real files use
/// UTF-16BE-with-BOM, PDFDocEncoding and opaque bytes. A `&str` API, or a
/// lossy decode anywhere in the path, would mangle a key read out of another
/// document and make it un-matchable against the file it came from.
///
/// The key here is deliberately **not valid UTF-8**.
#[test]
fn a_non_utf8_key_survives_byte_for_byte() {
    let mut s = session("basic-tree.pdf");
    // Built at runtime rather than as a literal: `invalid_from_utf8` fires at
    // COMPILE time on a `from_utf8` over a const slice, and the lint is right
    // in general — here the invalidity is the entire premise, so the check has
    // to stay and the value has to be opaque to the linter.
    let key: Vec<u8> = vec![0xFE, 0xFF, 0x00, 0x63, 0x00, 0xE9, 0xFF, 0xFE];
    let key: &[u8] = &key;
    assert!(
        std::str::from_utf8(key).is_err(),
        "the test premise: this key is not valid UTF-8"
    );
    s.add_named_destination(
        key,
        Destination::Page {
            page_index: 1,
            view: DestView::Fit,
        },
    )
    .expect("an opaque key must be definable");
    s.add_outline_item(
        None,
        "Chapitre",
        Some(Destination::Named { name: key.to_vec() }),
    )
    .expect("and referable");

    match raw_last_bookmark_dest(&s) {
        Object::String(written) => assert_eq!(
            written, key,
            "the key must come back byte-identical, not UTF-8-repaired"
        ),
        other => panic!("unexpected raw /Dest: {other:?}"),
    }
    // And it still resolves, which is the proof the written bytes match the
    // tree's key rather than merely surviving somewhere.
    let outline = saved_outline(&s);
    assert_eq!(outline.diagnostics.unresolved_names, 0);
    assert_eq!(
        outline.items.last().expect("bookmark").page_index(),
        Some(1)
    );
}

/// Would catch: undo leaving the name defined, so a redefinition is then
/// refused for a name the operator cannot see and did not keep.
#[test]
fn undo_removes_a_named_destination() {
    let mut s = session("basic-tree.pdf");
    let dest = || Destination::Page {
        page_index: 0,
        view: DestView::Fit,
    };
    s.add_named_destination(b"intro", dest()).expect("defines");
    assert!(s.undo().is_some(), "one undo entry must exist");
    s.add_named_destination(b"intro", dest())
        .expect("after undo the name must be free again");
}

/// Would catch: the collision check asking *"does this name reach a page?"*
/// instead of *"is this name defined?"*
///
/// ## The two questions differ exactly where it is dangerous
///
/// `DestinationResolver::resolve_destination` answers reachability and folds
/// **undefined**, **dangling** and **remote** all into `None` — correct for
/// its own callers, since a destination that already pointed nowhere is not
/// newly broken by a page delete. A writer that reused it as its collision
/// check would see `None` for a **defined but dangling** key and silently
/// overwrite it. That is why `DestinationResolver::lookup` exists as a
/// separate membership query.
///
/// Nothing in the rest of this file can distinguish the two, because every
/// destination it defines resolves. This fixture defines `ghost` as
/// `[null /Fit]` — present, and reaching nothing.
///
/// `[null /Fit]` rather than `[99 0 R /Fit]` with object 99 absent, which was
/// the first attempt: `resolve_destination` returns element 0 **as a
/// reference** without checking that the object exists, so a reference to a
/// missing object still answers `Some`. Only a non-reference element 0 makes
/// it answer `None`. That shape is not contrived — it is exactly what
/// `pageops::assemble`'s page barrier produces when a destination's target
/// page was not copied.
///
/// Silently overwriting would be the worst of the available failures: every
/// existing link and bookmark naming `ghost` would be re-aimed at a page
/// nobody chose, and nothing would report it.
#[test]
fn a_defined_but_dangling_name_still_collides() {
    let doc = build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /Names << /Dests << /Names \
             [(ghost) [null /Fit]] >> >> >>",
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
    ]);
    let mut s = EditSession::new(Document::from_bytes(doc).expect("fixture must parse"));

    // Premise: the key is present and reaches nothing.
    let resolver = pdfcer_core::pageops::references::DestinationResolver::new(&s.graph());
    assert!(
        resolver.lookup(b"ghost").is_some(),
        "the key must be DEFINED — otherwise this tests nothing"
    );
    assert!(
        resolver
            .resolve_destination(&s.graph(), &Object::String(b"ghost".to_vec()))
            .is_none(),
        "and it must reach NO page — that gap is the whole point"
    );

    match s.add_named_destination(
        b"ghost",
        Destination::Page {
            page_index: 0,
            view: DestView::Fit,
        },
    ) {
        Err(EditError::NamedDestinationTaken { name }) => assert_eq!(name, "ghost"),
        other => panic!("a defined-but-dangling key must still collide, got {other:?}"),
    }
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
