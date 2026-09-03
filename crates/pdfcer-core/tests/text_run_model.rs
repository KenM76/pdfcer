//! `TextObject::runs` carries per-run **byte spans** and **positioning** —
//! the `Pass 32.0` substrate (ISO 32000-1 §9.4).
//!
//! ## Why this is its own test file, before any deletion verb exists
//!
//! `Pass 32.0` deletes one show operator out of a `BT`…`ET` that may hold
//! hundreds — on the operator's own drawing, one text object holds all 237
//! dimension labels. Two model properties have to be right before a verb
//! can be built on them, and **both fail silently if they are wrong**:
//!
//! 1. **The byte span.** A span off by one operator deletes a *different
//!    label* from the one picked. The result is well-formed, round-trips
//!    cleanly, and no structural check can see it. The only thing that
//!    catches it is a fixture whose runs are individually identifiable in
//!    the source bytes — which is why the fixture's four strings are
//!    distinct single words that each appear exactly once.
//! 2. **Whether a run's position is inherited.** §9.4.2 leaves the text
//!    matrix advanced past the string just drawn, so a run with no
//!    positioning operator before it has no coordinates anywhere in the
//!    file. Delete its predecessor and it moves.
//!
//! ## The existing corpus could not test either
//!
//! `text/scattered-text-one-object.pdf` positions **both** its runs with an
//! explicit `Tm`. Every run in it is `Explicit`, so an implementation that
//! hard-coded `Explicit` would pass against it — R162, an assertion that
//! cannot come out false. Hence `runs-inherited.pdf`, whose four runs are
//! Explicit / Inherited / Explicit / Inherited in that order: the third
//! proves the latch is not `Tm`-only, and the fourth proves it re-arms.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::vector::{Matrix, RunPositioning, TextObject, VectorObject, decompose_page};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

/// The first text object of a fixture, plus the page's decoded content
/// bytes — the spans are indices into exactly those bytes, so a test that
/// checked them against anything else would be checking nothing.
fn text_of(name: &str) -> (TextObject, Vec<u8>) {
    let bytes = std::fs::read(fixture(name))
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}", fixture(name).display()));
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = pages.first().expect("one page").clone();
    let model = decompose_page(&doc.view(), &page, Matrix::IDENTITY).expect("decomposes");
    let text = model
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => Some(t.clone()),
            _ => None,
        })
        .expect("the fixture has a text object");
    // The SAME decode `decompose_page` performed — the spans index exactly
    // these bytes, so re-deriving them any other way would test nothing.
    let content = ContentStream::from_page(&doc.view(), &page)
        .expect("page content decodes")
        .buf;
    (text, content)
}

// ---------------------------------------------------------------------------
// Byte spans
// ---------------------------------------------------------------------------

/// **Each run's span covers its own show operator and nothing else.**
///
/// Asserted by slicing the content buffer with the recorded span and
/// checking the slice contains that run's string and **none** of the other
/// three. Containment of its own string alone would be satisfied by a span
/// covering the whole `BT`…`ET`; the exclusions are what make it a real
/// assertion.
#[test]
fn every_run_span_covers_its_own_show_operator_and_no_other() {
    let (text, content) = text_of("runs-inherited.pdf");
    let names = ["ALPHA", "BETA", "GAMMA", "DELTA"];
    assert_eq!(
        text.runs.len(),
        4,
        "the fixture has four show operators; got {} — if this is 1, runs \
         are being unioned, and if it is 0 the font stopped resolving",
        text.runs.len(),
    );

    for (i, run) in text.runs.iter().enumerate() {
        let slice = content
            .get(run.bytes.start..run.bytes.end())
            .expect("the span is inside the content buffer");
        let s = String::from_utf8_lossy(slice);
        assert!(
            s.contains(names[i]),
            "run {i}'s span must cover its own string {:?}; it covers {s:?}",
            names[i],
        );
        for (j, other) in names.iter().enumerate() {
            if i == j {
                continue;
            }
            assert!(
                !s.contains(other),
                "run {i}'s span also covers run {j}'s string {other:?} — a span \
                 this wide would delete more than the operator picked, and the \
                 result would still be a valid PDF: {s:?}",
            );
        }
        assert!(
            s.contains("Tj"),
            "run {i}'s span must reach its OPERATOR, not just its operand — \
             deleting the string and leaving a bare `Tj` is malformed: {s:?}",
        );
    }
}

