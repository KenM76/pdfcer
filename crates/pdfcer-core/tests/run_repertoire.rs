//! # The alphabet is knowable before the first keystroke
//!
//! ## The operator problem, in the requesting shell's words
//!
//! > *"the refusal arrives **at commit**, so he types a whole word and then
//! > loses it. The alphabet is knowable before the first keystroke and we do
//! > not use it that way yet."*
//!
//! ## The contract they specified, and what this file measures
//!
//! > *"It must agree with the refusal: a character the query accepts,
//! > `encode_str` must not refuse for that run. That equivalence is the whole
//! > contract."*
//!
//! So the central test does not sample. It takes **every character the
//! repertoire accepts** and edits the run to it, requiring success; then takes
//! a set of characters the repertoire **rejects** and requires the refusal.
//! Both directions, over the whole set, because either half alone is
//! satisfiable by a degenerate answer — an empty repertoire passes the first,
//! and a universal one passes the second.
//!
//! ## The claim I could not promise, and therefore measured
//!
//! Acceptance is **per character**: a word is accepted exactly when each of
//! its characters is. That is not obviously true — `R-INV-5` seeds a tie-break
//! from codes already used in the run, and that seed *grows* as a word is
//! encoded. It changes **which code** a character gets, not **whether** it is
//! accepted, so the property holds; `a_word_of_accepted_characters_is_accepted`
//! is what makes that a measurement rather than a belief.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{EditOptions, EditRequest};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("fixture parses"))
}

/// `subset_missing.pdf` — font `ABCDEF+Helvetica`, an embedded SUBSET whose
/// carried codes are exactly the letters of "the cat".
///
/// ★ The right fixture for this: a full font would accept nearly everything
/// and the two directions of the equivalence would be lopsided. Here the
/// accepted set is small and its complement is enormous, so both halves have
/// something to say.
const SUBSET: &str = "textedit/subset_missing.pdf";

/// Whether `edit_text` accepts replacing `find` with `replace` on page 0.
///
/// A fresh session per call: the question is what the ORIGINAL run accepts,
/// and an accumulated session would answer about a run that previous edits had
/// already changed.
fn edit_accepts(rel: &str, find: &str, replace: &str) -> bool {
    let mut s = session(rel);
    s.edit_text(
        &EditRequest::find_replace(0, find, replace),
        &EditOptions::default(),
    )
    .is_ok()
}

/// ★★★ THE CONTRACT, MEASURED OVER THE WHOLE SET — accepted ⇒ not refused.
#[test]
fn every_accepted_character_is_one_edit_text_accepts() {
    let s = session(SUBSET);
    let rep = s.run_repertoire(0, "cat", None).expect("the run is there");
    assert!(
        rep.is_editable(),
        "this run has a usable encoding: {:?}",
        rep.reason
    );
    assert!(
        !rep.accepted.is_empty(),
        "an empty repertoire would satisfy this test vacuously"
    );

    for ch in &rep.accepted {
        let replace: String = std::iter::repeat_n(*ch, 3).collect();
        assert!(
            edit_accepts(SUBSET, "cat", &replace),
            "the repertoire accepted {ch:?} and edit_text refused it — that is the \
             one thing this verb promises not to do"
        );
    }
}

/// ★★ THE OTHER DIRECTION — rejected ⇒ actually refused.
///
/// Without this, a repertoire that accepted every character in Unicode would
/// pass the test above.
#[test]
fn a_rejected_character_is_one_edit_text_refuses() {
    let s = session(SUBSET);
    let rep = s.run_repertoire(0, "cat", None).expect("the run is there");

    // Characters chosen to span the reasons: an ordinary Latin letter the
    // subset does not carry, a digit, punctuation, and one outside the face
    // entirely.
    let probes = ['d', 'z', '7', '#', 'ß', '漢'];
    let mut rejected_probes = 0;
    for ch in probes {
        if rep.accepts(ch) {
            continue;
        }
        rejected_probes += 1;
        let replace: String = std::iter::repeat_n(ch, 3).collect();
        assert!(
            !edit_accepts(SUBSET, "cat", &replace),
            "the repertoire rejected {ch:?} and edit_text took it — the greyed key \
             would have been a lie in the safe direction, which is still a lie"
        );
    }
    assert!(
        rejected_probes >= 4,
        "this fixture's subset must reject most of the probe set, or the test is \
         measuring nothing: rejected {rejected_probes} of {}",
        probes.len()
    );
}

