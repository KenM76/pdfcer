//! `pdfcer object-copy --leaf`: copy objects inside a form XObject
//! (`copy_objects_in_form`) and paste them as page content where they were
//! drawn.

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
    let path =
        std::env::temp_dir().join(format!("pdfcer_copy_leaf_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn a_copied_leaf_pastes_where_and_as_large_as_it_was_drawn() {
    let input = fixture("paste");
    let clip = input.with_extension("clip");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "object-copy",
        s(&input),
        "--objects",
        "0",
        "--leaf",
        "--clip",
        s(&clip),
    ]);
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("objects=1 leaf=1 "), "{line}");
    let out = run(&[
        "object-paste",
        s(&input),
        "--clip",
        s(&clip),
        "-o",
        s(&output),
    ]);
    assert!(out.status.success(), "{out:?}");

    let drawn = EditSession::new(Document::load(&input).unwrap())
        .page_objects(0)
        .unwrap()
        .leaves[0]
        .object
        .page_bbox();
    let mut after = EditSession::new(Document::load(&output).unwrap());
    let model = after.page_objects(0).unwrap();
    let pasted: Vec<Bounds> = model.objects.iter().map(|o| o.page_bbox()).collect();
    assert!(pasted.contains(&drawn), "{pasted:?} vs {drawn:?}");
    assert!((drawn.max.x - drawn.min.x - 20.0).abs() < 1e-6);
    for f in [input, clip, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn leaf_with_cut_or_annotations_and_a_bad_leaf_are_refused() {
    let input = fixture("refuse");
    let clip = input.with_extension("clip");
    let cut = input.with_extension("cut.pdf");
    for extra in [
        &["--cut", s(&cut)][..],
        &["--annotations", "0"][..],
        &[][..],
    ] {
        let objects = if extra.is_empty() { "4" } else { "0" };
        let mut args = vec![
            "object-copy",
            s(&input),
            "--objects",
            objects,
            "--leaf",
            "--clip",
            s(&clip),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(9), "{extra:?} {out:?}");
        assert!(!clip.exists() && !cut.exists(), "{extra:?}");
    }
    let _ = std::fs::remove_file(input);
}
