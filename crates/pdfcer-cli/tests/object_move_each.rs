//! `pdfcer object-move-each` — several objects, each by its own offset, one
//! edit; a path by operand rewrite and an image by a `q cm Q` wrap.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A path under a 2x CTM and a 5x-placed image — a synthetic page.
fn mixed_pdf(tag: &str) -> PathBuf {
    build(
        tag,
        "q 2 0 0 2 0 0 cm 0 0 10 10 re S Q\nq 5 0 0 5 40 40 cm /Im1 Do Q",
    )
}

/// A synthetic page drawing `content`, with `/Im1` and a form `/Fm1` whose
/// own content is a path and an image.
fn build(tag: &str, content: &str) -> PathBuf {
    let form = "0 0 10 10 re S q 4 0 0 4 20 20 cm /Im1 Do Q";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /XObject << /Im1 5 0 R /Fm1 6 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Length 1 >>\nstream\n\u{0}\nendstream"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500]              /Resources << /XObject << /Im1 5 0 R >> >> /Length {} >>
stream
{form}
endstream",
            form.len()
        ),
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
    let path =
        std::env::temp_dir().join(format!("pdfcer_move_each_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str], input: &Path) -> Output {
    Command::new(BIN)
        .arg("object-move-each")
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

#[test]
fn a_path_and_an_image_each_move_by_their_own_offset() {
    let input = mixed_pdf("ok");
    let output = input.with_extension("out.pdf");
    let out = run(
        &[
            "--move",
            "0,5,0",
            "--move",
            "1,-3,4",
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
    assert!(stdout.contains(" moved=2 leaf=0 "), "{stdout}");
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    let listing = object_list(&output);
    assert!(
        listing.contains("index=0 kind=path bbox=5,0,25,20 "),
        "{listing}"
    );
    assert!(
        listing.contains("index=1 kind=image bbox=37,44,42,49 "),
        "{listing}"
    );
}

#[test]
fn an_object_named_twice_refuses_and_writes_nothing() {
    let input = mixed_pdf("dup");
    let output = input.with_extension("dup-out.pdf");
    let out = run(
        &[
            "--move",
            "1,5,0",
            "--move",
            "1,0,5",
            "-o",
            output.to_str().unwrap(),
        ],
        &input,
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("named more than once"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!output.exists(), "a refusal must write no output");
}

/// `--leaf` addresses objects inside a placed form, through its 2x placement.
#[test]
fn leaf_moves_reach_objects_inside_a_form() {
    let input = build("leaf", "q 2 0 0 2 100 100 cm /Fm1 Do Q");
    let output = input.with_extension("leaf-out.pdf");
    let before = object_list(&input);
    assert!(before.contains("leaves=2"), "{before}");
    let out = run(
        &[
            "--leaf",
            "--move",
            "0,6,0",
            "--move",
            "1,0,-8",
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
    assert!(
        stdout.contains(" leaf=1 invocations=1 pages=1 "),
        "{stdout}"
    );
    let after = object_list(&output);
    assert!(after.contains("bbox=106,100,126,120"), "{after}");
    assert!(after.contains("bbox=140,132,148,140"), "{after}");
}
