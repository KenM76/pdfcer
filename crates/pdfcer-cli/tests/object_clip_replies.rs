//! `pdfcer object-copy` / `object-paste`: a copied reply's lost thread is
//! counted and disclosed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_path(tag: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pdfcer_clip_replies_{tag}_{}.{ext}",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Reply to the fixture's Circle, copy only the reply, preview its paste:
/// the report says one reply link was not carried.
#[test]
fn a_pasted_reply_reports_its_unthreaded_link() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/demo-annotated.pdf");
    let replied = temp_path("replied", "pdf");
    let clip = temp_path("clip", "pdfceclip");
    assert_ok(&run(&[
        "add-reply",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "2",
        "--note",
        "a reply",
        "--output",
        replied.to_str().unwrap(),
    ]));
    // The reply is appended after the fixture's four annotations.
    assert_ok(&run(&[
        "object-copy",
        replied.to_str().unwrap(),
        "--page",
        "1",
        "--annotations",
        "4",
        "--clip",
        clip.to_str().unwrap(),
    ]));
    let o = run(&[
        "object-paste",
        replied.to_str().unwrap(),
        "--page",
        "1",
        "--clip",
        clip.to_str().unwrap(),
        "--preview",
    ]);
    assert_ok(&o);
    let stdout = String::from_utf8_lossy(&o.stdout);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stdout.contains("replies_unthreaded=1"), "{stdout}");
    assert!(
        stderr.contains("1 annotation(s) were replies"),
        "the lost thread must be disclosed: {stderr}"
    );
    for p in [replied, clip] {
        std::fs::remove_file(p).ok();
    }
}
