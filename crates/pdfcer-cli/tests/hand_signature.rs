//! `--hand-signature` on `annotate --as-content`, `add-text` and `add-image`,
//! read back by `list-hand-signatures`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;
const USAGE: i32 = 2;

fn synthetic(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_hand_sig_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn ok(o: &Output) -> String {
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Run `verb src … --output <new>` and return the new file.
fn step(verb: &str, src: &Path, extra: &[&str], tag: &str) -> PathBuf {
    let out = temp_path(tag);
    let mut args = vec![verb, src.to_str().unwrap()];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap()]);
    ok(&run(&args));
    out
}

/// The three verbs each mark their output for a field, and
/// `list-hand-signatures` reports all three with their bounds.
#[test]
fn three_verbs_mark_and_the_listing_reads_them_back() {
    let a = step(
        "annotate",
        &synthetic("hello.pdf"),
        &[
            "--type",
            "square",
            "--page",
            "1",
            "--rect",
            "20,15,120,55",
            "--as-content",
            "--hand-signature",
            "Shape",
        ],
        "annotate",
    );
    let b = step(
        "add-text",
        &a,
        &[
            "--at",
            "30,100",
            "--text",
            "J. Smith",
            "--hand-signature",
            "Typed",
        ],
        "text",
    );
    let image = synthetic("images/rgb8.png");
    let c = step(
        "add-image",
        &b,
        &[
            "--image",
            image.to_str().unwrap(),
            "--page",
            "1",
            "--rect",
            "150,20,250,70",
            "--hand-signature",
            "Drawn",
        ],
        "image",
    );
    let listed = ok(&run(&["list-hand-signatures", c.to_str().unwrap()]));
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 4, "{listed}");
    assert!(
        lines[0].starts_with("page=1 field=\"Shape\" bounds="),
        "{listed}"
    );
    assert!(
        lines[1].starts_with("page=1 field=\"Typed\" bounds="),
        "{listed}"
    );
    // The image keeps its aspect ratio inside `--rect`; the bounds are where
    // it paints, not the rectangle asked for.
    assert_eq!(
        lines[2], "page=1 field=\"Drawn\" bounds=162.500,20.000,237.500,70.000",
        "{listed}"
    );
    assert_eq!(lines[3], "total=3");
    let page1 = ok(&run(&[
        "list-hand-signatures",
        c.to_str().unwrap(),
        "--page",
        "1",
    ]));
    assert_eq!(page1, listed);
    for p in [a, b, c] {
        std::fs::remove_file(p).ok();
    }
}

/// An unmarked file lists nothing; a page outside it is refused.
#[test]
fn an_unmarked_file_and_a_bad_page() {
    let src = synthetic("hello.pdf");
    let listed = ok(&run(&["list-hand-signatures", src.to_str().unwrap()]));
    assert_eq!(listed, "total=0\n");
    let o = run(&["list-hand-signatures", src.to_str().unwrap(), "--page", "9"]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(String::from_utf8_lossy(&o.stderr).contains("out of range"));
}

/// A hand signature is page content: `annotate` without `--as-content`
/// rejects the flag, and an empty field name is refused with nothing written.
#[test]
fn refusals() {
    let src = synthetic("hello.pdf");
    let out = temp_path("refused");
    let o = run(&[
        "annotate",
        src.to_str().unwrap(),
        "--type",
        "square",
        "--page",
        "1",
        "--rect",
        "20,15,120,55",
        "--hand-signature",
        "Shape",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(USAGE));
    let o = run(&[
        "add-text",
        src.to_str().unwrap(),
        "--at",
        "30,100",
        "--text",
        "x",
        "--hand-signature",
        "",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        o.status.code(),
        Some(EDIT_REFUSED),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!out.exists());
}
