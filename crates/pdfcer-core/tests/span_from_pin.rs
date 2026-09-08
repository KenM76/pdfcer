//! # `find` says WHAT, the pin says WHICH ONE
//!
//! ## The gap
//!
//! A pin and a `find` answer different questions, and until `Pass 272.0` a
//! caller could only ask one at a time:
//!
//! - **`find` alone** — pdfcer scans the page and edits one occurrence of its
//!   own choosing. Fine when the text is unique; silently wrong when it is not.
//!   ★ And the familiar description of *which* one — *"whichever comes first"*
//!   — is **wrong**, measured: `find_anchor` looks inside a single operator
//!   first, so a **single-operator** occurrence anywhere on the page beats a
//!   **spanning** one above it. Which means a spanning run is unreachable by
//!   `find` alone whenever a single-operator twin exists at all. See
//!   `an_unpinned_request_still_scans_the_page`.
//! - **a pin alone** — exact, but the match had to lie **inside that one
//!   operator**, and a producer that emits one glyph per show operator will not
//!   have a whole run in any single one.
//!
//! A click-driven shell has exactly what the first lacks: it knows which
//! operator was touched. It had no way to say so.
//!
//! ## ★★ There is no known file on which this bites, and that is deliberate
//!
//! Requested by `pdfcer-gui` citing a bill-of-materials sheet an operator could
//! not edit. **They retracted that motivation the same day, before this
//! shipped.** The real cause was subset-embedded fonts carrying 46 of 95
//! printable ASCII characters with every lowercase letter absent — he was
//! typing letters the font does not have, and `UnsupportedFont` was right.
//!
//! They then measured this verb's actual population on all four of his sheets.
//! A run must **both** repeat on the page **and** span more than one show
//! operator: 57–133 of the first, 4–11 of the second, **intersection zero**.
//!
//! The gap is still real — it is a property of the API, not of one drawing —
//! and the tests below are synthetic for exactly that reason. But this file
//! does not claim it explains anybody's report, because it does not, and a
//! retracted measurement left standing is what a later reader would cite as
//! evidence.
//!
//! ## ★★ The reported location was one guard too late, and the truth is worse
//!
//! The report placed the fault at `find_anchor_span`'s
//! `Err(e) if req.pinned_span.is_some()` arm. **That arm is unreachable for a
//! pin that resolves** — `find_anchor` returns `Ok(i)` for a pinned request
//! without ever consulting `find`.
//!
//! Measured three ways before any code was changed:
//!
//! | request | result | what it proves |
//! |---|---|---|
//! | pin + `find` inside that operator | **succeeds** | the pin resolves |
//! | **bogus** pin | `PinnedSpanNotFound` | that arm fires only here |
//! | pin + spanning `find` | `NoMatch` | so the failure is elsewhere |
//!
//! The real site was `s.text.find(find).unwrap_or(0)`: the search missed, and
//! the fallback **claimed the match began at byte 0** of the pinned operator.
//! An anchor pointing at bytes nobody asked about, which then failed
//! downstream with a message blaming the text. That is a wrong answer standing
//! where a refusal belonged, and it is now a refusal.
//!
//! ## Why the fixture has THREE occurrences, one per sabotage that survived
//!
//! `"ABCD"` appears three times: **spanning** on line 1, in a **single
//! operator** on line 2, **spanning again** on line 3. Each was added because
//! a weaker fixture let a real defect through.
//!
//! | occurrence | catches |
//! |---|---|
//! | line 1, spanning | the feature not working at all |
//! | line 2, single operator | an implementation that **ignores the pin** and scans — with one occurrence it edits the right thing for the wrong reason |
//! | line 3, spanning again | an implementation that turns the flag on but **scans from operator 0** — with only lines 1 and 2 the pinned operator is always the first spannable one, so restricting to it changes nothing |
//!
//! ★ All three sabotages are now caught. The third was green until line 3
//! existed, and green again after that until the assertions stopped asking
//! *"does `(MNOP) Tj` appear somewhere"* and started asking **which line it
//! appeared on** — the base revision is still in the file by design, and both
//! outcomes replace exactly one spanning pair with one operator, so the
//! obvious assertions are vacuous in two different ways.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{EditError, EditOptions, EditRequest, edit_text};

/// Byte spans in the fixture's content stream, as its generator prints them.
///
/// Three occurrences of `"ABCD"`: line 1 spanning, line 2 in one operator,
/// line 3 spanning again. The generator asserts each span really names the
/// show operator rather than the `Td` before it.
const SPANNING_LINE1: (usize, usize) = (23, 7); // `(AB) Tj` on line 1
const SINGLE_OP: (usize, usize) = (65, 9); // `(ABCD) Tj` on line 2
const SPANNING_LINE3: (usize, usize) = (100, 7); // `(AB) Tj` on line 3

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/text/span-from-pin.pdf")
}

fn doc() -> Document {
    Document::load(&fixture()).expect("fixture parses")
}

