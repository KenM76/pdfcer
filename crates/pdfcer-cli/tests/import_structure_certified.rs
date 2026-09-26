//! `import-structure` applies the certification refusal every `EditSession`
//! edit applies: an enforced DocMDP certification (ISO 32000-1 §12.8.4)
//! forbids changes, so a non-empty import exits `EDIT_REFUSED` and writes
//! nothing. The same edit on an uncertified file is the control.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a renumbering fails here.
const EDIT_REFUSED: i32 = 9;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_impstruct_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("spawn pdfcer")
}

/// Exports `input`, changes one content-stream string, and imports it back.
fn import_edited(input: &Path) -> (Output, PathBuf) {
    let exported = temp_path("export");
    let out = run(&[
        "export-structure",
        input.to_str().unwrap(),
        "-o",
        exported.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "export failed: {out:?}");
    let bytes = std::fs::read(&exported).unwrap();
    let edited: Vec<u8> = String::from_utf8_lossy(&bytes)
        .replacen("page text", "PAGE TEXT", 1)
        .into_bytes();
    assert_ne!(bytes, edited, "the fixture text the edit targets is gone");
    std::fs::write(&exported, &edited).unwrap();

    let output = temp_path("out");
    let _ = std::fs::remove_file(&output);
    let out = run(&[
        "import-structure",
        input.to_str().unwrap(),
        "--edited",
        exported.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(&exported);
    (out, output)
}

#[test]
fn a_certified_document_refuses_the_import_and_writes_nothing() {
    let (out, output) = import_edited(&fixture("addtext/certified-locked.pdf"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "stderr: {stderr}");
    assert!(stderr.contains("certification signature"), "{stderr}");
    assert!(stderr.contains("12.8.4"), "{stderr}");
    assert!(
        !output.exists(),
        "a refused import wrote {}",
        output.display()
    );
}

#[test]
fn the_same_edit_on_an_uncertified_document_imports() {
    let (out, output) = import_edited(&fixture("addtext/plain.pdf"));
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(output.exists());
    let _ = std::fs::remove_file(&output);
}
