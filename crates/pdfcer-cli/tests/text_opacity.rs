//! `G222` — `format-text --set-opacity` and `add-text --opacity` write a
//! fill-opacity graphics state (`/ca`, ISO 32000-1 §8.4.5) over the real
//! binary, and refuse an opacity outside 0 to 1.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(dir)
        .join(name)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_opacity_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

/// Run `args` writing to a temp file; the stdout and the output file's bytes.
fn written(tag: &str, args: &[&str]) -> (String, String) {
    let out_path = temp_path(tag);
    let mut all = args.to_vec();
    all.extend(["-o", out_path.to_str().unwrap()]);
    let out = run(&all);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = String::from_utf8_lossy(&std::fs::read(&out_path).unwrap()).into_owned();
    let _ = std::fs::remove_file(out_path);
    (String::from_utf8_lossy(&out.stdout).into_owned(), bytes)
}

#[test]
fn format_text_set_opacity_fades_the_run_and_reports_it() {
    let input = fixture("textedit", "format_color.pdf");
    let (report, bytes) = written(
        "fmt",
        &[
            "format-text",
            input.to_str().unwrap(),
            "--find",
            "hello",
            "--set-opacity",
            "0.5",
        ],
    );
    assert!(report.contains("opacity=1->0.5"), "{report}");
    assert!(
        bytes.contains("/pdfcerTA1 gs") && bytes.contains("/pdfcerTA2 gs"),
        "{bytes}"
    );
    assert!(bytes.contains("/ca 0.5"), "the faded state is bound");
}

#[test]
fn add_text_opacity_writes_a_graphics_state_inside_the_run() {
    let input = fixture("addtext", "plain.pdf");
    let (_, bytes) = written(
        "add",
        &[
            "add-text",
            input.to_str().unwrap(),
            "--at",
            "72,500",
            "--text",
            "DRAFT",
            "--opacity",
            "0.25",
        ],
    );
    assert!(bytes.contains("q\n/pdfcerTA1 gs"), "inside the run's own q");
    assert!(bytes.contains("/ca 0.25"), "the state is bound");
}

#[test]
fn an_opacity_outside_zero_to_one_is_refused_by_both_verbs() {
    let out_path = temp_path("bad");
    let o = out_path.to_str().unwrap();
    let fmt_in = fixture("textedit", "format_color.pdf");
    let add_in = fixture("addtext", "plain.pdf");
    for args in [
        vec![
            "format-text",
            fmt_in.to_str().unwrap(),
            "--find",
            "hello",
            "--set-opacity",
            "1.5",
            "-o",
            o,
        ],
        vec![
            "add-text",
            add_in.to_str().unwrap(),
            "--at",
            "72,500",
            "--text",
            "X",
            "--opacity",
            "2",
            "-o",
            o,
        ],
    ] {
        let out = run(&args);
        assert!(!out.status.success(), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("invalid opacity"), "{err}");
        assert!(!out_path.exists(), "no output on a refusal");
    }
}
