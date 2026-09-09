//! # `pdfcer run-repertoire` (`Pass 280.0`)
//!
//! Black-box over the **real binary**.
//!
//! ## Why a CLI suite when the core one drives the whole contract
//!
//! `crates/pdfcer-core/tests/run_repertoire.rs` proves the equivalence
//! (accepted ⇒ `edit_text` accepts) over the entire repertoire. It cannot see
//! a flag the dispatch drops, an argument the boundary refuses differently
//! from core, or an output line that is ambiguous to a script — and those are
//! exactly what a scriptable verb is for.
//!
//! The measurement here is therefore the **loop a script would run**: ask the
//! repertoire, then attempt an edit with a character it named and one it did
//! not, and require the two answers to agree with it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_rrep_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const SUBSET: &str = "textedit/subset_missing.pdf";

fn repertoire(rel: &str, extra: &[&str]) -> Output {
    let mut args: Vec<String> = vec![
        "run-repertoire".to_owned(),
        fixture(rel).to_str().unwrap().to_owned(),
        "--find".to_owned(),
        "cat".to_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs)
}

fn field(line: &str, key: &str) -> String {
    line.split_whitespace()
        .find_map(|t| t.strip_prefix(key))
        .unwrap_or_else(|| panic!("no {key} in {line}"))
        .to_owned()
}

/// ★★★ THE SCRIPT'S LOOP: what the line says, the editor honours.
#[test]
fn a_character_the_line_names_edits_and_one_it_omits_does_not() {
    let out = repertoire(SUBSET, &["--list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let line = stdout(&out);
    let listed = field(&line, "accepted_chars=");
    let points: Vec<u32> = listed
        .split(',')
        .map(|t| u32::from_str_radix(t.trim_start_matches("U+"), 16).expect("a code point"))
        .collect();
    assert!(points.len() >= 3, "{line}");

    // A character the line named.
    let named = char::from_u32(points[0]).expect("a scalar");
    let ok = run(&[
        "edit-text",
        fixture(SUBSET).to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "cat",
        "--replace",
        &named.to_string().repeat(3),
        "-o",
        temp_path("named").to_str().unwrap(),
    ]);
    assert!(
        ok.status.success(),
        "run-repertoire named U+{:04X} and edit-text refused it: {}",
        points[0],
        stderr(&ok)
    );

    // And one it did not: 'd', which this subset does not carry.
    assert!(
        !points.contains(&u32::from(b'd')),
        "the fixture's subset must not carry 'd', or this half proves nothing: {line}"
    );
    let refused = run(&[
        "edit-text",
        fixture(SUBSET).to_str().unwrap(),
        "--page",
        "1",
        "--find",
        "cat",
        "--replace",
        "ddd",
        "-o",
        temp_path("unnamed").to_str().unwrap(),
    ]);
    assert_eq!(
        refused.status.code(),
        Some(EDIT_REFUSED),
        "run-repertoire omitted 'd' and edit-text took it"
    );
}

/// ★ The listed set is CODE POINTS, and that is not cosmetic.
///
/// The first cut printed the characters themselves through `sanitize_token`,
/// which maps a space to `_` — so a repertoire containing a space and one
/// containing an underscore printed **identically**, and a set containing a
/// comma or a quote would have been worse. This fixture's set contains a
/// space, which is what makes the test able to see the difference.
#[test]
fn the_listed_set_is_unambiguous_code_points() {
    let line = stdout(&repertoire(SUBSET, &["--list"]));
    let listed = field(&line, "accepted_chars=");
    assert!(
        listed.split(',').all(|t| t.starts_with("U+")),
        "every entry must be a code point: {listed}"
    );
    assert!(
        listed.contains("U+0020"),
        "this run accepts a SPACE, and a raw-character rendering could not say so \
         distinguishably: {listed}"
    );
}

/// The subset disclosure fires, and names the remedy rather than the deferral.
#[test]
fn an_embedded_subset_is_disclosed_with_its_remedy() {
    let out = repertoire(SUBSET, &[]);
    let msg = stderr(&out);
    assert!(msg.contains("embedded SUBSET"), "{msg}");
    assert!(
        msg.contains("set-font"),
        "a disclosure that names no route is the defect Pass 274.0 closed: {msg}"
    );
}

/// Without `--list` the line still carries what a script branches on.
#[test]
fn the_summary_line_carries_the_counts() {
    let line = stdout(&repertoire(SUBSET, &[]));
    assert_eq!(
        field(&line, "run="),
        "cat",
        "the line names the run it answered about"
    );
    assert_eq!(field(&line, "editable="), "1");
    assert_eq!(field(&line, "subset="), "1");
    assert!(!line.contains("accepted_chars="), "--list was not passed");
    let accepted: usize = field(&line, "accepted=").parse().expect("a count");
    let tested: usize = field(&line, "tested=").parse().expect("a count");
    let refused: usize = field(&line, "refused=").parse().expect("a count");
    assert_eq!(
        accepted + refused,
        tested,
        "the three counts must close: {line}"
    );
}

/// An unpinned empty `--find` is refused at the BOUNDARY, naming the flag.
///
/// Core would answer about whichever run it located first — an answer to a
/// question nobody asked. Same refusal `font-preflight` applies, and it names
/// the argument rather than the document.
#[test]
fn an_unpinned_empty_find_is_told_about_the_flag() {
    let out = run(&["run-repertoire", fixture(SUBSET).to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let msg = stderr(&out);
    assert!(
        msg.contains("--find") && msg.contains("--pin-span"),
        "name both routes to a located run: {msg}"
    );
}
