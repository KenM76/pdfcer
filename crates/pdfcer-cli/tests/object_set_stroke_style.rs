//! `pdfcer set-object-stroke-style` — sets width, dash and opacity on a path,
//! refuses text by index, and refuses an invalid value before writing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{Dash, PathObject, VectorObject};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Object 0: a stroked square. Object 1: text.
fn path_and_text(tag: &str) -> PathBuf {
    let content = "0 0 10 10 re S BT /F1 12 Tf 20 20 Td (A) Tj ET";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
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
        "pdfcer_set_stroke_style_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(input: &Path, output: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("set-object-stroke-style")
        .arg(input)
        .args(args)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn path0(file: &Path) -> PathObject {
    let mut s = EditSession::new(Document::from_bytes(std::fs::read(file).unwrap()).unwrap());
    let model = s.page_objects(0).unwrap();
    let VectorObject::Path(p) = &model.objects[0] else {
        panic!("object 0 is not a path");
    };
    p.clone()
}

#[test]
fn sets_width_dash_and_opacity_and_refuses_text() {
    let input = path_and_text("set");
    let output = input.with_extension("out.pdf");
    let out = run(
        &input,
        &output,
        &[
            "--objects",
            "0,1",
            "--width",
            "2",
            "--dash",
            "3,1",
            "--dash-phase",
            "0.5",
            "--stroke-alpha",
            "0.5",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("changed=0 refused=1"), "{stdout}");
    assert!(stderr.contains("object 1 not styled"), "{stderr}");
    let p = path0(&output);
    assert_eq!(p.line_width, 2.0);
    assert_eq!(p.dash, Dash::new(vec![3.0, 1.0], 0.5));
    assert!((p.stroke_alpha - 0.5).abs() < 1e-6);
    assert_eq!(p.fill_alpha, 1.0, "an option not given is left alone");
}

#[test]
fn solid_clears_a_dash() {
    let input = path_and_text("solid");
    let output = input.with_extension("out.pdf");
    let out = run(&input, &output, &["--objects", "0", "--dash", "solid"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(path0(&output).dash.is_solid());
}

#[test]
fn an_invalid_value_writes_nothing() {
    let input = path_and_text("bad");
    let output = input.with_extension("out.pdf");
    let out = run(&input, &output, &["--objects", "0", "--fill-alpha", "2"]);
    assert_eq!(
        out.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!output.exists());
}

#[test]
fn some_style_option_is_required() {
    let input = path_and_text("none");
    let output = input.with_extension("out.pdf");
    assert!(!run(&input, &output, &["--objects", "0"]).status.success());
}
