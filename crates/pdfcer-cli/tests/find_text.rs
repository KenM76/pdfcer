//! CLI tests for `find-text` — the first slice of the Reader-parity sweep.
//!
//! # Why this command exists
//!
//! A 2026-08-10 audit against Acrobat Reader found pdfcer well ahead on
//! editing and behind on plain consumption. Text search was the starkest
//! case: `pdfcer-core` has had the whole scan — extract, match, turn the
//! matched glyph span into a page-space quad — since redaction's
//! search-to-mark shipped, but it was **buried inside a mutating
//! redaction verb** and unreachable on its own. pdfcer could find text
//! only as a side effect of marking it for destruction.
//!
//! # The assertion that matters most
//!
//! [`find_and_redaction_search_agree_on_geometry`]. The scan is now
//! shared between `find-text` and `redact-mark --search`, and the reason
//! is not code reuse — it is that two copies of glyph-span-to-quad
//! geometry drift in the worst available direction: **a redaction
//! covering a slightly different box than the search that found it.** An
//! operator searches, sees a hit, marks it, and the mark is not quite
//! where the hit was. Sharing one scanner makes that unrepresentable;
//! this test makes the sharing observable from outside.
//!
//! # The documented limits are tested as limits
//!
//! Two things `find-text` deliberately does not do are asserted here so
//! they stay deliberate: it does not match `/ActualText` runs (they carry
//! no per-glyph geometry, so a hit could be counted but not located), and
//! finding nothing exits `0` (a search that matched nothing succeeded —
//! a non-zero exit would make "no hits" indistinguishable from "could not
//! read the file" in a pipeline).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

fn code(out: &Output) -> u8 {
    u8::try_from(out.status.code().expect("process was killed by a signal")).unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout must be valid UTF-8")
}

/// A hit is reported with its page and its box, not merely counted.
///
/// "page 1" is not an answer when a word appears six times on it. The
/// rectangle is what lets a caller draw a box, crop an image, or hand a
/// coordinate to another command.
#[test]
fn a_hit_reports_where_it_is() {
    let f = fixture("text/composite-editable.pdf");
    let out = run(&["find-text", &f.display().to_string(), "--needle", "ABC"]);
    assert_eq!(code(&out), 0);
    let s = stdout(&out);

    assert!(s.contains("matches=1"), "{s}");
    let hit = s
        .lines()
        .find(|l| l.starts_with("match "))
        .unwrap_or_else(|| panic!("no match line in:\n{s}"));
    assert!(hit.contains("page=1"), "1-based page: {hit}");
    assert!(
        hit.contains("rect=72.00,589.44,158.40,640.80"),
        "the on-page box must be reported: {hit}"
    );
}

