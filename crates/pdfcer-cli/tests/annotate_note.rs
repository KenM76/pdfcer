//! # `pdfcer annotate --note / --note-author / --note-date` (`Pass 150.0`)
//!
//! Black-box over the **real binary**, and every assertion reads the *output
//! file* back through `list-annotations` rather than trusting what `annotate`
//! printed about itself.
//!
//! ## ★ The decision these tests pin
//!
//! **pdfcer does not read a clock.** `--note-date` is the caller's PDF date
//! string, written verbatim, and a malformed one is **refused by name** rather
//! than written or silently replaced with "now". Two reasons: byte-identical
//! output for identical input is an acceptance criterion across this project,
//! and a timestamp pdfcer chose would be a value it *inferred* and wrote
//! silently into the operator's document.
//!
//! A refusal here is cheap. A wrong `/M` is not: the read side hands it back as
//! an opaque string, so it looks authoritative and nothing downstream would
//! ever report it.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_note_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Author a square carrying `extra`, returning the output path.
fn annotate(extra: &[&str], tag: &str) -> (Output, PathBuf) {
    let out = temp_path(tag);
    let src = fixture();
    let mut args = vec![
        "annotate",
        src.to_str().unwrap(),
        "--type",
        "square",
        "--page",
        "1",
        "--rect",
        "10,10,60,40",
        "--color",
        "000000",
    ];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap()]);
    (run(&args), out)
}

/// `list-annotations`' summary line, read from the output file.
fn summary(path: &Path) -> String {
    let o = run(&["list-annotations", path.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    stdout(&o)
        .lines()
        .find(|l| l.starts_with("list-annotations"))
        .unwrap_or_default()
        .to_owned()
}

// ---------------------------------------------------------------------------

#[test]
fn a_note_with_author_and_date_reaches_the_saved_file() {
    let (r, out) = annotate(
        &[
            "--note",
            "Check this dimension",
            "--note-author",
            "Ken",
            "--note-date",
            "D:20260828073200Z",
        ],
        "full",
    );
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));

    let s = summary(&out);
    assert!(s.contains("with_note=1"), "{s}");
    assert!(s.contains("with_author=1"), "{s}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn an_author_alone_writes_a_title_without_any_note_text() {
    // "Attribute this shape to me, I have nothing to say about it" is a real
    // request; requiring --note first would refuse it for no reason.
    let (r, out) = annotate(&["--note-author", "Ken"], "authoronly");
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    assert!(summary(&out).contains("with_author=1"), "{}", summary(&out));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn no_note_flags_leave_the_annotation_wordless_as_before() {
    // The default must stay what every markup authored before this Pass got.
    let (r, out) = annotate(&[], "none");
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let s = summary(&out);
    assert!(s.contains("with_note=0"), "{s}");
    assert!(s.contains("with_author=0"), "{s}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_human_readable_date_is_refused_by_name_and_writes_nothing() {
    let (r, out) = annotate(&["--note", "x", "--note-date", "28 August 2026"], "baddate");
    assert_eq!(r.status.code(), Some(EDIT_REFUSED), "{}", stdout(&r));
    let err = stderr(&r);
    assert!(err.contains("is not a PDF date string"), "{err}");
    assert!(
        err.contains("28 August 2026"),
        "it shows what was rejected: {err}"
    );
    assert!(err.contains("§7.9.4"), "and cites the clause: {err}");
    assert!(err.contains("Nothing was written"), "{err}");
    assert!(!out.exists(), "and no file was produced");
}

#[test]
fn a_bare_year_is_accepted_because_section_7_9_4_says_so() {
    // Every trailing component of a PDF date is optional. A validator that
    // demanded full precision would refuse dates the standard permits.
    let (r, out) = annotate(&["--note", "x", "--note-date", "D:2026"], "bareyear");
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn the_help_says_pdfcer_will_not_supply_the_date() {
    // The reason is operator-facing, not just an internal doc comment: a
    // caller who does not know pdfcer refuses to guess will assume it guessed.
    let h = stdout(&run(&["annotate", "--help"]));
    assert!(h.contains("--note-date"), "{h}");
    let long = stdout(&run(&["help", "annotate"]));
    assert!(
        long.contains("does not read a clock") || long.contains("You know what"),
        "the long help states the decision: {long}"
    );
}
