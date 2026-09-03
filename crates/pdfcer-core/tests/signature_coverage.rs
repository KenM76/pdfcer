//! `/ByteRange` coverage — what a signature actually protects, measured
//! without any cryptography.
//!
//! # The question this answers, and the one it does not
//!
//! It does NOT say a signature is valid. It says what a signature would
//! be valid OVER. Those are different claims, and the second is the one
//! an operator cannot get from a green badge: a signature can be
//! cryptographically perfect over the first 40 KB of a 900 KB file.
//!
//! # Why the modality drives the assertions
//!
//! §12.8.1 makes whole-file coverage a **`should`**, not a `shall` —
//! *"Other ranges may be used but ... their use is not recommended."* So
//! a short range is CONFORMING and must not be reported as malformed,
//! while an OVERLAPPING range violates Table 252's "exact byte range"
//! and must be. Two fixtures exist precisely so those two cannot be
//! collapsed into one verdict.
//!
//! # Why new fixtures were needed
//!
//! Neither existing candidate exercises the real shape, and a test
//! against either would have been vacuous in the way that looks like a
//! pass: `forms/certified-p2-form.pdf` has `/ByteRange [0 1 2 3]` and no
//! signature FIELD at all, and `forms/unfillable-fields-form.pdf` has a
//! `/FT /Sig` field with no `/V`. See `tools/gen-signature-fixtures.py`.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::signature;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/signature")
        .join(name)
}

/// Load a fixture and measure it against its own real byte length.
fn coverage(name: &str) -> (Vec<signature::ByteRangeCoverage>, u64) {
    let path = fixture(name);
    let len = std::fs::metadata(&path).expect("stat fixture").len();
    let doc = Document::load(&path).expect("load fixture");
    let session = EditSession::new(doc);
    (signature::byte_range_coverage(&session.graph(), len), len)
}

/// A signature covering the whole file reports exactly that.
///
/// The two-pair shape is asserted as well as the total: one pair would
/// mean `/Contents` sits inside its own digest, which cannot verify, and
/// a reader that accepted it would be reporting coverage for a signature
/// that could never be valid.
#[test]
fn full_coverage_reaches_the_last_byte() {
    let (cov, len) = coverage("signed-full-coverage.pdf");
    let c = cov
        .first()
        .expect("the fixture has a signed /FT /Sig field");

    assert_eq!(c.field_name.as_deref(), Some("Approval"));
    assert_eq!(c.file_len, len);
    assert_eq!(c.pair_count, 2, "two ranges straddling /Contents: {c:?}");
    assert!(c.ranges_well_formed, "{c:?}");
    assert_eq!(c.uncovered_tail, 0, "nothing lies past the signed range");
    assert!(c.covers_to_eof());
    // The digest necessarily covers LESS than the file: the /Contents
    // hole is excluded by construction. A reader reporting covered ==
    // file_len would have silently included the signature value in its
    // own digest.
    assert!(
        c.covered < c.file_len,
        "the /Contents hole is excluded: {c:?}"
    );
}

/// A short range is reported as short — and NOT as malformed.
///
/// §12.8.1's `should`. This is the assertion that keeps pdfcer from
/// calling a conforming document broken, and it is one half of a pair:
/// the other test asserts a genuinely malformed range reports
/// differently.
#[test]
fn a_short_range_is_under_protecting_but_conforming() {
    let (cov, len) = coverage("signed-short-coverage.pdf");
    let c = cov.first().expect("signed fixture");

    assert_eq!(c.uncovered_tail, 200, "200 bytes lie past the signed range");
    assert!(!c.covers_to_eof());
    assert!(
        c.ranges_well_formed,
        "a SHORT range is conforming — only overlap is malformed: {c:?}"
    );
    assert_eq!(c.file_len, len);
}

/// Overlapping ranges ARE malformed, and report differently from short.
///
/// Without this, "well formed" could be a constant `true` and the short
/// test above would still pass. The two together are what make either
/// mean anything.
#[test]
fn overlapping_ranges_are_malformed() {
    let (cov, _) = coverage("signed-malformed-range.pdf");
    let c = cov.first().expect("signed fixture");

    assert!(
        !c.ranges_well_formed,
        "Table 252's ranges are exact; overlap is not permitted: {c:?}"
    );
}

/// The three fixtures do not all report the same thing.
///
/// A guard against the whole measurement degenerating: if a future change
/// made `byte_range_coverage` return a constant, every test above could
/// still pass individually while the function had stopped measuring
/// anything.
#[test]
fn the_three_shapes_are_distinguishable() {
    let full = coverage("signed-full-coverage.pdf").0;
    let short = coverage("signed-short-coverage.pdf").0;
    let bad = coverage("signed-malformed-range.pdf").0;

    let f = full.first().expect("signed");
    let s = short.first().expect("signed");
    let b = bad.first().expect("signed");

    assert_ne!(f.uncovered_tail, s.uncovered_tail, "full vs short");
    assert_ne!(
        f.ranges_well_formed, b.ranges_well_formed,
        "full vs malformed"
    );
    assert_ne!(
        s.ranges_well_formed, b.ranges_well_formed,
        "short vs malformed"
    );
}
