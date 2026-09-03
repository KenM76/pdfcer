//! # `pdfcer reflow` integration tests
//!
//! Black-box tests over the **real binary** for the Pass 15.1 within-block
//! reflow SURGERY (decision 015 §6, slice 15.1). They assert the process
//! contract a script depends on and pin the acceptance criteria on the
//! committed synthetic corpus:
//!
//! - a re-wrap writes an output whose original file is a byte-PREFIX
//!   (incremental save, R34) and which re-loads + re-renders faithfully
//!   (R59 render gate on reflowed output);
//! - a JUSTIFIED re-wrap distributes slack and is reported;
//! - a re-wrap at a narrowed width past a small page bottom DISCLOSES the
//!   overflow AND emits the content (never clipped, R76);
//! - a bad block index / bad alignment keyword is a clean, named non-zero
//!   exit (never a crash).
//!
//! Uses the committed `fixtures/synthetic/reflow/reflow.pdf` (provenance:
//! that directory's `PROVENANCE.md`) — five pages, one aligned paragraph
//! each, standard-14 Courier (monospace, exact geometry). Page 4 is the
//! justified paragraph; page 5 is the tiny overflow page.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/reflow")
        .join("reflow.pdf")
}

/// Run `reflow` with the given extra args, writing to a unique temp output;
/// returns the process output and the output path.
fn run_reflow(extra: &[&str], tag: &str) -> (Output, PathBuf) {
    // Globally-unique-per-call temp path: `process::id()` disambiguates across
    // parallel test *binaries*, and the process-global atomic `N` disambiguates
    // across parallel test *threads* within this binary. Without the counter,
    // two calls sharing a `tag` (or a concurrent second `cargo test` run) would
    // collide on the same path; a half-written file re-opened by another test
    // then loads via xref RECOVERY, and the next incremental op is refused with
    // `RecoveredBaseForbidsIncremental` — a flake that only surfaces under a
    // full parallel `cargo test --workspace`. See the sibling `temp_path`
    // helpers in add_text.rs / edit_text.rs / format_text.rs for the same guard.
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let out_path = std::env::temp_dir().join(format!(
        "pdfcer_reflow_{tag}_{}_{n}.pdf",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&out_path);
    let mut cmd = Command::new(BIN);
    cmd.arg("reflow").arg(fixture());
    for a in extra {
        cmd.arg(a);
    }
    cmd.arg("--output").arg(&out_path);
    let output = cmd.output().expect("the binary runs");
    (output, out_path)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn rewrap_is_incremental_and_reloads() {
    // Page 1 (left paragraph): re-wrap to a wide box. The output's original
    // bytes are a prefix (incremental save), and it re-loads.
    let src = std::fs::read(fixture()).unwrap();
    let (out, out_path) = run_reflow(&["--page", "1", "--block", "0", "--width", "400"], "incr");
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let bytes = std::fs::read(&out_path).unwrap();
    assert_eq!(
        bytes.get(..src.len()),
        Some(src.as_slice()),
        "incremental prefix"
    );
    assert!(
        bytes.len() > src.len(),
        "an incremental revision was appended"
    );
    // The summary reports the block re-wrapped and only its content object.
    let s = stdout(&out);
    assert!(s.contains("page=1 block=0"), "{s}");
    assert!(s.contains("align=left"), "{s}");
    // The incremental/prior-text disclosure is surfaced verbatim.
    assert!(s.contains("INCREMENTALLY"), "{s}");
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn justified_rewrap_distributes_slack_and_is_reported() {
    // Page 4 is the justified paragraph. Re-wrap it justified; at least one
    // full line is justified (last line un-stretched).
    let (out, out_path) = run_reflow(
        &[
            "--page",
            "4",
            "--block",
            "0",
            "--align",
            "justified",
            "--width",
            "200",
        ],
        "just",
    );
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("align=justified"), "{s}");
    assert!(
        s.contains("justified_lines=") && !s.contains("justified_lines=0"),
        "at least one line justified: {s}"
    );
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn narrow_overflow_is_disclosed_and_emitted_not_clipped() {
    // Page 5 is the tiny overflow page; a narrow width grows it off the page
    // bottom. The overflow is DISCLOSED and the content EMITTED (R76).
    let (out, out_path) = run_reflow(&["--page", "5", "--block", "0", "--width", "12"], "ovf");
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let s = stdout(&out);
    assert!(
        s.contains("overflow: past_bottom="),
        "overflow reported: {s}"
    );
    assert!(
        s.contains("EMITTED off-page, not clipped"),
        "emitted-not-clipped: {s}"
    );
    // The output still loads (the off-page content is real, recoverable).
    let bytes = std::fs::read(&out_path).unwrap();
    assert!(bytes.len() > 100);
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn bad_block_index_is_a_clean_refusal() {
    // Block 9 does not exist ⇒ a named non-zero exit (exit::EDIT_REFUSED = 9),
    // never a crash, and no output written.
    let (out, out_path) = run_reflow(&["--page", "1", "--block", "9"], "badblk");
    assert!(!out.status.success(), "non-zero exit");
    assert_eq!(out.status.code(), Some(9), "EDIT_REFUSED");
    assert!(!out_path.exists(), "no output on refusal");
}

#[test]
fn bad_alignment_keyword_is_refused_before_any_work() {
    let (out, _out_path) = run_reflow(&["--page", "1", "--align", "sideways"], "badalign");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(9));
    assert!(stderr(&out).contains("expected left|right|center|justified"));
}
