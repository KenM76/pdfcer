//! `turn-widget` and `edit-widget --opacity` (pdfcer-gui request G160).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// One text field `T` at `[100 100 200 120]` with a foreign appearance.
fn input(tag: &str) -> PathBuf {
    let ap = "0 0 1 rg 0 0 100 20 re f\n";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (T) /P 3 0 R /Rect [100 100 200 120] \
         /AP << /N 5 0 R >> >>"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 20] /Length {} >>\nstream\n{ap}endstream",
            ap.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_widget_turn_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn widget(path: &Path) -> pdfcer_core::object::Dict {
    let doc = Document::load(path).unwrap();
    let form = pdfcer_core::forms::parse_acroform(&doc).unwrap();
    let id = form.fields[0].widgets[0].id;
    doc.resolved(id).as_dict().cloned().unwrap()
}

#[test]
fn turn_widget_turns_and_reports() {
    let src = input("turn");
    let out = src.with_extension("out.pdf");
    let o = run(&[
        "turn-widget",
        src.to_str().unwrap(),
        "--name",
        "T",
        "--degrees",
        "-30",
        "-o",
        out.to_str().unwrap(),
        "--verify-undo",
    ]);
    let stdout = String::from_utf8_lossy(&o.stdout);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("turn-widget ") && stdout.contains("was=0 now=-30"),
        "{stdout}"
    );
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    assert!(
        stderr.contains("/MK"),
        "the regenerating-viewer disclosure: {stderr}"
    );
    let rect: Vec<f64> = widget(&out)
        .get(b"Rect")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_number().unwrap())
        .collect();
    // 100 x 20 at 30 degrees bounds to 96.6 x 67.3.
    assert!((rect[3] - rect[1] - 67.32).abs() < 0.01, "{rect:?}");
}

#[test]
fn turn_widget_refuses_a_quarter_turn() {
    let src = input("quarter");
    let out = src.with_extension("out.pdf");
    let o = run(&[
        "turn-widget",
        src.to_str().unwrap(),
        "--name",
        "T",
        "--degrees",
        "90",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("rotate_widget"));
}

#[test]
fn edit_widget_writes_and_clears_opacity() {
    let src = input("opacity");
    let half = src.with_extension("half.pdf");
    let o = run(&[
        "edit-widget",
        src.to_str().unwrap(),
        "--name",
        "T",
        "--opacity",
        "0.5",
        "-o",
        half.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("/CA"));
    assert_eq!(
        widget(&half).get(b"CA").and_then(Object::as_number),
        Some(0.5)
    );
    let clear = src.with_extension("clear.pdf");
    let o = run(&[
        "edit-widget",
        half.to_str().unwrap(),
        "--name",
        "T",
        "--clear-opacity",
        "-o",
        clear.to_str().unwrap(),
    ]);
    assert!(o.status.success());
    assert!(widget(&clear).get(b"CA").is_none());
}
