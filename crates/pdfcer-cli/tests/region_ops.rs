//! Region operations: `object-delete --rect` (Pass 694.0), `redact-mark
//! --outside` (Pass 694.1) and `annotate --highlighter` (Pass 695.0).

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A one-page 400×200 PDF drawing `content`, with Helvetica as `/F1`.
fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
         /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f\r\n");
    for o in offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer-region-ops-{}-{name}", std::process::id()))
}

fn source(name: &str, content: &str) -> PathBuf {
    let p = tmp(&format!("{name}-src.pdf"));
    std::fs::write(&p, pdf(content)).unwrap();
    p
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// One rectangle wholly inside `10,10,100,60`, one crossing its right
/// edge, one far outside.
const THREE_RECTS: &str = "0 g 20 20 20 20 re f 90 20 60 20 re f 300 100 50 50 re f";

#[test]
fn a_region_deletes_the_enclosed_object_and_reports_the_crossing_one() {
    let src = source("enclosed", THREE_RECTS);
    let out = tmp("enclosed-out.pdf");
    let o = run(&[
        "object-delete",
        src.to_str().unwrap(),
        "--rect",
        "10,10,100,60",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let line = stdout(&o);
    assert!(
        line.contains(" select=enclosed deleted=1 crossing_kept=1 "),
        "{line}"
    );
    let listed = stdout(&run(&["object-list", out.to_str().unwrap()]));
    assert!(listed.contains(" objects=2 paths=2 "), "{listed}");
}

#[test]
fn touched_also_deletes_what_crosses_the_edge() {
    let src = source("touched", THREE_RECTS);
    let out = tmp("touched-out.pdf");
    let o = run(&[
        "object-delete",
        src.to_str().unwrap(),
        "--rect",
        "10,10,100,60",
        "--select",
        "touched",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    assert!(
        stdout(&o).contains(" deleted=2 crossing_kept=0 "),
        "{}",
        stdout(&o)
    );
}

#[test]
fn an_empty_region_refuses_and_writes_nothing() {
    let src = source("empty", THREE_RECTS);
    let out = tmp("empty-out.pdf");
    let _ = std::fs::remove_file(&out);
    let o = run(&[
        "object-delete",
        src.to_str().unwrap(),
        "--rect",
        "200,10,250,40",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(!out.exists());
}

#[test]
fn an_outside_redaction_keeps_only_the_region() {
    let src = source(
        "outside",
        "BT /F1 12 Tf 20 150 Td (KEEPME) Tj ET BT /F1 12 Tf 250 40 Td (GONEAWAY) Tj ET",
    );
    let marked = tmp("outside-marked.pdf");
    let m = run(&[
        "redact-mark",
        src.to_str().unwrap(),
        "--rect",
        "10,140,150,170",
        "--outside",
        "--output",
        marked.to_str().unwrap(),
    ]);
    assert_eq!(m.status.code(), Some(0), "{m:?}");
    let applied = tmp("outside-applied.pdf");
    let a = run(&[
        "redact-apply",
        marked.to_str().unwrap(),
        "--output",
        applied.to_str().unwrap(),
    ]);
    assert_eq!(a.status.code(), Some(0), "{a:?}");
    let text = stdout(&run(&["extract-text", applied.to_str().unwrap()]));
    assert!(text.contains("KEEPME"), "{text}");
    assert!(!text.contains("GONEAWAY"), "{text}");
}

#[test]
fn outside_needs_a_rect() {
    let src = source("outside-norect", THREE_RECTS);
    let o = run(&[
        "redact-mark",
        src.to_str().unwrap(),
        "--outside",
        "--output",
        tmp("outside-norect-out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
}

#[test]
fn a_highlighter_ellipse_is_a_borderless_multiply_wash() {
    let src = source("hl", "BT /F1 12 Tf 20 100 Td (TEXT) Tj ET");
    let out = tmp("hl-out.pdf");
    let o = run(&[
        "annotate",
        src.to_str().unwrap(),
        "--type",
        "circle",
        "--page",
        "1",
        "--rect",
        "10,90,120,120",
        "--highlighter",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let bytes = std::fs::read(&out).unwrap();
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("/BM /Multiply"), "no annotation blend");
    assert!(s.contains("/IC [1.0 1.0 0.0]"), "no yellow fill");
    assert!(
        !s.contains("/C [1.0 1.0 0.0]"),
        "a highlighter has no border colour"
    );
}

#[test]
fn a_highlighter_line_is_refused() {
    let src = source("hl-line", THREE_RECTS);
    let o = run(&[
        "annotate",
        src.to_str().unwrap(),
        "--type",
        "line",
        "--page",
        "1",
        "--line",
        "10,10,100,10",
        "--highlighter",
        "--output",
        tmp("hl-line-out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(9), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("--highlighter"));
}
