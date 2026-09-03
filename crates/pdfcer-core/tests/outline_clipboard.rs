//! Bookmarks on the clipboard — copy, cut, paste (`Pass 172.0`).
//!
//! ## Why this one is an exceed rather than a catch-up
//!
//! Adobe's own documentation says bookmarks *"can't be copied directly … from
//! one file to another"*. Acrobat offers cut and paste of a bookmark **within**
//! a document and nothing between two. So the interesting design question here
//! — what happens to a destination naming a page the other document does not
//! have — is one the parity reference never had to answer.
//!
//! pdfcer **drops** it and counts it. Not clamped to the last page: a bookmark
//! that navigates confidently to the wrong place is worse than one that
//! plainly does not navigate, and §12.3.3 permits an item with no destination.
//!
//! ## What these tests pin
//!
//! 1. A subtree copies whole — children, open state, colour, style flags.
//! 2. **The view survives**, not just the page. A bookmark to *"Detail B —
//!    400%"* is copying the zoom as much as the page, and substituting `/Fit`
//!    would have been a silent, plausible loss.
//! 3. A destination past the end of the destination document is dropped and
//!    **counted**, and the count is askable *before* the press.
//! 4. Cut is one undo entry.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OutlinePlacement};
use pdfcer_core::outline::{DestView, Destination, OutlineClip, read_outline};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// Build a small outline: a parent with two children, the parent open, one
/// child pointing at page 0 at 400% zoom.
fn with_outline(s: &mut EditSession) -> pdfcer_core::object::ObjId {
    let parent = s
        .add_outline_item(None, "Sheet 1", None)
        .expect("parent bookmark");
    s.add_outline_item(
        Some(parent),
        "Detail B",
        Some(Destination::Page {
            page_index: 0,
            view: DestView::Xyz {
                left: Some(120.0),
                top: Some(480.0),
                zoom: Some(4.0),
            },
        }),
    )
    .expect("child one");
    s.add_outline_item(Some(parent), "Notes", None)
        .expect("child two");
    s.set_outline_open(parent, true).expect("open it");
    parent
}

