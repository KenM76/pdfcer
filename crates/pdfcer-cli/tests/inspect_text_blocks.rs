//! # `pdfcer inspect --text-blocks` integration tests
//!
//! Black-box tests over the **real binary** for the Pass 14.0 read-only
//! block-recognition dump. They assert the process contract a script
//! depends on: exit code `0`, the stable summary line on stdout, the
//! derived-structure disclosures on stderr (never mistakable for sourced
//! content), and well-formed `--json`. They also pin that plain `inspect`
//! (no `--text-blocks`) is **unchanged** — the version-line contract must
//! not regress.
//!
//! Uses the committed `fixtures/synthetic/textblocks/multi-column.pdf`
//! (provenance: that directory's `PROVENANCE.md`) — a two-column,
//! four-paragraph, ten-line synthetic page.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textblocks")
        .join("multi-column.pdf")
}

fn run(args: &[&str]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.arg("inspect").arg(fixture());
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn plain_inspect_still_prints_the_version_line() {
    // The pre-existing contract must not regress: without --text-blocks,
    // inspect prints exactly the stable `path: PDF M.N` line and exits 0.
    let out = run(&[]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains(": PDF 1.7"),
        "plain inspect must still report the version: {text:?}"
    );
    assert!(
        !text.contains("text-blocks"),
        "plain inspect must not emit the block report"
    );
}

#[test]
fn text_blocks_summary_line_reports_the_recognised_counts() {
    let out = run(&["--text-blocks"]);
    assert!(out.status.success(), "exit 0");
    let text = stdout(&out);
    let summary = text
        .lines()
        .find(|l| l.starts_with("text-blocks "))
        .expect("a summary line on stdout");
    // The multi-column fixture: 1 page, 10 lines, 4 blocks, 2 columns.
    assert!(summary.contains("pages=1"), "{summary}");
    assert!(summary.contains("lines=10"), "{summary}");
    assert!(summary.contains("blocks=4"), "{summary}");
    assert!(summary.contains("columns_max=2"), "{summary}");
    assert!(summary.contains("multi_column_pages=1"), "{summary}");
    // The per-block report lists all four paragraphs.
    assert_eq!(
        text.matches("kind=paragraph").count(),
        4,
        "four paragraph blocks in the report"
    );
    assert!(text.contains("Left column paragraph one"));
    assert!(text.contains("Right column paragraph two"));
}

#[test]
fn derived_structure_is_disclosed_on_stderr() {
    // Rule 4: the guessing must be visible and can never be confused with
    // sourced content, so the disclosures go to stderr.
    let out = run(&["--text-blocks"]);
    let err = stderr(&out);
    assert!(
        err.contains("DERIVED"),
        "the derived-structure disclosure must be on stderr: {err:?}"
    );
    assert!(
        err.contains("column bands"),
        "the multi-column reading-order disclosure must be on stderr: {err:?}"
    );
}

#[test]
fn json_mode_is_well_formed_and_carries_provenance() {
    let out = run(&["--text-blocks", "--json"]);
    assert!(out.status.success());
    let json = stdout(&out);
    // Structural sanity without a JSON dependency: the keys the schema
    // promises are present, and the substrate fields are there.
    assert!(json.trim_start().starts_with('{'));
    assert!(json.contains("\"blocks\""));
    assert!(json.contains("\"provenance\""));
    assert!(json.contains("\"operator_span\""));
    assert!(json.contains("\"font_resource\": \"F1\""));
    // The blue paragraph's fill colour is disclosed; the reset-to-black
    // paragraphs read as an explicit gray.
    assert!(json.contains("\"fill_color\": \"rgb:0.000,0.000,1.000\""));
    // In --json to stdout, the summary line moves to stderr so stdout is
    // clean JSON.
    assert!(stderr(&out).contains("text-blocks "));
}

#[test]
fn a_page_out_of_range_is_refused_not_ignored() {
    // The fixture has one page; asking for page 5 is a mistake a batch
    // script must hear about (the R27 fail-clean posture).
    let out = run(&["--text-blocks", "--pages", "5"]);
    assert!(
        !out.status.success(),
        "an out-of-range page must not exit 0"
    );
}
