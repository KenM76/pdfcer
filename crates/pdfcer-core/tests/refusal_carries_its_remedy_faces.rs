//! A coverage refusal carries its remedy as DATA, not only as prose
//! (`Pass 296.1`).
//!
//! # What is being defended
//!
//! `Pass 274.0` made this refusal end on a working remedy instead of on
//! "choose a font that covers it", and `Pass 279.0` made that remedy
//! page-aware because the naive list was wrong in its most prominent position
//! on this very fixture. Both improvements lived inside `Refusal::message`,
//! where the only way to reach them was to split the sentence on
//! `"these standard-14 faces have it: "` — a locator for this crate's message
//! format, living in somebody else's GUI.
//!
//! So the consuming shell showed nothing at all, on the one refusal an
//! operator can act on. **A remedy computed correctly and then made
//! unreachable is the same as not computing it.**
//!
//! # ★★ The assertion that matters is the AGREEMENT
//!
//! Not "the field is non-empty" — that would pass on a field populated from a
//! second, independent computation that had drifted from the sentence. The
//! test asserts the field and the message's clause are two renderings of ONE
//! value, by rebuilding the clause from the field and finding it inside the
//! message. Populate the field from anywhere else and this goes red.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{EditError, EditOptions, EditRequest, faces_clause};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// `subset_missing.pdf` — font `ABCDEF+Helvetica`, an embedded SUBSET whose
/// carried codes are exactly the letters of "the cat". Editing `cat` to a word
/// using any other letter hits the embedded-subset floor.
const SUBSET: &str = "textedit/subset_missing.pdf";

/// The refusal from the edit this fixture exists to refuse.
fn subset_floor_refusal() -> pdfcer_core::text_edit::Refusal {
    let mut s = EditSession::new(Document::load(&fixture(SUBSET)).expect("fixture parses"));
    let err = s
        .edit_text(
            &EditRequest::find_replace(0, "cat", "dog"),
            &EditOptions::default(),
        )
        .expect_err("the subset cannot show 'd', 'o' or 'g'");
    match err {
        EditError::Refused(r) => r,
        other => panic!("expected a refusal, got {other}"),
    }
}

#[test]
fn the_refusal_carries_the_faces_its_message_names() {
    let refusal = subset_floor_refusal();

    assert!(
        !refusal.remedy_faces.is_empty(),
        "this fixture has a working remedy and the refusal must carry it; message was: {}",
        refusal.message
    );

    // ★ The agreement, rebuilt rather than parsed: take the FIELD, run it
    // through the one clause producer, and require the result to be present in
    // the message verbatim. Two computations that merely agree today would
    // pass a weaker test and drift tomorrow.
    let borrowed: Vec<&str> = refusal.remedy_faces.iter().map(String::as_str).collect();
    let clause = faces_clause(&borrowed);
    assert!(
        refusal.message.contains(&clause),
        "the field and the sentence must be one computation.\n  field -> {clause}\n  message: {}",
        refusal.message
    );
}

#[test]
fn the_remedy_is_page_aware_not_the_naive_coverage_list() {
    // ★★ THE WHOLE POINT, and the reason `std14_faces_covering` now carries a
    // warning. On THIS page the font is `ABCDEF+Helvetica`, so `set_font
    // Helvetica` resolves back into the very subset that refused and the
    // follow-up edit fails word for word (measured in `Pass 279.0`). The naive
    // list puts `Helvetica` FIRST; the page-aware one must not offer it at all.
    let refusal = subset_floor_refusal();

    assert!(
        !refusal.remedy_faces.iter().any(|f| f == "Helvetica"),
        "Helvetica resolves into this page's own refusing subset and must not \
         be offered: {:?}",
        refusal.remedy_faces
    );

    // And the naive computation is shown to disagree, so this test fails if
    // the refusal ever quietly reverts to it.
    let naive = pdfcer_core::text_edit::std14_faces_covering('o');
    assert!(
        naive.contains(&"Helvetica"),
        "the naive list is supposed to be the WRONG one here; if it no longer \
         offers Helvetica this test has stopped measuring anything: {naive:?}"
    );
}
