//! `pdfcer group-objects`, `ungroup-form`, `group-annotations` and
//! `ungroup-annotations` reach the engine with the arguments given.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{ImageSource, VectorObject};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

const CONTENT: &str =
    "0 0 1 rg 0 0 10 10 re f\n1 0 0 rg 5 5 10 10 re f\n0 1 0 rg 50 50 10 10 re f\n";

fn fixture(tag: &str) -> PathBuf {
    let annot = |kind: &str| format!("<< /Type /Annot /Subtype /{kind} /Rect [10 10 50 50] >>");
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
         /Resources << >> /Annots [5 0 R 6 0 R 7 0 R] >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}endstream",
            CONTENT.len()
        ),
        annot("Square"),
        annot("Circle"),
        annot("Link"),
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
    let path = std::env::temp_dir().join(format!("pdfcer_group_{tag}_{}.pdf", std::process::id()));
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

/// `F` for a form, `P` for a path, in paint order.
fn kinds(file: &Path) -> String {
    let mut s = EditSession::new(Document::from_bytes(std::fs::read(file).unwrap()).unwrap());
    s.page_objects(0)
        .unwrap()
        .objects
        .iter()
        .map(|o| match o {
            VectorObject::Image(i) if i.source == ImageSource::Form => 'F',
            VectorObject::Path(_) => 'P',
            _ => '?',
        })
        .collect()
}

#[test]
fn objects_group_and_ungroup() {
    let input = fixture("objects");
    let grouped = input.with_extension("grouped.pdf");
    let out = run(
        "group-objects",
        &input,
        &grouped,
        &["--objects", "0,1", "--verify-undo"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{:?}", out.stderr);
    assert!(
        stdout.contains(" index=0 ") && stdout.contains(" members=2 "),
        "{stdout}"
    );
    assert_eq!(kinds(&grouped), "FP");

    let loose = input.with_extension("loose.pdf");
    let out = run("ungroup-form", &grouped, &loose, &["--object", "0"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains(" indices=0..2 ") && stdout.contains(" clipped=false "),
        "{stdout}"
    );
    assert_eq!(kinds(&loose), "PPP");

    let out = run("ungroup-form", &loose, &grouped, &["--object", "0"]);
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a form"));
}

/// The index of the most deeply nested leaf, and how many forms deep it is.
fn deepest_leaf(file: &Path) -> (usize, usize) {
    let mut s = EditSession::new(Document::from_bytes(std::fs::read(file).unwrap()).unwrap());
    let model = s.page_objects(0).unwrap();
    (0..model.leaves.len())
        .map(|i| (i, model.leaves[i].containment.len()))
        .max_by_key(|&(_, depth)| depth)
        .unwrap()
}

#[test]
fn objects_group_and_ungroup_inside_a_group() {
    let input = fixture("nested");
    let outer = input.with_extension("outer.pdf");
    let out = run("group-objects", &input, &outer, &["--objects", "0,1,2"]);
    assert!(out.status.success(), "{:?}", out.stderr);
    assert_eq!(kinds(&outer), "F");

    let inner = input.with_extension("inner.pdf");
    let out = run(
        "group-objects",
        &outer,
        &inner,
        &["--objects", "0,1", "--leaf", "--verify-undo"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{:?}", out.stderr);
    assert!(
        stdout.contains(" members=2 ") && stdout.contains(" invocations=1 pages=1 "),
        "{stdout}"
    );
    assert_eq!(kinds(&inner), "F", "the page still paints the one group");

    // A leaf two forms deep: its own form goes back into the outer one.
    let deep = deepest_leaf(&inner);
    assert_eq!(deep.1, 2);
    let back = input.with_extension("back.pdf");
    let leaf = deep.0.to_string();
    let out = run(
        "ungroup-form",
        &inner,
        &back,
        &["--object", &leaf, "--leaf"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{:?}", out.stderr);
    assert!(
        stdout.contains(" clipped=false ") && !stdout.contains("container=page"),
        "{stdout}"
    );

    // One level left: the leaf's form is the page's own, so it lands there.
    assert_eq!(deepest_leaf(&back).1, 1);
    let loose = input.with_extension("loose.pdf");
    let out = run("ungroup-form", &back, &loose, &["--object", "0", "--leaf"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{:?}", out.stderr);
    assert!(stdout.contains(" container=page "), "{stdout}");
    assert_eq!(kinds(&loose), "PPP");

    let out = run(
        "group-objects",
        &loose,
        &back,
        &["--objects", "0", "--leaf"],
    );
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn annotations_group_and_ungroup() {
    let input = fixture("annots");
    let grouped = input.with_extension("grouped.pdf");
    let out = run(
        "group-annotations",
        &input,
        &grouped,
        &["--primary", "0", "--members", "1"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{:?}", out.stderr);
    assert!(stdout.contains(" count=1 "), "{stdout}");
    let bytes = std::fs::read(&grouped).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/RT /Group"));

    let loose = input.with_extension("loose.pdf");
    let out = run("ungroup-annotations", &grouped, &loose, &["--index", "1"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains(" count=1 "), "{stdout}");

    let out = run(
        "group-annotations",
        &input,
        &grouped,
        &["--primary", "0", "--members", "2"],
    );
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a markup annotation"));
}