/// Spans are in content order and do not overlap — the precondition every
/// multi-run edit depends on, since overlapping edits are silently dropped
/// by the splice rather than applied.
#[test]
fn run_spans_are_ordered_and_disjoint() {
    let (text, _) = text_of("runs-inherited.pdf");
    for pair in text.runs.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(
            a.bytes.end() <= b.bytes.start,
            "run spans overlap or run backwards: {:?} then {:?}",
            a.bytes,
            b.bytes,
        );
    }
}

// ---------------------------------------------------------------------------
// Positioning — the guard the deletion verb is gated on
// ---------------------------------------------------------------------------

/// **The four cases, in one fixture.** Explicit, Inherited, Explicit-via-`Td`,
/// Inherited-again.
///
/// Each of the last three exists to kill a specific wrong implementation:
/// all-`Explicit` (which the pre-existing corpus could not distinguish),
/// `Tm`-only latching, and a latch that clears once and never re-arms.
#[test]
fn run_positioning_distinguishes_explicit_from_inherited() {
    let (text, _) = text_of("runs-inherited.pdf");
    let got: Vec<RunPositioning> = text.runs.iter().map(|r| r.positioned_by).collect();
    assert_eq!(
        got,
        vec![
            RunPositioning::Explicit,  // a `Tm` precedes it, and it is first
            RunPositioning::Inherited, // nothing between it and ALPHA
            RunPositioning::Explicit,  // a `Td` — proves the latch is not Tm-only
            RunPositioning::Inherited, // proves the latch RE-ARMS
        ],
        "got {got:?}",
    );
}

/// The **first** run of a text object is `Explicit` even with no positioning
/// operator at all: `BT` resets the text and line matrices to the identity
/// (§9.4.1), which is an origin of its own.
///
/// Checked on the `TJ` fixture, whose single run is preceded by a `Tm` — and
/// on the reasoning that a first run could not be "inherited" from anything,
/// since there is no predecessor to inherit from. An implementation that
/// initialised the latch to `false` would report `Inherited` here and make
/// every text object's first run undeletable for no reason.
#[test]
fn the_first_run_of_a_text_object_is_never_inherited() {
    for name in ["runs-inherited.pdf", "runs-tj-array.pdf"] {
        let (text, _) = text_of(name);
        assert_eq!(
            text.runs.first().map(|r| r.positioned_by),
            Some(RunPositioning::Explicit),
            "{name}: the first run has no predecessor to inherit from",
        );
    }
}

// ---------------------------------------------------------------------------
// A `TJ` array is ONE run
// ---------------------------------------------------------------------------

/// `[(A) -120 (B) -120 (C)] TJ` is **one** run, not three.
///
/// The numeric elements are kerning within a single positioned string, not
/// separate placements. An implementation counting show *strings* rather
/// than show *operators* would report 3 — and "delete this run" would then
/// mean deleting one letter out of a word, which is not an operation
/// anybody asked for.
#[test]
fn a_tj_array_is_one_run_however_many_strings_it_holds() {
    let (text, content) = text_of("runs-tj-array.pdf");
    assert_eq!(text.runs.len(), 1, "got {} runs", text.runs.len());

    let run = text.runs[0];
    let slice = content
        .get(run.bytes.start..run.bytes.end())
        .expect("span inside the buffer");
    let s = String::from_utf8_lossy(slice);
    for needle in ["(A)", "(B)", "(C)", "TJ"] {
        assert!(
            s.contains(needle),
            "the run's span must cover the WHOLE array including {needle:?}: {s:?}",
        );
    }
}

// ---------------------------------------------------------------------------
// The pre-existing corpus still behaves
// ---------------------------------------------------------------------------