/// ★ The claim about `R-INV-5` I refused to promise without measuring.
///
/// A word made only of accepted characters must itself be accepted. The
/// tie-break seed grows as a word is encoded, so per-character acceptance
/// implying whole-word acceptance is a property, not a tautology.
#[test]
fn a_word_of_accepted_characters_is_accepted() {
    let s = session(SUBSET);
    let rep = s.run_repertoire(0, "cat", None).expect("the run is there");
    let word: String = rep.accepted.iter().collect();
    assert!(
        word.chars().count() >= 2,
        "a one-character word cannot exercise the growing seed"
    );
    assert!(
        edit_accepts(SUBSET, "cat", &word),
        "every character of {word:?} is individually accepted, so the word must be"
    );
    // And the same characters in the other order, because a seed that grows
    // could in principle be order-sensitive.
    let reversed: String = word.chars().rev().collect();
    assert!(
        edit_accepts(SUBSET, "cat", &reversed),
        "acceptance must not depend on the order the characters are typed in"
    );
}

/// The repertoire is narrowed by the PAGE, not by the face.
///
/// `ABCDEF+Helvetica` is an embedded subset of a font that has every letter;
/// the repertoire must reflect what this file carries, which is far less. A
/// query that consulted the standard-14 table by `/BaseFont` name would accept
/// 'd' here — the exact mistake `Pass 279.0` fixed one level up.
#[test]
fn an_embedded_subset_narrows_the_repertoire_to_what_the_page_carries() {
    let s = session(SUBSET);
    let rep = s.run_repertoire(0, "cat", None).expect("the run is there");
    assert!(rep.embedded_subset, "the fixture's font is a subset");
    assert!(
        !rep.accepts('d'),
        "Helvetica has 'd'; THIS FILE does not carry it, and the repertoire \
         answers about the file"
    );
    assert!(
        rep.candidates_tested > rep.accepted.len(),
        "the face addresses more than the page carries: {} tested, {} accepted",
        rep.candidates_tested,
        rep.accepted.len()
    );
}

/// The characters actually on the page are the ones that must be accepted.
///
/// A subset carries the codes its own text uses, so "the cat" — the fixture's
/// own words — must be typeable. This is the concrete floor under the
/// whole-set test above: it names characters by hand, so a repertoire that
/// silently became empty could not pass by having nothing to check.
#[test]
fn the_characters_already_on_the_page_are_accepted() {
    let s = session(SUBSET);
    let rep = s.run_repertoire(0, "cat", None).expect("the run is there");
    for ch in "the cat".chars().filter(|c| !c.is_whitespace()) {
        assert!(
            rep.accepts(ch),
            "{ch:?} is drawn on this very page, so the subset carries its code"
        );
    }
}

/// A run that cannot be located is an error; a run with no usable encoding is
/// an empty answer. The requester asked for exactly this split.
#[test]
fn a_missing_run_errors_and_is_not_an_empty_repertoire() {
    let s = session(SUBSET);
    let r = s.run_repertoire(0, "NOT ON THIS PAGE", None);
    assert!(
        r.is_err(),
        "an editor should not open on a run that is not there, and an empty \
         repertoire would read as 'found it, nothing works'"
    );
}

/// A full (non-subset) embedded font accepts far more than a subset of the
/// same face — the control that stops every assertion above from passing
/// against a repertoire that is always small.
#[test]
fn a_full_embedded_font_accepts_more_than_a_subset_does() {
    let full = session("textedit/embedded_full.pdf");
    let sub = session(SUBSET);
    let a = full
        .run_repertoire(0, "teh", None)
        .expect("a run on the full-font fixture");
    let b = sub.run_repertoire(0, "cat", None).expect("the subset run");
    assert!(
        a.accepted.len() > b.accepted.len(),
        "a full embedded font must offer more than a subset of one: {} vs {}",
        a.accepted.len(),
        b.accepted.len()
    );
    assert!(!a.embedded_subset, "embedded_full.pdf is not a subset");
}

