//! `pdfcer extract-region` (`Pass 449.0`): the flags reach the core verb
//! and the report reaches stdout. The cutting itself is tested in
//! `pdfcer-core/tests/region_export.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

const CONTENT: &str = "\
BT /F1 12 Tf 60 100 Td (INSIDE) Tj ET
BT /F1 12 Tf 300 300 Td (OUTSIDE) Tj ET
/OC /L1 BDC BT /F1 12 Tf 60 140 Td (HIDDENLAYER) Tj ET EMC
/OC /L2 BDC BT /F1 12 Tf 60 120 Td (SHOWNLAYER) Tj ET EMC";

const AP: &str = "BT /F1 10 Tf 2 5 Td (ANNOTIN) Tj ET";

fn fixture() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [6 0 R 7 0 R] \
         /D << /Order [6 0 R 7 0 R] /OFF [6 0 R] >> >> >>"
            .to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] \
         /Resources << /Font << /F1 5 0 R >> /Properties << /L1 6 0 R /L2 7 0 R >> >> \
         /Contents 4 0 R /Annots [8 0 R] >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
            CONTENT.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
        "<< /Type /OCG /Name (Hidden) >>".to_string(),
        "<< /Type /OCG /Name (Shown) >>".to_string(),
        "<< /Type /Annot /Subtype /Square /Rect [100 160 200 190] /F 4 /AP << /N 9 0 R >> >>"
            .to_string(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 30] \
             /Resources << /Font << /F1 5 0 R >> >> /Length {} >>\nstream\n{AP}\nendstream",
            AP.len()
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
    buf
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "pdfcer-test-extract-region-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `extract-region` on the fixture with `extra` flags.
fn run(tag: &str, extra: &[&str]) -> (Output, Option<Document>) {
    let dir = TempDir::new(tag);
    let input = dir.0.join("in.pdf");
    let output = dir.0.join("out.pdf");
    std::fs::write(&input, fixture()).unwrap();
    let mut args = vec![
        "extract-region",
        input.to_str().unwrap(),
        "--page",
        "1",
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    if !extra.contains(&"--rect") {
        args.extend_from_slice(&["--rect", "50,50,250,250"]);
    }
    let out = Command::new(BIN).args(&args).output().expect("spawn");
    let doc = std::fs::read(&output)
        .ok()
        .map(|b| Document::from_bytes(b).unwrap());
    (out, doc)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn text(doc: &Document) -> String {
    let pages = page_tree::pages(doc).unwrap();
    extract_page(doc, &pages[0], 0, &ExtractOptions::default())
        .unwrap()
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect()
}

#[test]
fn writes_the_region_and_prints_the_report() {
    let (out, doc) = run("default", &[]);
    assert!(out.status.success(), "{out:?}");
    let s = stdout(&out);
    assert!(s.contains("region: rect=[50 50 250 250]"), "{s}");
    assert!(s.contains("layers: hidden=1 kept=1"), "{s}");
    assert!(
        s.contains("annotations: fields_flattened=0 widgets_flattened=0 flattened=1"),
        "{s}"
    );
    assert!(s.contains("residuals=no"), "{s}");
    let doc = doc.unwrap();
    let page = &page_tree::pages(&doc).unwrap()[0];
    assert_eq!(page.media_box, Rect::from_corners(50.0, 50.0, 250.0, 250.0));
    let t = text(&doc);
    assert!(t.contains("INSIDE") && t.contains("ANNOTIN"), "{t}");
    assert!(!t.contains("OUTSIDE") && !t.contains("HIDDENLAYER"), "{t}");
}

#[test]
fn no_annotations_removes_them() {
    let (out, doc) = run("no-annots", &["--no-annotations"]);
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains("flattened=0 ce_dimensions_flattened=0 removed=1"));
    assert!(!text(&doc.unwrap()).contains("ANNOTIN"));
}

#[test]
fn layer_flags_replace_the_default_state() {
    let flags = ["--show-layer", "Hidden", "--hide-layer", "Shown"];
    let (out, doc) = run("layers", &flags);
    assert!(out.status.success(), "{out:?}");
    let t = text(&doc.unwrap());
    assert!(
        t.contains("HIDDENLAYER") && !t.contains("SHOWNLAYER"),
        "{t}"
    );
}

#[test]
fn an_unknown_layer_name_is_a_note() {
    let (out, _) = run("unknown-layer", &["--hide-layer", "Nope"]);
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains("note: no layer is named \"Nope\""));
}

#[test]
fn refusals_exit_9_and_write_nothing() {
    for (tag, flags) in [
        ("zero-area", &["--rect", "10,10,10,200"][..]),
        ("three-numbers", &["--rect", "1,2,3"][..]),
        (
            "clash",
            &["--show-layer", "Shown", "--hide-layer", "Shown"][..],
        ),
    ] {
        let (out, doc) = run(tag, flags);
        assert_eq!(out.status.code(), Some(9), "{tag}: {out:?}");
        assert!(doc.is_none(), "{tag} wrote an output");
    }
}