fn span(v: (usize, usize)) -> ByteSpan {
    ByteSpan {
        start: v.0,
        len: v.1,
    }
}

/// The saved bytes of an edit, or the refusal.
fn run(req: &EditRequest) -> Result<Vec<u8>, EditError> {
    edit_text(&doc(), req, &EditOptions::default()).map(|o| o.bytes)
}

/// The text of one line of the NEWEST revision, named by its positioning
/// operator and running to the end of its text object.
///
/// ★ Lines have to be told apart by position, because "did `(MNOP) Tj` appear
/// somewhere" cannot distinguish *which* occurrence was edited — and a count
/// of surviving operators cannot either, since each outcome replaces exactly
/// one spanning pair with one operator. Both weaker forms were written first
/// and both passed the sabotage.
///
/// Note the shape a spanning edit writes: it **empties the earlier operators
/// and puts the replacement in the last**, so line 3 reads
/// `20 60 Td () Tj (MNOP) Tj`. Asserting the naive `20 60 Td (MNOP) Tj` fails
/// against a perfectly correct edit — which is why this helper hands back the
/// whole line and the assertions look for the text *within* it.
fn line_at(bytes: &[u8], td: &str) -> String {
    let s = String::from_utf8_lossy(bytes).into_owned();
    // The newest revision is what a viewer resolves to, so search from the
    // LAST occurrence of the positioning operator rather than the first.
    let Some(start) = s.rfind(td) else {
        return String::new();
    };
    let rest = &s[start..];
    let end = rest.find("ET").unwrap_or(rest.len());
    rest[..end].to_owned()
}

/// Does the saved file's newest revision contain this show operator?
fn shows(bytes: &[u8], token: &[u8]) -> bool {
    // An incremental save keeps the base revision, so the ORIGINAL operators
    // are still in the file by design. Search the tail — everything after the
    // first `%%EOF` — which is the appended revision.
    let tail = bytes
        .windows(5)
        .position(|w| w == b"%%EOF")
        .map_or(bytes, |i| &bytes[i..]);
    tail.windows(token.len()).any(|w| w == token)
}

/// ★★★ Pinning the SPANNING occurrence edits it, and leaves the other alone.
#[test]
fn a_pinned_spanning_run_is_the_one_edited() {
    let req = EditRequest::spanning_from(0, span(SPANNING_LINE1), "ABCD", "WXYZ");
    let out = run(&req).expect("the run beginning at the pinned operator is editable");

    assert!(
        shows(&out, b"(WXYZ) Tj"),
        "the spanning occurrence must be replaced"
    );
    assert!(
        shows(&out, b"(ABCD) Tj"),
        "and the OTHER occurrence -- the single-operator one -- must survive \
         untouched; editing it instead would be the page-scan behaviour this \
         verb exists to replace"
    );
}

/// ★★★ Pinning the SINGLE-OPERATOR occurrence edits THAT one instead.
///
/// The mirror, and the half that proves the pin is consulted at all. Same
/// `find`, same page, same document; only the pin differs, and the outcome
/// inverts.
#[test]
fn pinning_the_other_occurrence_edits_the_other_one() {
    let req = EditRequest::spanning_from(0, span(SINGLE_OP), "ABCD", "QRST");
    let out = run(&req).expect("a single-operator occurrence is still reachable this way");

    assert!(
        shows(&out, b"(QRST) Tj"),
        "the single-operator occurrence must be replaced"
    );
    assert!(
        shows(&out, b"(AB) Tj") && shows(&out, b"(CD) Tj"),
        "and the spanning occurrence must survive as its two original operators"
    );
}

/// ★★★ Pinning the SECOND spanning occurrence edits *that* one — not the
/// first one the scan would reach.
///
/// This is the test that proves the search **starts at the pin** rather than
/// merely being *enabled* by it, and it exists because a sabotage survived
/// without it. Ablating the start-at-the-pin restriction — letting the span
/// loop scan from operator 0 with the flag on — left every other test in this
/// file green, because in each of them the pinned operator happened to be the
/// first spannable one on the page.
///
/// Line 3 is a second spanning occurrence of the same text. A scan-from-zero
/// edits line 1 instead, and line 1 surviving is what catches it.
#[test]
fn the_search_starts_at_the_pin_rather_than_at_the_page() {
    let req = EditRequest::spanning_from(0, span(SPANNING_LINE3), "ABCD", "MNOP");
    let out = run(&req).expect("the third line's run is editable");

    // ★ WHICH LINE it landed on is the whole question, so the assertion has
    // to name the line. Asserting only that `(MNOP) Tj` exists somewhere, or
    // counting `(AB) Tj` over the whole file, is VACUOUS: the base revision is
    // still in the file by design, and either outcome replaces exactly one
    // spanning pair with one operator. Both of those weaker forms passed the
    // sabotage, which is how this comment came to be here.
    //
    // Lines are told apart by their positioning operator: line 1 is at
    // `20 140 Td`, line 3 at `20 60 Td`.
    let line1 = line_at(&out, "20 140 Td");
    let line3 = line_at(&out, "20 60 Td");

    assert!(
        line3.contains("(MNOP)"),
        "the PINNED (third) line must be the one replaced; it reads {line3:?}. \
         If line 1 got it instead, the span search started at the page rather \
         than at the pin"
    );
    assert!(
        !line1.contains("(MNOP)"),
        "line 1 must be untouched -- it is the occurrence a scan from the \
         start of the page reaches first; it reads {line1:?}"
    );
    assert!(
        line1.contains("(AB) Tj") && line1.contains("(CD) Tj"),
        "and line 1 must still be its own two operators; it reads {line1:?}"
    );
    assert!(
        shows(&out, b"(ABCD) Tj"),
        "and line 2, the single-operator occurrence, must survive"
    );
}

