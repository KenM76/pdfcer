//! `pdfcer set-object-paint --leaf` and `set-object-stroke-style --leaf` —
//! the same edits on paths inside a form XObject, addressed by leaf index.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{PathObject, PathPaint, VectorObject};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Leaf 0: a stroked square inside `Fm0`, which carries its own `/Resources`.
fn form_fixture(tag: &str) -> PathBuf {
    let page = "q 1 0 0 1 10 10 cm /Fm0 Do Q\n";
    let form = "0 0 10 10 re S\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
         /Resources << /XObject << /Fm0 5 0 R >> >> >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources << >> /Length {} \
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
        "pdfcer_style_in_form_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(verb: &str, input: &Path, output: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg(verb)
        .arg(input)
        .args(args)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn leaf0(path: &Path) -> PathObject {
    let mut s = EditSession::new(Document::load(path).unwrap());
    match &s.page_objects(0).unwrap().leaves[0].object {
        VectorObject::Path(p) => p.clone(),
        other => panic!("not a path: {other:?}"),
    }
}

#[test]
fn paint_and_style_with_leaf_edit_inside_the_form() {
    let input = form_fixture("both");
    let painted = input.with_extension("painted.pdf");
    let out = run(
        "set-object-paint",
        &input,
        &painted,
        &["--objects", "0", "--leaf", "--stroke", "#0000ff"],
    );
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("leaf=true invocations=1 pages=1"), "{line}");
    assert!(line.contains("changed=0"), "{line}");
    assert!(matches!(
        leaf0(&painted).stroke_paint,
        PathPaint::Device { rgb, .. } if rgb.b == 1.0 && rgb.r == 0.0
    ));

    let styled = input.with_extension("styled.pdf");
    let out = run(
        "set-object-stroke-style",
        &painted,
        &styled,
        &[
            "--objects",
            "0",
            "--leaf",
            "--width",
            "4",
            "--stroke-alpha",
            "0.5",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let p = leaf0(&styled);
    assert!((p.line_width - 4.0).abs() < 1e-9 && p.stroke_alpha == 0.5);
    for f in [input, painted, styled] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn a_leaf_index_out_of_range_writes_nothing() {
    let input = form_fixture("range");
    let output = input.with_extension("out.pdf");
    let out = run(
        "set-object-stroke-style",
        &input,
        &output,
        &["--objects", "5", "--leaf", "--width", "2"],
    );
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(!output.exists());
    let _ = std::fs::remove_file(input);
}
