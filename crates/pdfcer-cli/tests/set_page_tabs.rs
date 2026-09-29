//! `set-page-tabs`: writes `/Tabs`, reports the previous value, and refuses
//! a PDF 2.0 value in an older file.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-set-page-tabs-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

/// A PDF 1.7 page with no `/Tabs`.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

#[test]
fn structure_order_is_written_and_the_old_value_reported() {
    let out = temp_out("s.pdf");
    let (code, stdout, stderr) = run(&[
        "set-page-tabs",
        s(&fixture()),
        "--tabs",
        "S",
        "-o",
        s(&out),
        "--verify-undo",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("tabs=S was=none"), "{stdout}");
    let bytes = std::fs::read(&out).expect("output written");
    assert!(
        bytes.windows(8).any(|w| w == b"/Tabs /S"),
        "the saved page states /Tabs /S"
    );

    // Setting it again is a no-op the operator is told about.
    let again = temp_out("s-again.pdf");
    let (code, stdout, stderr) = run(&["set-page-tabs", s(&out), "--tabs", "S", "-o", s(&again)]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("was=S"), "{stdout}");
    assert!(stderr.contains("nothing changed"), "{stderr}");
}

#[test]
fn array_order_is_refused_in_a_pdf_17_file() {
    let out = temp_out("a.pdf");
    let (code, _, stderr) = run(&["set-page-tabs", s(&fixture()), "--tabs", "A", "-o", s(&out)]);
    assert_ne!(code, 0);
    assert!(stderr.contains("PDF 2.0"), "{stderr}");
    assert!(!out.exists(), "a refusal writes nothing");
}
