//! `import-structure` refuses an export taken from a different state of its
//! input, because compiling it would revert every change made since, unless
//! `--allow-stale-base` is passed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a renumbering fails here.
const EDIT_REFUSED: i32 = 9;

fn plain() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_impstale_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("spawn pdfcer")
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// An edited export of `plain.pdf`, compiled into `input` with `extra` flags.
fn import_into(input: &Path, extra: &[&str]) -> (Output, PathBuf) {
    let exported = temp_path("export");
    let out = run(&["export-structure", s(&plain()), "-o", s(&exported)]);
    assert!(out.status.success(), "export failed: {out:?}");
    let bytes = std::fs::read(&exported).unwrap();
    let edited = String::from_utf8_lossy(&bytes)
        .replacen("page text", "PAGE TEXT", 1)
        .into_bytes();
    assert_ne!(bytes, edited, "the fixture text the edit targets is gone");
    std::fs::write(&exported, &edited).unwrap();

    let output = temp_path("out");
    let _ = std::fs::remove_file(&output);
    let mut args = vec![
        "import-structure",
        s(input),
        "--edited",
        s(&exported),
        "-o",
        s(&output),
    ];
    args.extend_from_slice(extra);
    let out = run(&args);
    let _ = std::fs::remove_file(&exported);
    (out, output)
}

/// `plain.pdf` rotated: a later state than the one the export is taken from.
fn later_state() -> PathBuf {
    let rotated = temp_path("rotated");
    let out = run(&["rotate", s(&plain()), "--degrees", "90", "-o", s(&rotated)]);
    assert!(out.status.success(), "rotate failed: {out:?}");
    rotated
}

#[test]
fn a_current_export_imports_and_reports_a_matching_base() {
    let (out, output) = import_into(&plain(), &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("base=matches"), "{stdout}");
    let _ = std::fs::remove_file(&output);
}

#[test]
fn a_stale_export_is_refused_and_writes_nothing() {
    let input = later_state();
    let (out, output) = import_into(&input, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "stderr: {stderr}");
    assert!(stderr.contains("--allow-stale-base"), "{stderr}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("base=differs"));
    assert!(
        !output.exists(),
        "a refused import wrote {}",
        output.display()
    );
    let _ = std::fs::remove_file(&input);
}

#[test]
fn allow_stale_base_compiles_a_stale_export_with_a_warning() {
    let input = later_state();
    let (out, output) = import_into(&input, &["--allow-stale-base"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(output.exists());
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_file(&input);
}
