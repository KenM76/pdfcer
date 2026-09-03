//! # `pdfcer inspect --reflow-preview` integration tests
//!
//! Black-box tests over the **real binary** for the Pass 15.0 READ-ONLY
//! within-block reflow preview (decision 015 §6, slice 15.0). They assert
//! the process contract a script depends on: exit code `0`, the stable
//! summary line on stdout, the derived-layout disclosures on stderr (never
//! mistakable for sourced content, rule 4), and well-formed `--json`. They
//! pin the acceptance criteria on the committed synthetic corpus:
//!
//! - L/C/R/justified alignment inferred correctly on the aligned pages;
//! - a re-wrap at a narrowed width that overflows a small page discloses the
//!   overflow (COMPUTED, not applied — no write);
//! - the alignment override is honoured;
//! - an out-of-range block is refused (not ignored).
//!
//! Uses the committed `fixtures/synthetic/reflow/reflow.pdf` (provenance:
//! that directory's `PROVENANCE.md`) — five pages, one aligned paragraph
//! each, in standard-14 Courier (monospace, exact geometry).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/reflow")
        .join("reflow.pdf")
}

fn run(args: &[&str]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.arg("inspect").arg(fixture()).arg("--reflow-preview");
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

/// The summary line (first stdout line) for a given page.
fn summary(page: &str) -> String {
    let out = run(&["--pages", page]);
    assert!(out.status.success(), "exit 0 for page {page}");
    stdout(&out)
        .lines()
        .find(|l| l.starts_with("reflow-preview "))
        .expect("a summary line on stdout")
        .to_string()
}

#[test]
fn left_alignment_is_detected() {
    let s = summary("1");
    assert!(s.contains("align=left"), "{s}");
    assert!(s.contains("align_source=detected"), "{s}");
    assert!(s.contains("overflow=0"), "{s}");
}

#[test]
fn right_alignment_is_detected() {
    let s = summary("2");
    assert!(s.contains("align=right"), "{s}");
    assert!(s.contains("align_source=detected"), "{s}");
}

#[test]
fn center_alignment_is_detected() {
    let s = summary("3");
    assert!(s.contains("align=center"), "{s}");
    assert!(s.contains("align_source=detected"), "{s}");
}

#[test]
fn justified_alignment_is_detected() {
    let s = summary("4");
    assert!(s.contains("align=justified"), "{s}");
    assert!(s.contains("align_source=detected"), "{s}");
}

#[test]
fn narrowed_width_overflows_the_small_page_and_is_disclosed() {
    let out = run(&["--pages", "5", "--width", "12"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let summary = text
        .lines()
        .find(|l| l.starts_with("reflow-preview "))
        .expect("summary");
    // Re-wrap at 12pt makes many one-word lines that grow past the page.
    assert!(summary.contains("overflow=1"), "{summary}");
    assert!(summary.contains("lines_after=10"), "{summary}");
    // The overflow condition is disclosed on stderr, never silent (R76).
    let err = stderr(&out);
    assert!(
        err.contains("past the page bottom"),
        "overflow must be disclosed on stderr: {err:?}"
    );
    // And the report still lists ALL lines (off-page content is computed,
    // not clipped-to-invisible) — the last line has a negative baseline.
    assert!(
        text.contains("baseline=-86.0"),
        "off-page line present: {text}"
    );
}

#[test]
fn alignment_override_is_honoured() {
    // Force the left page (1) to justified; the source tag flips.
    let out = run(&["--pages", "1", "--align", "justified"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("align=justified"), "{s}");
    assert!(s.contains("align_source=overridden"), "{s}");
}

#[test]
fn a_bad_alignment_keyword_is_refused() {
    let out = run(&["--pages", "1", "--align", "sideways"]);
    assert!(!out.status.success(), "a bad --align must not exit 0");
}

#[test]
fn an_out_of_range_block_is_refused() {
    // Each page has exactly one block (index 0); asking for block 9 is a
    // mistake a batch script must hear about (the R27 fail-clean posture).
    let out = run(&["--pages", "1", "--block", "9"]);
    assert!(!out.status.success(), "out-of-range block must not exit 0");
    assert!(
        stderr(&out).contains("out of range"),
        "the refusal names the cause: {:?}",
        stderr(&out)
    );
}

#[test]
fn derived_layout_is_disclosed_on_stderr() {
    // Rule 4: the derived layout must be visible and can never be confused
    // with sourced content, so the disclosures go to stderr.
    let out = run(&["--pages", "1"]);
    let err = stderr(&out);
    assert!(
        err.contains("DERIVED layout"),
        "the derived-layout disclosure must be on stderr: {err:?}"
    );
    assert!(
        err.contains("READ-ONLY"),
        "the read-only disclosure must be on stderr: {err:?}"
    );
}

#[test]
fn json_mode_is_well_formed_and_carries_the_preview() {
    let out = run(&["--pages", "4", "--json"]);
    assert!(out.status.success());
    let json = stdout(&out);
    // Structural sanity without a JSON dependency.
    assert!(json.trim_start().starts_with('{'));
    assert!(json.contains("\"alignment\""));
    assert!(json.contains("\"value\": \"justified\""));
    assert!(json.contains("\"lines\""));
    assert!(json.contains("\"justified_slack\""));
    assert!(json.contains("\"new_bbox\""));
    assert!(json.contains("\"disclosures\""));
    // In --json to stdout, the summary line moves to stderr so stdout is
    // clean JSON.
    assert!(stderr(&out).contains("reflow-preview "));
}

#[test]
fn a_page_out_of_range_is_refused() {
    // The fixture has five pages; asking for page 9 is a mistake.
    let out = run(&["--pages", "9"]);
    assert!(
        !out.status.success(),
        "an out-of-range page must not exit 0"
    );
}
