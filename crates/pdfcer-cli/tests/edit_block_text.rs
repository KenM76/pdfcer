//! `pdfcer edit-block-text` over the real binary: the report is printed, the
//! save is incremental, and an unencodable character refuses by name with no
//! output written.
//!
//! Fixture: `fixtures/synthetic/reflow/block_text.pdf` (provenance beside it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/reflow/block_text.pdf")
}

/// Run with `extra`, writing to a per-call temp path.
fn run(extra: &[&str]) -> (Output, PathBuf) {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let out_path =
        std::env::temp_dir().join(format!("pdfcer_block_text_{}_{n}.pdf", std::process::id()));
    let _ = std::fs::remove_file(&out_path);
    let output = Command::new(BIN)
        .arg("edit-block-text")
        .arg(fixture())
        .args(extra)
        .arg("--output")
        .arg(&out_path)
        .output()
        .expect("the binary runs");
    (output, out_path)
}

#[test]
fn a_point_picks_the_block_and_the_report_is_printed() {
    let text = "Synthetic placeholder words now make up a much longer first paragraph, \
                so the edit must wrap the new text onto more lines than before.";
    let (out, path) = run(&["--at", "100,701", "--text", text]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("block: 0 (at 100,701)"), "{stdout}");
    assert!(
        stdout.contains("old text: \"Synthetic placeholder words make up"),
        "{stdout}"
    );
    assert!(stdout.contains("lines: 3 -> 4"), "{stdout}");
    assert!(stdout.lines().any(|l| l == "looks: 1"), "{stdout}");
    assert!(stdout.contains("overflow: 14.00 pt"), "{stdout}");
    assert!(stdout.contains("font: Helvetica"), "{stdout}");
    assert!(
        stdout.contains("note: block 0 replaced and wrapped"),
        "{stdout}"
    );
    let src = std::fs::read(fixture()).unwrap();
    let saved = std::fs::read(&path).unwrap();
    assert!(saved.starts_with(&src), "incremental save");
    let _ = std::fs::remove_file(path);
}

#[test]
fn an_unencodable_character_refuses_by_name_and_writes_nothing() {
    let (out, path) = run(&["--block", "0", "--text", "Omega \u{3A9} and \u{3042}"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(9), "EDIT_REFUSED: {stderr}");
    assert!(
        stderr.contains("U+03A9") && stderr.contains("U+3042"),
        "{stderr}"
    );
    assert!(!path.exists(), "no output on refusal");
}
