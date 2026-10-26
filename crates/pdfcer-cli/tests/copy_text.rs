//! `pdfcer copy-text`: the `-o` route, which writes the same payloads the
//! clipboard route places. The clipboard placement itself is not run here,
//! for the reason `copy_page.rs` gives.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(objects: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A bold heading, a blank line, and a paragraph wrapped over two lines.
fn page() -> Vec<u8> {
    let content = "BT /F2 14 Tf 72 730 Td (Seat Belts) Tj ET\n\
BT /F1 12 Tf 72 712 Td ( ) Tj ET\n\
BT /F1 12 Tf 72 694 Td (One paragraph that) Tj ET\n\
BT /F1 12 Tf 72 680 Td (wraps.) Tj ET\n";
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>"
            .into(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .into(),
    ])
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("pdfcer-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("spawn")
}

fn copy_to(format: &str) -> (String, String) {
    let dir = TempDir::new(&format!("copy-text-{format}"));
    let input = dir.0.join("in.pdf");
    std::fs::write(&input, page()).unwrap();
    let out = dir.0.join("out");
    let o = run(&[
        "copy-text",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--format",
        format,
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    (std::fs::read_to_string(&out).unwrap(), stdout)
}

#[test]
fn text_is_one_line_per_paragraph_with_the_wrap_joined() {
    let (text, line) = copy_to("text");
    assert_eq!(text, "Seat Belts\n\nOne paragraph that wraps.\n");
    assert!(
        line.starts_with("copied paragraphs=3 weights_derived=")
            && line.contains("families_missing=0 colours_dropped=0 to=file"),
        "{line}"
    );
}

#[test]
fn rtf_keeps_the_heading_bold_and_its_size() {
    let (rtf, _) = copy_to("rtf");
    assert!(rtf.starts_with("{\\rtf1"), "{rtf}");
    assert!(rtf.contains("\\fs28\\b Seat Belts}\\par"), "{rtf}");
    assert!(rtf.contains("One paragraph that wraps."), "{rtf}");
}

#[test]
fn format_without_output_is_refused_by_clap() {
    let o = run(&["copy-text", "x.pdf", "--format", "rtf"]);
    assert_eq!(o.status.code(), Some(2));
}
