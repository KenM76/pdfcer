//! `pdfcer object-transform-each` — several objects, each by its own
//! page-space matrix, one edit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A 10x5 rectangle under a 2x CTM (page box 0,0..20,10) and a 5x-placed
/// image (page box 40,40..45,45) — a synthetic page.
fn mixed_pdf(tag: &str) -> PathBuf {
    let content = "q 2 0 0 2 0 0 cm 0 0 10 5 re S Q\nq 5 0 0 5 40 40 cm /Im1 Do Q";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /XObject << /Im1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Length 1 >>\nstream\n\u{0}\nendstream"
            .to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_transform_each_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str], input: &Path) -> Output {
    Command::new(BIN)
        .arg("object-transform-each")
        .arg(input)
        .args(args)
        .output()
        .unwrap()
}

fn object_list(path: &Path) -> String {
    let out = Command::new(BIN)
        .arg("object-list")
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The path turns a quarter about its own centre (10,5); the image moves by
/// (-3,4). Each lands where only its own matrix could put it.
#[test]
fn a_path_turns_and_an_image_moves_each_by_its_own_matrix() {
    let input = mixed_pdf("ok");
    let output = input.with_extension("out.pdf");
    let out = run(
        &[
            "--transform",
            "0,0,1,-1,0,15,-5",
            "--transform",
            "1,1,0,0,1,-3,4",
            "--verify-undo",
            "-o",
            output.to_str().unwrap(),
        ],
        &input,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(" transformed=2 "), "{stdout}");
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    let listing = object_list(&output);
    assert!(
        listing.contains("index=0 kind=path bbox=5,-5,15,15 "),
        "{listing}"
    );
    assert!(
        listing.contains("index=1 kind=image bbox=37,44,42,49 "),
        "{listing}"
    );
}

#[test]
fn a_singular_matrix_refuses_and_writes_nothing() {
    let input = mixed_pdf("sing");
    let output = input.with_extension("sing-out.pdf");
    let out = run(
        &[
            "--transform",
            "0,1,0,0,1,5,0",
            "--transform",
            "1,0,0,0,1,0,0",
            "-o",
            output.to_str().unwrap(),
        ],
        &input,
    );
    assert!(!out.status.success());
    assert!(!output.exists(), "a refusal must write no output");
}

#[test]
fn an_object_named_twice_refuses_and_writes_nothing() {
    let input = mixed_pdf("dup");
    let output = input.with_extension("dup-out.pdf");
    let out = run(
        &[
            "--transform",
            "1,1,0,0,1,5,0",
            "--transform",
            "1,1,0,0,1,0,5",
            "-o",
            output.to_str().unwrap(),
        ],
        &input,
    );
    assert!(!out.status.success());
    assert!(!output.exists(), "a refusal must write no output");
}

#[test]
fn a_short_matrix_is_refused_naming_the_shape() {
    let input = mixed_pdf("bad");
    let output = input.with_extension("bad-out.pdf");
    let out = run(
        &["--transform", "0,1,0,0,1,5", "-o", output.to_str().unwrap()],
        &input,
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("INDEX,A,B,C,D,E,F"), "{stderr}");
    assert!(!output.exists());
}
