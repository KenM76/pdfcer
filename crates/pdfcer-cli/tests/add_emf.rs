//! `add-emf` (`Pass 446.0`): an EMF placed as a Form XObject of vector
//! operators, its fit reported, and every skipped record named on both
//! channels. The EMF files are built here from [MS-EMF] records.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn le(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// An EMF of `records` (`(type, fields)`), framed 100 × 50 mm at 0.1 mm per
/// logical unit.
fn emf(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (kind, fields) in records {
        body.extend_from_slice(&kind.to_le_bytes());
        body.extend_from_slice(&(8 + fields.len() as u32).to_le_bytes());
        body.extend_from_slice(fields);
    }
    let mut out = le(&[1, 108, 0, 0, 999, 499, 0, 0, 9_999, 4_999]);
    out.extend_from_slice(b" EMF");
    out.extend_from_slice(&le(&[0x0001_0000, (108 + body.len() + 20) as i32]));
    out.extend_from_slice(&le(&[records.len() as i32 + 2, 8, 0, 0, 0]));
    out.extend_from_slice(&le(&[1000, 1000, 100, 100, 0, 0, 0, 100_000, 100_000]));
    out.extend_from_slice(&body);
    out.extend_from_slice(&le(&[0x0E, 20, 0, 16, 20]));
    out
}

/// A red brush, a rectangle, and an EMR_ARC pdfcer does not draw.
fn picture() -> Vec<u8> {
    emf(&[
        (0x27, le(&[1, 0, 0x0000_00FF, 0])),
        (0x25, le(&[1])),
        (0x2B, le(&[100, 100, 500, 300])),
        (0x2D, le(&[0, 0, 10, 10, 0, 0, 10, 10])),
    ])
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-add-emf-tests-{}", std::process::id()));
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

fn emf_file(name: &str, body: &[u8]) -> PathBuf {
    let p = temp_out(name);
    std::fs::write(&p, body).expect("emf written");
    p
}

/// Default fit is contain: a 2:1 picture in a square is centred
/// vertically, and the skipped record is named on stdout and stderr.
#[test]
fn an_emf_is_placed_contained_and_the_skipped_record_is_named() {
    let file = emf_file("draw.emf", &picture());
    let out = temp_out("placed.pdf");
    let (code, stdout, stderr) = run(&[
        "add-emf",
        s(&fixture()),
        "--emf",
        s(&file),
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
        "skipped=EMR_ARC:1",
        "emf_plus_ignored=0",
        "undo_identical=1",
    ] {
        assert!(stdout.contains(needle), "stdout carries {needle}: {stdout}");
    }
    assert!(stderr.contains("EMR_ARC"), "stderr names it: {stderr}");
    let text = String::from_utf8_lossy(&std::fs::read(&out).expect("output")).into_owned();
    assert!(text.contains("/Subtype /Form"), "a Form XObject is written");
    assert!(!text.contains("/Subtype /Image"), "no raster");
}

/// `--natural` uses the picture frame's own size; `--stamp --stretch` fills
/// the rectangle as a `/Stamp` and reports the distortion.
#[test]
fn natural_and_stretched_stamp_fits() {
    let file = emf_file("fit.emf", &picture());
    let out = temp_out("natural.pdf");
    let (code, stdout, stderr) = run(&[
        "add-emf",
        s(&fixture()),
        "--emf",
        s(&file),
        "--page",
        "1",
        "--rect",
        "10,20,30,40",
        "--natural",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("placed=10.000,20.000,293.465,161.732"),
        "{stdout}"
    );
    let out = temp_out("stamp.pdf");
    let (code, stdout, stderr) = run(&[
        "add-emf",
        s(&fixture()),
        "--emf",
        s(&file),
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
}

/// Not an EMF, or EMF+ only: refused (exit 9), nothing written, the
/// reason named.
#[test]
fn a_non_emf_or_emf_plus_only_file_is_refused() {
    let mut plus = le(&[28]);
    plus.extend_from_slice(b"EMF+");
    plus.extend_from_slice(&[0x01, 0x40, 0, 0]);
    plus.extend_from_slice(&le(&[28, 16, 0xDBC0_1002_u32 as i32, 0, 96, 96]));
    let cases = [
        (emf_file("not.emf", b"%PDF-1.7 not an emf"), "not an EMF"),
        (emf_file("plus.emf", &emf(&[(0x46, plus)])), "EMF+"),
    ];
    for (file, reason) in cases {
        let out = temp_out("refused.pdf");
        let (code, stdout, stderr) = run(&[
            "add-emf",
            s(&fixture()),
            "--emf",
            s(&file),
            "--page",
            "1",
            "--rect",
            "0,0,100,100",
            "-o",
            s(&out),
        ]);
        assert_eq!(code, 9, "{stdout}\n{stderr}");
        assert!(stderr.contains(reason), "{reason}: {stderr}");
        assert!(!out.exists(), "nothing is written");
    }
}

/// `--stamp --note --author --opacity`: the stamp carries `/T`, `/Contents`
/// and `/CA`; without `--stamp` the markup flags are refused by the parser.
#[test]
fn an_emf_stamp_takes_the_markup_flags() {
    let file = emf_file("signed.emf", &picture());
    let out = temp_out("signed.pdf");
    let (code, stdout, stderr) = run(&[
        "add-emf",
        s(&fixture()),
        "--emf",
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
        "add-emf",
        s(&fixture()),
        "--emf",
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
