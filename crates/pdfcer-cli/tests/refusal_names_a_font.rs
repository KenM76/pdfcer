//! # The refusal names a font, and the font it names actually works
//!
//! ## Why this cannot be a core test
//!
//! The value of naming a remedy is that **acting on it succeeds**. A core test
//! can assert the message contains the word "Helvetica"; only an end-to-end
//! run can show that doing what the message says gets the operator unstuck.
//!
//! ★ That is the whole defect being closed. The capability already existed —
//! switch the run to a standard-14 face, then edit — and the refusal did not
//! say so, which made it undiscoverable. A test that checked only the wording
//! would leave the sentence true and the route unverified, which is the exact
//! state this Pass found.
//!
//! ## The sequence
//!
//! 1. Edit a run to text its subset cannot show → **refused**, and the refusal
//!    names faces.
//! 2. Take the **first** face it named, verbatim, and `format-text --set-font`
//!    with it.
//! 3. Repeat the edit → **succeeds**.
//!
//! Step 2 uses the string the refusal printed rather than a hard-coded
//! `Helvetica`, so a message that named a face `set_font` does not accept
//! fails here rather than reading plausibly forever.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

/// A `/Square`-free text fixture whose font is a SUBSET missing lowercase.
///
/// `symbolic-truetype-private-cmap.pdf` embeds a three-glyph subset, so any
/// character outside it is refused for exactly the reason under test.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/symbolic-truetype-private-cmap.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_rnf_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// ★★★ The refusal names faces, and the first one it names unsticks the edit.
#[test]
fn the_named_font_actually_works() {
    let src = fixture();

    // 1. The edit that cannot work.
    let out = run(&[
        "edit-text",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "A",
        "--replace",
        "z",
        "-o",
        temp_path("refused").to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "the subset has no 'z': {}",
        stderr(&out)
    );
    let msg = stderr(&out);
    // ★ EITHER member of the family. Two refusals produce this one operator
    // experience -- `InverseEncoding`'s "no glyph for" when the font has none
    // at all, and the "embedded-subset floor" when the subset does not carry
    // the code on this page. Asserting one wording pinned one member and let
    // the other ship without the remedy; that is how the gap was found. What
    // matters to the operator is identical, so the assertion is on the shared
    // half: a font was named.
    assert!(
        msg.contains("faces have it: "),
        "the refusal must name a remedy whichever member fired: {msg}"
    );

    // 2. The face the refusal itself named, taken verbatim from the message.
    let named = msg
        .rsplit("faces have it: ")
        .next()
        .and_then(|tail| tail.split(',').next())
        .map(|s| s.trim().trim_end_matches('.').to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| panic!("the refusal named no font at all: {msg}"));
    assert!(
        named.starts_with(char::is_uppercase),
        "a /BaseFont name, not prose: {named:?} (from {msg})"
    );

    // 3. Switch the run to it, then repeat the edit.
    let switched = temp_path("switched");
    let out = run(&[
        "format-text",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "A",
        "--set-font",
        &named,
        "-o",
        switched.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "set-font must accept the name the refusal printed ({named:?}): {}",
        stderr(&out)
    );

    let edited = temp_path("edited");
    let out = run(&[
        "edit-text",
        switched.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "A",
        "--replace",
        "z",
        "-o",
        edited.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "★ THE POINT OF THE WHOLE PASS: doing what the refusal said must get \
         the operator unstuck. It did not: {}",
        stderr(&out)
    );
}

/// A fixture whose font's subset stem IS a standard-14 name — `ABCDEF+Helvetica`.
///
/// ★ This is the fixture the test above could not have used, and its absence is
/// why `Pass 279.0` existed. `symbolic-truetype-private-cmap.pdf`'s font is
/// `AAAAAA+pdfcerSymbolicPrivate`, whose stem matches no standard-14 name, so
/// the collision this file tests is **structurally impossible** there. The
/// three-step loop above was correct, ran green, and could not fail.
fn shadowing_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit/subset_missing.pdf")
}

/// ★★★ THE NAMED FACE MUST NOT BE ONE THE PAGE ALREADY SHADOWS.
///
/// `set_font` resolves a selector against the page's own resources **first**,
/// matching a subset-stemmed `/BaseFont` — so on this page asking for
/// `Helvetica` re-uses `ABCDEF+Helvetica`, the very subset that refused the
/// character, and the whole remedy is a no-op that reports success.
///
/// Measured before the fix: the refusal named `Helvetica` **first**,
/// `--set-font Helvetica` printed `ABCDEF+Helvetica->ABCDEF+Helvetica` and
/// exited 0, and the repeated edit refused **word for word**.
///
/// The same three-step loop as `the_named_font_actually_works`, on the fixture
/// that can exhibit the defect. That duplication is the point: one loop, two
/// pages, and only one of them could ever have gone red.
#[test]
fn the_named_face_is_not_shadowed_by_the_pages_own_subset() {
    let src = shadowing_fixture();

    let out = run(&[
        "edit-text",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "cat",
        "--replace",
        "dog",
        "-o",
        temp_path("shadow_refused").to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "this subset does not carry 'd': {}",
        stderr(&out)
    );
    let msg = stderr(&out);
    assert!(msg.contains("faces have it: "), "{msg}");

    let named = msg
        .rsplit("faces have it: ")
        .next()
        .and_then(|tail| tail.split(',').next())
        .map(|s| s.trim().trim_end_matches('.').to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| panic!("the refusal named no font at all: {msg}"));

    // The direct statement of the defect, beside the loop that proves it.
    // Asserting only the loop would leave a future reader unable to see WHICH
    // face was wrong, and asserting only this would not prove the survivor
    // works.
    assert_ne!(
        named, "Helvetica",
        "the page's own font is ABCDEF+Helvetica, so `set_font Helvetica` \
         re-uses that subset and changes nothing: {msg}"
    );

    let switched = temp_path("shadow_switched");
    let out = run(&[
        "format-text",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "cat",
        "--set-font",
        &named,
        "-o",
        switched.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "set-font must accept the name the refusal printed ({named:?}): {}",
        stderr(&out)
    );

    let edited = temp_path("shadow_edited");
    let out = run(&[
        "edit-text",
        switched.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "cat",
        "--replace",
        "dog",
        "-o",
        edited.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "★ doing what the refusal said must get the operator unstuck, and on \
         this page it led in a circle until Pass 279.0: {}",
        stderr(&out)
    );
}

/// The filter removes a face for the RIGHT reason, and keeps the rest.
///
/// A fix that emptied the list, or that dropped every name the page's font
/// family resembles, would satisfy the loop above by accident — the loop only
/// ever reads the FIRST name. Here the whole list is inspected: `Helvetica` is
/// gone because the page shadows it, and the three other Helvetica faces
/// survive because their names do not match the subset's stem and `set_font`
/// will author them fresh.
#[test]
fn only_the_shadowed_name_is_dropped() {
    let out = run(&[
        "edit-text",
        shadowing_fixture().to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "cat",
        "--replace",
        "dog",
        "-o",
        temp_path("shadow_list").to_str().unwrap(),
    ]);
    let msg = stderr(&out);
    let list = msg
        .rsplit("faces have it: ")
        .next()
        .expect("a face list")
        .trim_end()
        .trim_end_matches('.');
    let faces: Vec<&str> = list.split(", ").map(str::trim).collect();
    assert!(
        !faces.contains(&"Helvetica"),
        "shadowed by ABCDEF+Helvetica: {list}"
    );
    for kept in [
        "Helvetica-Bold",
        "Helvetica-Oblique",
        "Times-Roman",
        "Courier",
    ] {
        assert!(
            faces.contains(&kept),
            "{kept} does not collide with the page's subset stem and must survive: {list}"
        );
    }
}

/// The refusal does not name a font when none would help.
///
/// A list that always appeared would be decoration, and worse than nothing —
/// it would send an operator to a face that fails the same way. CJK is outside
/// every standard-14 face.
#[test]
fn no_face_is_offered_when_none_would_help() {
    let out = run(&[
        "edit-text",
        fixture().to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "A",
        "--replace",
        "中",
        "-o",
        temp_path("cjk").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let msg = stderr(&out);
    assert!(
        !msg.contains("faces have it"),
        "no standard-14 face shows CJK, so none may be offered: {msg}"
    );
}

/// ★★★ The OTHER member of the family gets the same remedy.
///
/// Two refusals produce one operator experience:
///
/// | refusal | fires when |
/// |---|---|
/// | `InverseEncoding` / `CompositeEncoding` `TargetAbsent` | the font has no glyph for the character at all |
/// | R-INV-1 embedded-subset floor | the font has one, but this SUBSET does not carry the code on this page |
///
/// The first version of this Pass enriched only one of them. The gap was
/// found by the end-to-end test above happening to pick a fixture that
/// reached the *other* branch — and an ablation confirms it: removing the
/// font list from one site leaves the other site's test green.
///
/// `R245`'s shape, caught inside the same Pass that created it, and this test
/// is what keeps the pair together.
#[test]
fn the_other_refusal_in_the_family_names_a_font_too() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/cidfonttype2-noninjective-tounicode.pdf");
    let out = run(&[
        "edit-text",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "A",
        "--replace",
        "z",
        "-o",
        temp_path("family").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "{}", stderr(&out));
    let msg = stderr(&out);
    assert!(
        msg.contains("no glyph for"),
        "this fixture must reach the OTHER refusal, or the pair is untested \
         and this file has drifted back to covering one member: {msg}"
    );
    assert!(
        msg.contains("faces have it: ") && msg.contains("Helvetica"),
        "and it must carry the same remedy as its sibling: {msg}"
    );
}
