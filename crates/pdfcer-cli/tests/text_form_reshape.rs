//! `text-run-width`, `text-run-merge` and `text-object-split` with `--leaf`
//! (pdfcer-gui request G158).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::VectorObject;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `Fm0` drawn twice. Leaves 0–2 are the first drawing: 0 text `(Inv)
/// (oice)`, 1 text `(AB)` then `(CD)` at its own position, 2 text `(Wide)`.
fn input(tag: &str) -> PathBuf {
    let page = "q 2 0 0 2 10 10 cm /Fm0 Do Q\nq 1 0 0 1 10 120 cm /Fm0 Do Q\n";
    let fm0 = "BT /F1 6 Tf 0 0 Td (Inv) Tj (oice) Tj ET\n\
               BT /F1 6 Tf 0 10 Td (AB) Tj 20 0 Td (CD) Tj ET\n\
               BT /F1 6 Tf 0 20 Td (Wide) Tj ET\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject \
         << /Fm0 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 80 40] /Resources << /Font << /F1 6 0 R \
             >> >>",
            fm0,
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_text_form_reshape_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str], input: &Path, output: &Path) -> Output {
    Command::new(BIN)
        .arg(args[0])
        .arg(input)
        .args(&args[1..])
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn leaves(path: &Path) -> Vec<VectorObject> {
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    let mut s = EditSession::new(doc);
    s.page_objects(0)
        .unwrap()
        .leaves
        .iter()
        .map(|l| l.object.clone())
        .collect()
}

fn text_runs(objs: &[VectorObject], i: usize) -> usize {
    match &objs[i] {
        VectorObject::Text(t) => t.runs.len(),
        other => panic!("leaf {i}: {other:?}"),
    }
}

#[test]
fn width_merge_and_split_reach_inside_the_form() {
    let input = input("ok");

    let out_w = input.with_extension("w.pdf");
    let o = run(
        &[
            "text-run-width",
            "--leaf",
            "2",
            "--run",
            "0",
            "--width",
            "60",
        ],
        &input,
        &out_w,
    );
    assert!(o.status.success(), "{o:?}");
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains(" leaf=2 "), "{stdout}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("drawn 2 time(s)"));
    let b = leaves(&out_w)[2].page_bbox();
    assert!((b.max.x - b.min.x - 60.0).abs() < 0.5, "{b:?}");

    let out_m = input.with_extension("m.pdf");
    let o = run(
        &["text-run-merge", "--leaf", "0", "--run", "0,1"],
        &input,
        &out_m,
    );
    assert!(o.status.success(), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stdout).contains("text=\"Invoice\""));
    let objs = leaves(&out_m);
    assert_eq!((text_runs(&objs, 0), text_runs(&objs, 3)), (1, 1));

    let out_s = input.with_extension("s.pdf");
    let o = run(
        &["text-object-split", "--leaf", "1", "--before", "1"],
        &input,
        &out_s,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(leaves(&out_s).len(), leaves(&input).len() + 2);
}

#[test]
fn bad_targets_are_refused_without_output() {
    let input = input("bad");
    let output = input.with_extension("bad_out.pdf");
    for args in [
        &[
            "text-run-width",
            "--leaf",
            "9",
            "--run",
            "0",
            "--width",
            "60",
        ][..],
        &[
            "text-run-width",
            "--leaf",
            "2",
            "--object",
            "0",
            "--run",
            "0",
            "--width",
            "6",
        ],
        &["text-run-merge", "--leaf", "1", "--run", "0"],
        &["text-object-split", "--leaf", "1"],
        &[
            "text-object-split",
            "--leaf",
            "1",
            "--before",
            "1",
            "--dry-run",
        ],
    ] {
        let o = run(args, &input, &output);
        assert!(!o.status.success(), "{args:?} succeeded");
        assert!(!output.exists(), "{args:?} wrote output");
    }
}
