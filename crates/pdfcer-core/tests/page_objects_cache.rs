//! # The page-decomposition cache must never serve a stale model
//! (`Pass 181.0`)
//!
//! The speed is measured in `edit_latency.rs`. **This file is about
//! correctness**, and it matters more.
//!
//! `PageObjects` addresses page content by **index**. A cache that returned a
//! model built from different bytes than the page now holds would not be
//! slow — it would make `move_objects(page, &[7], …)` edit whatever object 7
//! is in the *stale* model, which is a different object, or none. That is
//! silent corruption of the operator's drawing, produced by a verb that
//! reports success.
//!
//! So every assertion here is of the form *"after X, the cache does not lie"*,
//! and the cases are chosen to be the ones where a plausible cheaper key would
//! get it wrong:
//!
//! - after an **edit**, because that is the obvious one;
//! - after **undo**, because a generation counter that only counts forward
//!   would happily serve the post-edit model for a pre-edit page;
//! - after **redo**, the same in reverse;
//! - and on a page whose content is **unmodified**, where the cache must
//!   still be allowed to hit, or it is not a cache.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;

/// A one-page PDF with three separate `re`-filled rectangles, so an edit to
/// one is visible in the decomposition of the others' neighbourhood.
fn three_rects() -> Vec<u8> {
    let content = "1 0 0 RG 10 10 20 20 re f 50 50 20 20 re f 90 90 20 20 re f\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// The bounding box of object `i`, as a coarse identity for that object.
fn bbox(session: &mut EditSession, i: usize) -> (f64, f64) {
    let objs = session.page_objects(0).expect("decomposes");
    let b = objs.objects[i].page_bbox();
    (b.min.x, b.min.y)
}

/// **An unmodified page hits the cache, and the model is the same one.**
///
/// Asserted by pointer identity, not by value: two equal models built twice
/// would satisfy a value comparison and would mean the cache never fired.
#[test]
fn an_unmodified_page_returns_the_very_same_model() {
    let mut s = EditSession::new(Document::from_bytes(three_rects()).unwrap());
    let a = s.page_objects(0).expect("decomposes");
    let b = s.page_objects(0).expect("decomposes");
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "a second call on unchanged content must return the cached model, \
         not an equal one built again"
    );
}

/// ★ **After an edit the cache does NOT serve the pre-edit model.**
///
/// The case the whole design exists to get right.
#[test]
fn an_edit_invalidates_the_cached_model() {
    let mut s = EditSession::new(Document::from_bytes(three_rects()).unwrap());
    let before = s.page_objects(0).expect("decomposes");
    let moved_from = bbox(&mut s, 0);

    s.move_objects(0, &[0], 40.0, 0.0).expect("moves");

    let after = s.page_objects(0).expect("decomposes");
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after),
        "the model must be rebuilt after the content changed"
    );
    let moved_to = bbox(&mut s, 0);
    assert!(
        (moved_to.0 - moved_from.0 - 40.0).abs() < 0.01,
        "and it must reflect the edit: {moved_from:?} -> {moved_to:?}"
    );
}

/// ★★ **UNDO invalidates it too, and this is the case a cheaper key gets
/// wrong.**
///
/// A monotonic generation counter — bumped on every commit — would be at a
/// *higher* generation after the undo than the cached entry, and would
/// therefore look invalid, which is safe. But a counter compared for equality
/// against a remembered value, or one that counts commands rather than
/// content states, can land back on a value it has already used. The span key
/// cannot: restoring an earlier span restores exactly those bytes, and
/// serving the model built from them is correct BY CONSTRUCTION rather than
/// by the counter happening to differ.
#[test]
fn undo_and_redo_both_invalidate_the_cached_model() {
    let mut s = EditSession::new(Document::from_bytes(three_rects()).unwrap());
    let origin = bbox(&mut s, 0);

    s.move_objects(0, &[0], 40.0, 0.0).expect("moves");
    let moved = bbox(&mut s, 0);
    assert!((moved.0 - origin.0 - 40.0).abs() < 0.01, "precondition");

    s.undo().expect("undo");
    let undone = bbox(&mut s, 0);
    assert!(
        (undone.0 - origin.0).abs() < 0.01,
        "★ after undo the cache must not still serve the MOVED model: \
         expected {origin:?}, got {undone:?}"
    );

    s.redo().expect("redo");
    let redone = bbox(&mut s, 0);
    assert!(
        (redone.0 - moved.0).abs() < 0.01,
        "★ and after redo it must not serve the UNDONE one: \
         expected {moved:?}, got {redone:?}"
    );
}

/// **The verb and the accessor agree** — they must, because they share one
/// entry, and a shell passes indices from one into the other.
///
/// If these ever disagreed, every index a shell obtained would address a
/// different object than the verb resolved it to.
#[test]
fn the_verb_edits_the_object_the_accessor_named() {
    let mut s = EditSession::new(Document::from_bytes(three_rects()).unwrap());
    let objs = s.page_objects(0).expect("decomposes");
    assert_eq!(objs.objects.len(), 3, "the fixture has three rectangles");

    // Pick the MIDDLE one by its geometry, then move it by index and check
    // that the object which moved is the one that was picked.
    let target = objs
        .objects
        .iter()
        .position(|o| (o.page_bbox().min.x - 50.0).abs() < 0.01)
        .expect("the middle rectangle is findable");

    s.move_objects(0, &[target], 0.0, 25.0).expect("moves");

    let after = s.page_objects(0).expect("decomposes");
    let moved: Vec<_> = after
        .objects
        .iter()
        .filter(|o| (o.page_bbox().min.y - 75.0).abs() < 0.01)
        .collect();
    assert_eq!(
        moved.len(),
        1,
        "exactly the picked rectangle moved to y=75, not a neighbour"
    );
}