/// The scattered-text fixture keeps its two runs, and both are `Explicit`
/// — pinned so that this file documents *why* it could not have caught the
/// inherited case, rather than leaving that as a claim in the header.
#[test]
fn the_scattered_text_fixture_has_only_explicit_runs() {
    let (text, _) = text_of("scattered-text-one-object.pdf");
    assert_eq!(text.runs.len(), 2);
    assert!(
        text.runs
            .iter()
            .all(|r| r.positioned_by == RunPositioning::Explicit),
        "both runs are placed by their own `Tm`; if this ever fails, that \
         fixture has changed and the R162 argument in this file's header \
         needs revisiting",
    );
}

// ---------------------------------------------------------------------------
// Per-run text (`TextObject::run_text`)
// ---------------------------------------------------------------------------

/// **Each run reports its OWN string, not the object's whole preview.**
///
/// This is the assertion the accessor exists for. `TextObject::preview`
/// decodes a whole `BT`…`ET` into one running string, so before this
/// existed the only answer to *"what does run 2 say?"* was
/// `"ALPHABETAGAMMADELTA"` — the same answer for all four runs.
///
/// The exclusions carry the weight. Checking only that run 1 contains
/// `"BETA"` would pass against an implementation that returned the whole
/// preview for every index (R162 — an assertion that cannot come out
/// false), because the preview contains `"BETA"` too. Requiring EQUALITY,
/// and requiring the other three names to be absent, is what makes this
/// fail if the ranges are wrong.
#[test]
fn each_run_reports_its_own_string_and_not_the_objects_whole_preview() {
    let (text, _) = text_of("runs-inherited.pdf");
    let names = ["ALPHA", "BETA", "GAMMA", "DELTA"];
    assert_eq!(text.runs.len(), 4, "the fixture has four show operators");

    for (i, expected) in names.iter().enumerate() {
        let got = text
            .run_text(i)
            .unwrap_or_else(|| panic!("run {i} has no readable text"));
        assert_eq!(
            got, *expected,
            "run {i} should read exactly {expected:?}, got {got:?} — if this \
             is the whole concatenation, the range is not being sliced"
        );
        for (j, other) in names.iter().enumerate() {
            if i != j {
                assert!(
                    !got.contains(other),
                    "run {i} ({got:?}) leaked run {j}'s text ({other:?})"
                );
            }
        }
    }
}

/// **An out-of-range index is `None`, not a panic and not run 0.**
///
/// The DXF exporter walks runs by index; a wrapping or clamping accessor
/// would emit a duplicate label rather than stopping.
#[test]
fn an_out_of_range_run_index_is_none() {
    let (text, _) = text_of("runs-inherited.pdf");
    assert!(text.run_text(4).is_none(), "index 4 of a 4-run object");
    assert!(text.run_text(usize::MAX).is_none());
}

/// **The ranges tile the preview in order, with no gap and no overlap.**
///
/// A structural check that does not depend on the fixture's particular
/// words: concatenating every run's text must reproduce the preview
/// exactly. If two ranges overlapped, the concatenation would be longer
/// than the preview; if one were dropped or short, shorter.
#[test]
fn the_run_ranges_tile_the_preview_exactly() {
    let (text, _) = text_of("runs-inherited.pdf");
    let joined: String = (0..text.runs.len())
        .map(|i| text.run_text(i).expect("readable"))
        .collect();
    let pdfcer_core::vector::TextPreview::Decoded { text: whole, .. } = &text.preview else {
        panic!("the fixture decodes");
    };
    assert_eq!(
        &joined, whole,
        "the per-run ranges must tile the preview with no gap or overlap"
    );
    let mut prev_end = 0;
    for (i, run) in text.runs.iter().enumerate() {
        assert_eq!(
            run.text_start, prev_end,
            "run {i} does not abut its predecessor"
        );
        assert!(
            run.text_end >= run.text_start,
            "run {i} has a backwards range"
        );
        prev_end = run.text_end;
    }
}
