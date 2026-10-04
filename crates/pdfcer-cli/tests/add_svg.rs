//! `add-svg` (`Pass 445.0`): an SVG placed as a Form XObject of vector
//! operators, its fit reported, and what was not carried named on both
//! channels.
#![cfg(feature = "svg-import")]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
  <defs><filter id="b"><feGaussianBlur stdDeviation="3"/></filter></defs>
  <rect x="10" y="10" width="80" height="40" fill="#336699" filter="url(#b)"/>
  <path d="M 110 10 L 190 50" stroke="#cc3300" stroke-width="6"/>
</svg>"##;

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-add-svg-tests-{}", std::process::id()));
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

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn svg_file(name: &str, body: &str) -> PathBuf {
    let p = temp_out(name);
    std::fs::write(&p, body).expect("svg written");
    p
}

/// Default fit is contain: a 2:1 drawing in a square is centred vertically,
/// and the filter that could not be carried is named on stdout and stderr.
#[test]
fn an_svg_is_placed_contained_and_the_filter_is_named() {
    let svg = svg_file("draw.svg", SVG);
    let out = temp_out("placed.pdf");
    let (code, stdout, stderr) = run(&[
        "add-svg",
        s(&fixture()),
        "--svg",
        s(&svg),
        "--page",
        "1",
        "--rect",
        "72,72,472,472",
        "--verify-undo",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    for needle in [
        "placed=72.000,172.000,472.000,372.000",
        "fit=contain",
        "as=content",
        "distorted=0",
        "not_carried=filter:1",
        "undo_identical=1",
    ] {
        assert!(stdout.contains(needle), "stdout carries {needle}: {stdout}");
    }
    assert!(
        stderr.contains("filter"),
        "stderr names the filter: {stderr}"
    );
    let bytes = std::fs::read(&out).expect("output written");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Form"), "a Form XObject is written");
    assert!(!text.contains("/Subtype /Image"), "no raster");
}

/// `--stamp --stretch`: a `/Stamp` annotation filling the rectangle, the
/// distortion reported.
#[test]
fn a_stretched_stamp_reports_its_distortion() {
    let svg = svg_file("stamp.svg", SVG);
    let out = temp_out("stamp.pdf");
    let (code, stdout, stderr) = run(&[
        "add-svg",
        s(&fixture()),
        "--svg",
        s(&svg),
        "--page",
        "1",
        "--rect",
        "72,72,472,472",
        "--stamp",
        "--stretch",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    for needle in [
        "placed=72.000,72.000,472.000,472.000",
        "as=stamp",
        "content=-",
        "distorted=1",
    ] {
        assert!(stdout.contains(needle), "stdout carries {needle}: {stdout}");
    }
    let text = String::from_utf8_lossy(&std::fs::read(&out).expect("output written")).into_owned();
    assert!(
        text.contains("/Subtype /Stamp"),
        "a stamp annotation is written"
    );
}

/// A file that is not an SVG is refused (exit 9) and nothing is written.
#[test]
fn a_non_svg_is_refused_before_anything_is_written() {
    let not_svg = svg_file("photo.svg", "\u{89}PNG not really");
    let bad = temp_out("bad.svg");
    std::fs::write(&bad, [0xff_u8, 0xfe, 0x00]).expect("written");
    for file in [&not_svg, &bad] {
        let out = temp_out("refused.pdf");
        let (code, stdout, stderr) = run(&[
            "add-svg",
            s(&fixture()),
            "--svg",
            s(file),
            "--page",
            "1",
            "--rect",
            "0,0,100,100",
            "-o",
            s(&out),
        ]);
        assert_eq!(code, 9, "{stdout}\n{stderr}");
        assert!(!out.exists(), "nothing is written");
    }
}

/// `--stamp --note --author --opacity`: the stamp carries `/T`, `/Contents`
/// and `/CA`; without `--stamp` the markup flags are refused by the parser.
#[test]
fn a_svg_stamp_takes_the_markup_flags() {
    let file = svg_file("signed.svg", SVG);
    let out = temp_out("signed.pdf");
    let (code, stdout, stderr) = run(&[
        "add-svg",
        s(&fixture()),
        "--svg",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,72,272,172",
        "--stamp",
        "--note",
        "checked",
        "--author",
        "Ken",
        "--opacity",
        "0.5",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let text = String::from_utf8_lossy(&std::fs::read(&out).expect("output")).into_owned();
    for needle in ["/T (Ken)", "/Contents (checked)", "/CA 0.5"] {
        assert!(text.contains(needle), "the stamp carries {needle}");
    }
    let refused = temp_out("unstamped.pdf");
    let (code, _, stderr) = run(&[
        "add-svg",
        s(&fixture()),
        "--svg",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,72,272,172",
        "--opacity",
        "0.5",
        "-o",
        s(&refused),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(!refused.exists(), "nothing is written");
}
