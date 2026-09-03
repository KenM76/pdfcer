//! # `pdfcer edit-text` integration tests (Pass 14.1)
//!
//! Black-box tests over the **real binary** for in-place text editing. They
//! assert the process contract a script depends on — exit codes, the stable
//! report lines, the verbatim disclosures — across the five committed
//! `fixtures/synthetic/textedit/` fixtures (provenance: that directory's
//! `PROVENANCE.md`). Each acceptance clause of decision 014 §5.2's "13.1"
//! slice has a test here:
//!
//! - an embedded-full run and a non-embedded run edit correctly, and the
//!   incremental output keeps every untouched object byte-identical (the
//!   original file is a byte-prefix of the output);
//! - a subset-missing keystroke is REFUSED by name (exit `EDIT_REFUSED`);
//! - a supplied `--font-dir` face lifts a non-embedded run to `Supplied`;
//! - a tagged run keeps its `/MCID` wrapper and discloses staleness;
//! - an absolute-`Tm` follower is repositioned by the advance delta.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/textedit")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

/// A unique temp path so parallel tests never collide.
fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_edit_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("edit-text")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn nonembedded_edit_succeeds_and_is_disclosed_bundled() {
    let out_path = temp_path("ne");
    let out = run(&[
        fixture("nonembedded.pdf").to_str().unwrap(),
        "--find",
        "teh",
        "--replace",
        "the",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("glyph_source=Bundled"), "{text}");
    assert!(text.contains("base_font=Calibri"), "{text}");
    // The incremental save keeps every untouched object byte-identical: the
    // original file is a byte-prefix of the output.
    let orig = std::fs::read(fixture("nonembedded.pdf")).unwrap();
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        edited.starts_with(&orig),
        "output must be an incremental append"
    );
    assert!(edited.len() > orig.len());
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn embedded_full_edit_only_changes_the_edited_stream() {
    let out_path = temp_path("ef");
    let out = run(&[
        fixture("embedded_full.pdf").to_str().unwrap(),
        "--find",
        "teh",
        "--replace",
        "the",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("glyph_source=Embedded"), "{text}");
    let orig = std::fs::read(fixture("embedded_full.pdf")).unwrap();
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        edited.starts_with(&orig),
        "untouched objects stay byte-identical"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn subset_missing_keystroke_is_refused_by_name() {
    let out_path = temp_path("sm");
    let out = run(&[
        fixture("subset_missing.pdf").to_str().unwrap(),
        "--find",
        "cat",
        "--replace",
        "caz",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "refusal is exit 9");
    let err = stderr(&out);
    assert!(err.contains("R-INV-1"), "named refusal: {err}");
    assert!(err.contains("embedded-subset floor"), "{err}");
    // No output is written on a refusal.
    assert!(!out_path.exists(), "a refused edit writes nothing");
}

#[test]
fn supplied_font_dir_lifts_a_nonembedded_run_to_supplied() {
    // Copy the in-repo Foxit CFF into a temp dir as `Calibri.cff` so
    // --font-dir registers a `Calibri` face (decision 012).
    // Per-call-unique temp dir (pid = binary-safe, atomic `N` = thread-safe) so
    // no two concurrent tests share this scratch dir. See `temp_path` above.
    let font_dir = {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("pdfcer_fd_{}_{n}", std::process::id()))
    };
    std::fs::create_dir_all(&font_dir).unwrap();
    let cff =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../pdfcer-render/assets/fonts/FoxitSans.cff");
    std::fs::copy(&cff, font_dir.join("Calibri.cff")).unwrap();

    let out_path = temp_path("sup");
    let out = run(&[
        fixture("nonembedded.pdf").to_str().unwrap(),
        "--find",
        "teh",
        "--replace",
        "the",
        "--font-dir",
        font_dir.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(
        stdout(&out).contains("glyph_source=Supplied"),
        "{}",
        stdout(&out)
    );
    let _ = std::fs::remove_file(out_path);
    let _ = std::fs::remove_dir_all(font_dir);
}

#[test]
fn tagged_run_keeps_mcid_and_discloses_staleness() {
    let out_path = temp_path("tg");
    let out = run(&[
        fixture("tagged.pdf").to_str().unwrap(),
        "--find",
        "teh",
        "--replace",
        "the",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("tagged_mcid=0"), "{text}");
    assert!(
        text.contains("BDC/EMC+MCID wrapper was PRESERVED"),
        "{text}"
    );
    // The output still carries the marked-content id.
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&edited).contains("/MCID 0"),
        "the MCID wrapper must survive"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn tm_follower_is_repositioned_by_the_advance_delta() {
    let out_path = temp_path("tm");
    let out = run(&[
        fixture("tm_follower.pdf").to_str().unwrap(),
        "--find",
        "Hello",
        "--replace",
        "Hi",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("followers_repositioned=1"), "{text}");
    assert!(
        text.contains("advance_delta=-"),
        "shorter run ⇒ negative ΔA: {text}"
    );
    let _ = std::fs::remove_file(out_path);
}