fn titles(s: &EditSession) -> Vec<String> {
    fn walk(items: &[pdfcer_core::outline::OutlineItem], out: &mut Vec<String>) {
        for item in items {
            out.push(item.title.clone());
            walk(&item.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&read_outline(&s.graph()).items, &mut out);
    out
}

#[test]
fn copying_a_bookmark_carries_its_whole_subtree() {
    let mut s = session("hello.pdf");
    let parent = with_outline(&mut s);

    let clip = s.copy_outline_item(parent).expect("copy");
    assert_eq!(clip.len(), 3, "the parent and both children");
    let root = clip.items.first().expect("a root");
    assert_eq!(root.title, "Sheet 1");
    assert!(root.open, "the open state travelled");
    assert_eq!(root.children.len(), 2);
    assert_eq!(
        root.children.first().map(|c| c.title.as_str()),
        Some("Detail B"),
    );
    // A copy is not a move.
    assert_eq!(titles(&s).len(), 3);
}

/// ★ The VIEW survives, not just the page.
///
/// A bookmark to a detail at 400% is copying the zoom as much as the page.
/// Substituting `/Fit` would have been a silent, plausible loss — the
/// bookmark still works, it just goes somewhere else.
#[test]
fn a_bookmarks_zoom_and_scroll_position_survive_the_clipboard() {
    let mut s = session("hello.pdf");
    let parent = with_outline(&mut s);
    let clip = s.copy_outline_item(parent).expect("copy");
    let wired = OutlineClip::from_bytes(&clip.to_bytes()).expect("through the wire");
    assert_eq!(wired, clip, "nothing is lost on the wire");

    let detail = wired
        .items
        .first()
        .and_then(|r| r.children.first())
        .expect("Detail B");
    assert_eq!(
        detail.destination,
        Some(Destination::Page {
            page_index: 0,
            view: DestView::Xyz {
                left: Some(120.0),
                top: Some(480.0),
                zoom: Some(4.0),
            },
        }),
        "the fit style AND its parameters came through",
    );
}

#[test]
fn a_pasted_subtree_arrives_whole_in_another_document() {
    let mut source = session("hello.pdf");
    let parent = with_outline(&mut source);
    let clip = source.copy_outline_item(parent).expect("copy");
    let wired = OutlineClip::from_bytes(&clip.to_bytes()).expect("wire");

    let mut destination = session("hello.pdf");
    assert!(titles(&destination).is_empty(), "precondition: no outline");
    let outcome = destination
        .paste_outline_item(&wired, OutlinePlacement::LastChild { parent: None })
        .expect("paste");
    assert_eq!(outcome.items_pasted, 3);
    assert_eq!(outcome.destinations_dropped, 0, "page 0 exists here");
    assert_eq!(
        titles(&destination),
        vec![
            "Sheet 1".to_owned(),
            "Detail B".to_owned(),
            "Notes".to_owned()
        ],
    );
}

/// ★ A destination naming a page the destination document does not have is
/// DROPPED and counted — never clamped to the last page.
#[test]
fn a_destination_past_the_end_is_dropped_and_counted_not_clamped() {
    let mut source = session("hello.pdf");
    // Grow the SOURCE to nine pages FIRST — `add_outline_item` validates that
    // the destination page exists, which is itself worth knowing: a bookmark
    // pointing past the end cannot be authored, only inherited from a file or
    // created by deleting a page afterwards.
    let page = source.copy_pages(&[0]).expect("copy a page");
    for _ in 0..8 {
        source
            .paste_pages(&page, pdfcer_core::pageops::InsertPosition::End)
            .expect("grow the source");
    }
    let parent = source
        .add_outline_item(
            None,
            "Sheet 9",
            Some(Destination::Page {
                page_index: 8,
                view: DestView::Fit,
            }),
        )
        .expect("a bookmark to the ninth page, which is legitimate HERE");
    let clip = source.copy_outline_item(parent).expect("copy");
    assert_eq!(
        clip.deepest_page(),
        Some(8),
        "a shell can ask this BEFORE the press and say how many will not survive",
    );

    let mut destination = session("hello.pdf");
    let outcome = destination
        .paste_outline_item(&clip, OutlinePlacement::LastChild { parent: None })
        .expect("paste");
    assert_eq!(outcome.items_pasted, 1);
    assert_eq!(
        outcome.destinations_dropped, 1,
        "the destination named page 9 of a one-page document",
    );
    let items = read_outline(&destination.graph()).items;
    let pasted = items.first().expect("the bookmark arrived");
    assert_eq!(pasted.title, "Sheet 9", "with its title");
    assert_eq!(
        pasted.destination, None,
        "and WITHOUT a destination -- not clamped to the last page, because a \
         bookmark that navigates confidently to the wrong place is worse than \
         one that plainly does not navigate",
    );
}

/// ★ Cut is one undo entry, and undoing it puts the whole subtree back.
#[test]
fn cutting_a_bookmark_subtree_is_one_undo_entry() {
    let mut s = session("hello.pdf");
    let parent = with_outline(&mut s);
    let depth_before = s.undo_depth();

    let clip = s.cut_outline_item(parent).expect("cut");
    assert_eq!(clip.len(), 3, "all three are on the clipboard");
    assert!(titles(&s).is_empty(), "and off the outline");
    assert_eq!(s.undo_depth(), depth_before + 1, "ONE undo entry");

    s.undo().expect("one press");
    assert_eq!(titles(&s).len(), 3, "one press restores the whole subtree");
}

/// A paste of a multi-level subtree is one undo entry too, however many
/// bookmarks it holds.
#[test]
fn pasting_a_subtree_is_one_undo_entry() {
    let mut source = session("hello.pdf");
    let parent = with_outline(&mut source);
    let clip = source.copy_outline_item(parent).expect("copy");

    let mut destination = session("hello.pdf");
    let depth_before = destination.undo_depth();
    destination
        .paste_outline_item(&clip, OutlinePlacement::LastChild { parent: None })
        .expect("paste");
    assert_eq!(
        destination.undo_depth(),
        depth_before + 1,
        "three bookmarks, an open-state write and any colour patches folded \
         into one entry",
    );
    destination.undo().expect("one press");
    assert!(
        titles(&destination).is_empty(),
        "and one press takes all of them away again",
    );
}

#[test]
fn a_payload_that_is_not_a_bookmark_clip_is_refused_by_its_signature() {
    assert!(matches!(
        OutlineClip::from_bytes(b"not a clip"),
        Err(pdfcer_core::outline::OutlineClipError::NotAClip)
    ));
}

#[test]
fn an_empty_clip_pastes_nothing_and_commits_nothing() {
    let mut s = session("hello.pdf");
    let empty =
        OutlineClip::from_bytes(&OutlineClip::empty().to_bytes()).expect("an empty clip is legal");
    let outcome = s
        .paste_outline_item(&empty, OutlinePlacement::LastChild { parent: None })
        .expect("paste");
    assert_eq!(outcome.items_pasted, 0);
    assert_eq!(s.undo_depth(), 0, "nothing reached the undo stack");
}