/// ★★ Without the flag, a pinned spanning request is REFUSED BY NAME.
///
/// This is the behaviour the consuming shell asked to keep: a plain pin still
/// confines the match to one operator, so no existing caller's refusal changes
/// meaning. What changed is that the refusal is now *deliberate* — it used to
/// resolve to byte 0 and fail further downstream.
#[test]
fn a_plain_pin_still_confines_the_match_to_one_operator() {
    let mut req = EditRequest::find_replace(0, "ABCD", "WXYZ");
    req.pinned_span = Some(span(SPANNING_LINE1));
    // NOT `spanning_from` -- the flag is deliberately absent.

    let err = run(&req).expect_err("the find does not lie inside the pinned operator");
    assert!(
        matches!(err, EditError::NoMatch(ref s) if s == "ABCD"),
        "refused by name, naming the text that was not found there: {err:?}"
    );
}

/// A pin that names nothing is still a DIFFERENT refusal, flag or no flag.
///
/// Pinned here to keep the two failures apart. Conflating them is what sent
/// the original report to the wrong guard: `PinnedSpanNotFound` means the pin
/// is wrong, `NoMatch` means the pin is fine and the text is not where it was
/// looked for, and only the second is what a repeated-text page produces.
#[test]
fn a_bogus_pin_is_reported_as_a_bad_pin_not_as_missing_text() {
    let bogus = ByteSpan {
        start: 9_999,
        len: 7,
    };
    for req in [EditRequest::spanning_from(0, bogus, "ABCD", "WXYZ"), {
        let mut r = EditRequest::find_replace(0, "ABCD", "WXYZ");
        r.pinned_span = Some(bogus);
        r
    }] {
        let err = run(&req).expect_err("the pin names no operator");
        assert!(
            matches!(err, EditError::PinnedSpanNotFound { .. }),
            "a bad pin must not be reported as missing text: {err:?}"
        );
    }
}

/// An unpinned request is completely unaffected — and what it does is **not**
/// "the first occurrence".
///
/// ★ MEASURED, AND IT CORRECTED THIS FILE'S OWN FIRST DRAFT. The page scan
/// edits the **single-operator** occurrence on line 2, not the spanning one on
/// line 1 that comes before it. The reason is the ladder's order:
/// `find_anchor` runs first and looks for `find` inside **one** operator's
/// text, so it reaches the later line; the spanning search is only tried when
/// that fails.
///
/// So the familiar description of the old behaviour — *"it edits whichever
/// occurrence comes first"* — is wrong, and wrong in a direction that matters:
/// **a spanning occurrence is unreachable by `find` alone whenever a
/// single-operator twin exists anywhere on the page**, however far down. That
/// is a stronger argument for this Pass than the one it was requested with,
/// and it is only visible because the fixture carries both shapes.
#[test]
fn an_unpinned_request_still_scans_the_page() {
    let out = run(&EditRequest::find_replace(0, "ABCD", "WXYZ")).expect("page scan still works");
    assert!(
        shows(&out, b"(WXYZ) Tj"),
        "the page scan must still edit something"
    );
    assert!(
        shows(&out, b"(AB) Tj") && shows(&out, b"(CD) Tj"),
        "and it is the SINGLE-OPERATOR occurrence it takes -- the spanning one \
         survives as its two original operators, because `find_anchor` looks \
         inside one operator first and only falls through to the span search \
         when nothing on the page satisfies that"
    );
}

/// The constructor sets what it says it sets.
///
/// Cheap, and it is the assertion that stops the four above from silently
/// testing `find_replace` if `spanning_from` ever stopped setting the flag.
#[test]
fn the_constructor_sets_both_the_pin_and_the_flag() {
    let s = span(SPANNING_LINE1);
    let req = EditRequest::spanning_from(3, s, "find me", "replaced");
    assert_eq!(req.page_index, 3);
    assert_eq!(req.pinned_span, Some(s));
    assert!(req.span_from_pin);
    assert_eq!(req.find, "find me");
    assert_eq!(req.replace, "replaced");

    // And the plain constructor does NOT set it.
    assert!(!EditRequest::find_replace(0, "a", "b").span_from_pin);
}
