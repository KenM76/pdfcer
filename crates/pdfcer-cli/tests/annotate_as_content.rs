//! `pdfcer annotate --as-content`: a Review shape drawn into the page's own
//! content instead of as a comment.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_as_content_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn annotate(kind: &str, extra: &[&str], tag: &str) -> (Output, PathBuf) {
    let out = temp_path(tag);
    let src = fixture();
    let mut args = vec![
        "annotate",
        src.to_str().unwrap(),
        "--type",
        kind,
        "--page",
        "1",
        "--rect",
        "20,15,120,55",
        "--as-content",
    ];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap()]);
    (run(&args), out)
}

/// The shape lands as page objects, reported by index range, and the file
/// gains no annotation.
#[test]
fn a_square_is_drawn_into_the_page_and_not_as_a_comment() {
    let (o, out) = annotate("square", &["--opacity", "0.5"], "square");
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let range = stdout
        .split("objects=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("no object range in {stdout}"));
    let (start, end) = range.split_once("..").unwrap();
    assert!(end.parse::<usize>().unwrap() > start.parse::<usize>().unwrap());
    assert!(stdout.contains("resources_added=1"), "{stdout}");

    let listed = run(&["list-annotations", out.to_str().unwrap()]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("annots=0"),
        "an annotation was authored"
    );
    std::fs::remove_file(out).ok();
}

/// A text-bearing type has no geometric shape to draw; refused by name.
#[test]
fn a_stamp_is_refused() {
    let (o, out) = annotate("stamp", &[], "stamp");
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--as-content"));
    assert!(!out.exists());
}

/// A note has nowhere to go in page content, so the flags conflict.
#[test]
fn a_note_cannot_be_combined_with_it() {
    let (o, out) = annotate("square", &["--note", "hi"], "note");
    assert_eq!(o.status.code(), Some(2), "clap usage error expected");
    assert!(!out.exists());
}
