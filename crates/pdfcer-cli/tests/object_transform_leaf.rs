//! `pdfcer object-transform`, page route and `--leaf` (objects inside a form
//! XObject, `transform_objects_in_form`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::Bounds;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Page object 0: a 10x10 square at (60,60). Leaf 0: a 10x10 square inside
/// `Fm0`, placed at `2 0 0 2 10 10 cm`, so 20x20 on the page.
fn fixture(tag: &str) -> PathBuf {
    let page = "60 60 10 10 re S\nq 2 0 0 2 10 10 cm /Fm0 Do Q\n";
    let form = "0 0 10 10 re S\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
         /Resources << /XObject << /Fm0 5 0 R >> >> >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length {} \
             >>\nstream\n{form}endstream",
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
    let path = std::env::temp_dir().join(format!(
        "pdfcer_transform_leaf_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(input: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("object-transform")
        .arg(input)
        .args(args)
        .output()
        .unwrap()
}

fn leaf0(path: &Path) -> Bounds {
    let mut s = EditSession::new(Document::load(path).unwrap());
    s.page_objects(0).unwrap().leaves[0].object.page_bbox()
}

fn width(b: Bounds) -> f64 {
    b.max.x - b.min.x
}

#[test]
fn leaf_scales_the_form_object_about_its_own_centre() {
    let input = fixture("leaf");
    let output = input.with_extension("out.pdf");
    let before = leaf0(&input);
    let out = Command::new(BIN)
        .args(["object-transform"])
        .arg(&input)
        .args(["--objects", "0", "--leaf", "--scale", "2", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("leaf=1 invocations=1 pages=1"), "{line}");

    let after = leaf0(&output);
    assert!(
        (width(after) - 2.0 * width(before)).abs() < 1e-6,
        "{after:?}"
    );
    let mid = |b: Bounds| (b.min.x + b.max.x) / 2.0;
    assert!(
        (mid(after) - mid(before)).abs() < 1e-6,
        "default pivot = leaf centre"
    );
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn without_leaf_the_page_object_moves_and_the_form_does_not() {
    let input = fixture("page");
    let output = input.with_extension("out.pdf");
    let out = Command::new(BIN)
        .args(["object-transform"])
        .arg(&input)
        .args(["--objects", "0", "--translate", "5,0", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("leaf=0 mode="), "{line}");
    assert!(line.contains(" transformed=1 "), "{line}");
    assert_eq!(leaf0(&output), leaf0(&input), "the form is untouched");
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn preview_with_leaf_and_a_bad_leaf_are_refused() {
    let input = fixture("refuse");
    let out = run(
        &input,
        &["--objects", "0", "--leaf", "--scale", "2", "--preview"],
    );
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    let output = input.with_extension("out.pdf");
    let out = Command::new(BIN)
        .args(["object-transform"])
        .arg(&input)
        .args(["--objects", "4", "--leaf", "--scale", "2", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(!output.exists());
    let _ = std::fs::remove_file(input);
}