/// ★ The requester's second ask, by name: *"a run whose font has no usable
/// encoding at all should answer 'nothing' rather than error, so the editor
/// can decline to open rather than open and refuse every key."*
#[test]
fn a_run_with_no_usable_encoding_answers_nothing_and_says_why() {
    let s = session("text/identity-h-no-tounicode.pdf");
    let rep = s
        .run_repertoire(0, "", None)
        .expect("this is an EMPTY ANSWER, not an error");
    assert!(rep.accepted.is_empty());
    assert!(
        !rep.is_editable(),
        "an editor must be able to decline to open on this"
    );
    let reason = rep
        .reason
        .expect("an empty answer must say why it is empty");
    assert!(
        reason.contains("ToUnicode"),
        "the reason must name the missing thing, not merely report failure: {reason}"
    );
    assert_eq!(
        rep.candidates_tested, 0,
        "nothing could even be asked about"
    );
}

/// A composite (Type 0 / CIDFont) run is answered too, and by the same
/// contract.
///
/// ★ Without this the whole file would measure only the simple-font branch,
/// and the composite one — a different encoder, a different floor test — would
/// ship on the strength of the other's tests. That is this project's most
/// frequently recorded defect shape.
#[test]
fn a_composite_run_is_answered_by_the_same_contract() {
    let s = session("text/composite-editable.pdf");
    let rep = s.run_repertoire(0, "ABC", None).expect("a composite run");
    assert!(
        !rep.accepted.is_empty(),
        "this fixture is editable, so its repertoire is not empty"
    );
    for ch in &rep.accepted {
        let replace: String = std::iter::repeat_n(*ch, 2).collect();
        assert!(
            edit_accepts("text/composite-editable.pdf", "ABC", &replace),
            "the repertoire accepted {ch:?} on a composite run and edit_text refused it"
        );
    }
}

/// ★★ THE COMPOSITE SUBSET FLOOR, on the one fixture that can see it.
///
/// Disabling the composite floor stayed **green** against every other test in
/// this file: `composite-editable.pdf`'s font addresses exactly the three
/// characters the page carries, so the floor has nothing to remove there and
/// its absence is invisible. `cidfonttype2-subset-floor.pdf` addresses three
/// and carries two — measured both ways, 3 accepted with the floor disabled
/// and 2 with it enabled.
///
/// Same lesson as the simple-font side and as two other Passes today: **a
/// check is only measured by a fixture where it changes the answer.**
#[test]
fn the_composite_subset_floor_removes_a_character_the_font_addresses() {
    let s = session("text/cidfonttype2-subset-floor.pdf");
    let rep = s
        .run_repertoire(0, "", None)
        .expect("a composite subset run");
    assert!(rep.embedded_subset);
    assert_eq!(
        rep.candidates_tested, 3,
        "the font's /ToUnicode addresses three characters"
    );
    assert_eq!(
        rep.accepted.len(),
        2,
        "and the page carries two of them; the third is exactly what the floor \
         exists to remove: {:?}",
        rep.accepted
    );
}

/// ★ An empty repertoire owes a REASON even when nothing errored.
///
/// `cidfonttype2-noninjective-tounicode.pdf`'s font addresses characters —
/// every one of them by more than one code, so none can be edited
/// unambiguously (`R-INV-4`). The first cut returned an empty set with
/// `reason: None`, which is behaviourally right and tells the operator
/// nothing. A shell that greys every key must be able to say why.
#[test]
fn an_empty_repertoire_says_why_even_when_nothing_errored() {
    let s = session("text/cidfonttype2-noninjective-tounicode.pdf");
    let rep = s.run_repertoire(0, "", None).expect("not an error");
    assert!(rep.accepted.is_empty());
    assert!(!rep.is_editable());
    let reason = rep
        .reason
        .expect("an empty answer must say why, whatever produced it");
    assert!(
        reason.contains("more than one code"),
        "the reason must name the CAUSE, not merely restate the emptiness: {reason}"
    );
}

/// ★ The reported text is the RESOLVED run, not the caller's `find`.
///
/// A pinned request with an empty `find` means the whole show operator, and a
/// caller that cannot read back which run was answered about is guessing. The
/// field exists because `route_enumeration.rs` requires every anchor-locating
/// function to resolve its find — it caught `run_repertoire` as a fourth such
/// route within an hour of it being written.
#[test]
fn the_report_names_the_run_it_answered_about() {
    let s = session(SUBSET);
    let rep = s.run_repertoire(0, "cat", None).expect("the run is there");
    assert_eq!(
        rep.text, "cat",
        "an explicit find is reported verbatim: {:?}",
        rep.text
    );
    assert_eq!(rep.resource, "F1", "the run's own resource key");
    assert_eq!(rep.base_font, "ABCDEF+Helvetica");
}