/// A case-insensitive hit reports the text as the DOCUMENT spells it.
///
/// Searching `abc` and being told the document contains `abc` would be a
/// small lie with a real cost: an operator reviewing hits before redacting
/// needs to see which casing they actually matched.
#[test]
fn a_case_insensitive_hit_reports_the_documents_own_spelling() {
    let f = fixture("text/composite-editable.pdf");
    let out = run(&[
        "find-text",
        &f.display().to_string(),
        "--needle",
        "abc",
        "--ignore-case",
    ]);
    assert_eq!(code(&out), 0);
    let s = stdout(&out);
    assert!(s.contains("matches=1"), "{s}");
    assert!(
        s.contains(r#"text="ABC""#),
        "the matched text must be the document's, not the needle's:\n{s}"
    );
}

/// ★ `find-text` and `redact-mark --search` produce the SAME box.
///
/// Both come from one scan in core. This asserts it from outside, against
/// the bytes `redact-mark` actually wrote, so the sharing is observable
/// rather than a claim in a doc comment — and so that splitting the
/// scanner back into two copies has to break a test.
///
/// The `/QuadPoints` array is compared, not `/Rect`: the annotation's
/// rect carries a small margin around the covered area, which is a
/// separate and deliberate difference. Comparing rects would fail for a
/// reason that has nothing to do with the property under test.
#[test]
fn find_and_redaction_search_agree_on_geometry() {
    let f = fixture("text/composite-editable.pdf");

    let found = stdout(&run(&[
        "find-text",
        &f.display().to_string(),
        "--needle",
        "ABC",
    ]));
    assert!(found.contains("matches=1"), "{found}");

    let dir = std::env::temp_dir().join(format!("pdfcer-findtext-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let marked = dir.join("marked.pdf");
    let out = run(&[
        "redact-mark",
        &f.display().to_string(),
        "--search",
        "ABC",
        "-o",
        &marked.display().to_string(),
    ]);
    assert_eq!(code(&out), 0, "redact-mark failed");

    let bytes = std::fs::read(&marked).expect("read the marked file");
    let text = String::from_utf8_lossy(&bytes);
    let quad = text
        .split("QuadPoints [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap_or_else(|| panic!("no /QuadPoints in the marked file"));

    // The four corner pairs, as the writer rounded them.
    let nums: Vec<f64> = quad
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect();
    assert_eq!(nums.len(), 8, "a quad is four points: {quad:?}");
    let xs = [nums[0], nums[2], nums[4], nums[6]];
    let ys = [nums[1], nums[3], nums[5], nums[7]];
    let lo = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let expect = format!(
        "rect={:.2},{:.2},{:.2},{:.2}",
        lo(xs),
        lo(ys),
        hi(xs),
        hi(ys)
    );
    assert!(
        found.contains(&expect),
        "find-text and redact-mark must report the same geometry.\n\
         redact-mark wrote {expect}\nfind-text said:\n{found}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// An `/ActualText` run is NOT matched, and that is deliberate.
///
/// Those runs carry a replacement string with no per-glyph geometry, so a
/// match inside one could be counted but not located — and a hit the
/// caller cannot point at is worse than no hit, because it invites a
/// redaction that has nowhere to go.
///
/// `actual-text-drucker.pdf` extracts as "Druc"/"ker" through
/// `/ActualText`, so searching its visible text finds nothing. Asserted
/// so the limit stays a decision rather than becoming a bug report.
#[test]
fn actual_text_runs_are_not_matched() {
    let f = fixture("text/actual-text-drucker.pdf");
    let out = run(&["find-text", &f.display().to_string(), "--needle", "Druc"]);
    assert_eq!(code(&out), 0);
    assert!(
        stdout(&out).contains("matches=0"),
        "an /ActualText run has no glyph geometry to point at:\n{}",
        stdout(&out)
    );
}

/// Finding nothing is a SUCCESSFUL search.
///
/// A non-zero exit would make "no hits" indistinguishable from "could not
/// read the file" in a shell pipeline. The count is on the summary line
/// for a caller that wants to branch on it.
#[test]
fn no_matches_still_exits_zero() {
    let f = fixture("text/composite-editable.pdf");
    let out = run(&[
        "find-text",
        &f.display().to_string(),
        "--needle",
        "zzzznotpresent",
    ]);
    assert_eq!(code(&out), 0, "an empty result is not a failure");
    assert!(stdout(&out).contains("matches=0"));
}

// ---------------------------------------------------------------------------
// `Pass 127.0` — a zero match count is not evidence the needle is absent
// ---------------------------------------------------------------------------

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr must be valid UTF-8")
}

fn type3_fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/type3")
        .join(rel)
}

/// A search over a document containing unsearchable text SAYS SO, on the
/// summary line and in prose, and says it even when it found something.
///
/// # Why this is a test and not a nicety
///
/// `matches=0` has two causes and one appearance: the needle is absent, or
/// the document's text was never recoverable as Unicode so no needle could
/// have matched it. A Type 3 font (ISO 32000-1 §9.6.5) draws each glyph with
/// a content stream named by an arbitrary `/CharProcs` key, so without a
/// `/ToUnicode` CMap there is no sourced route to Unicode at all — and the
/// text **renders perfectly**, which is what makes the failure invisible.
///
/// The fixture holds three Type 3 fonts: `/TA` with a `/ToUnicode` (so
/// `HI!` is findable) and `/TB`/`/TC` with none. Asserting the disclosure on
/// the SUCCESSFUL search is deliberate — a successful search still owes the
/// operator the rest of the document, and a disclosure that only appears on
/// a zero-hit run would be silent in exactly the mixed case real documents
/// produce.
#[test]
fn a_search_discloses_the_text_it_could_not_read() {
    let f = type3_fixture("tounicode_gate.pdf");
    let out = run(&["find-text", &f.display().to_string(), "--needle", "HI!"]);
    assert_eq!(code(&out), 0);

    let s = stdout(&out);
    assert!(
        s.contains("matches=2"),
        "the /ToUnicode-backed run is found: {s}"
    );
    assert!(
        s.contains("type3_no_tounicode=2"),
        "the two fonts with no /ToUnicode ride the machine-readable line, so a \
         script can branch without parsing prose: {s}"
    );
    assert!(s.contains("unreadable_codes=2"), "{s}");
    assert!(s.contains("identity_no_tounicode=0"), "{s}");

    let e = stderr(&out);
    assert!(
        e.contains("Type 3 font(s)") && e.contains("/ToUnicode"),
        "and the prose explains it, on stderr so it cannot contaminate a \
         `find-text > hits.txt` capture: {e}"
    );
    assert!(
        e.contains("§9.6.5"),
        "the clause is cited, so an operator can check the claim: {e}"
    );
}

/// The zero-hit case — the one that matters — carries the same disclosure.
///
/// Without it the operator reads "not found" as "not present", which is the
/// wrong conclusion in the one situation where being wrong is expensive: the
/// word IS on the page, and they can see it.
#[test]
fn a_zero_hit_search_still_says_what_was_unreadable() {
    let f = type3_fixture("tounicode_gate.pdf");
    let out = run(&[
        "find-text",
        &f.display().to_string(),
        "--needle",
        "zzzznotpresent",
    ]);
    assert_eq!(code(&out), 0, "an empty result is not a failure");

    let s = stdout(&out);
    assert!(s.contains("matches=0"), "{s}");
    assert!(s.contains("type3_no_tounicode=2"), "{s}");

    assert!(
        stderr(&out).contains("not evidence the needle is absent"),
        "the conclusion the operator must NOT draw is named outright: {}",
        stderr(&out)
    );
}

/// A document with nothing unreadable in it says nothing — the disclosure is
/// conditional, not boilerplate.
///
/// A warning printed on every run is a warning nobody reads, and it would
/// make the counters above meaningless as a signal. The zeros still ride the
/// summary line for a script; only the prose is suppressed.
#[test]
fn a_fully_readable_document_gets_no_warning() {
    let f = fixture("text/composite-editable.pdf");
    let out = run(&["find-text", &f.display().to_string(), "--needle", "ABC"]);
    assert_eq!(code(&out), 0);

    assert!(
        stdout(&out).contains("type3_no_tounicode=0"),
        "the counter is always present: {}",
        stdout(&out)
    );
    assert!(
        !stderr(&out).contains("Type 3 font(s)"),
        "but the prose is not: {}",
        stderr(&out)
    );
}

/// `Pass 127.1` — `redact-mark --search` warns about text it could not read,
/// **even when it marked something**.
///
/// The partial case is the dangerous one. A run that authors two marks reads
/// as success at the shell, and the exit code says so; nothing in either
/// hints that a third occurrence sat in a font the scan could never match.
#[test]
fn a_search_driven_redaction_warns_about_unreadable_text() {
    let f = type3_fixture("tounicode_gate.pdf");
    let out_pdf = std::env::temp_dir().join("pdfcer-redact-disclosure-test.pdf");
    let out = run(&[
        "redact-mark",
        &f.display().to_string(),
        "--search",
        "HI!",
        "--output",
        &out_pdf.display().to_string(),
    ]);
    assert_eq!(code(&out), 0);

    let s = stdout(&out);
    assert!(
        s.contains("marks_created=2"),
        "the readable run is still marked: {s}"
    );

    let e = stderr(&out);
    assert!(
        e.contains("could not be mapped to Unicode"),
        "the unreadable codes must be named on a SUCCESSFUL run: {e}"
    );
    assert!(
        e.contains("Type 3 font(s)") && e.contains("§9.6.5"),
        "with the reason and the clause: {e}"
    );
    assert!(
        e.contains("DO NOT treat this document as cleared"),
        "and the conclusion the operator must not draw, said outright: {e}"
    );

    let _ = std::fs::remove_file(&out_pdf);
}

/// A fully readable document gets no redaction warning.
///
/// The control. A warning printed on every run is a warning nobody reads, and
/// it would make the real one worthless.
#[test]
fn a_readable_document_gets_no_redaction_warning() {
    let f = fixture("text/composite-editable.pdf");
    let out_pdf = std::env::temp_dir().join("pdfcer-redact-clean-test.pdf");
    let out = run(&[
        "redact-mark",
        &f.display().to_string(),
        "--search",
        "ABC",
        "--output",
        &out_pdf.display().to_string(),
    ]);
    assert_eq!(code(&out), 0);
    assert!(
        !stderr(&out).contains("could not be mapped to Unicode"),
        "nothing was unreadable here: {}",
        stderr(&out)
    );
    let _ = std::fs::remove_file(&out_pdf);
}
