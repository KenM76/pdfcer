//! `format-text --underline / --strikethrough / --no-decoration`, through
//! the binary: core tests build the request directly, so only this run can
//! show the flags are wired.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const CONTENT: &str = "BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj ET";

fn fixture(tag: &str) -> PathBuf {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
            CONTENT.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!("pdfcer_deco_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, out).unwrap();
    path
}

fn format(input: &Path, tag: &str, flags: &[&str]) -> (i32, String) {
    let output = input.with_file_name(format!("pdfcer_deco_{tag}_out_{}.pdf", std::process::id()));
    let mut args = vec![
        "format-text".to_owned(),
        input.display().to_string(),
        "-o".to_owned(),
        output.display().to_string(),
        "--page".to_owned(),
        "1".to_owned(),
        "--find".to_owned(),
        "Hello".to_owned(),
    ];
    args.extend(flags.iter().map(|f| (*f).to_owned()));
    let run = Command::new(BIN).args(&args).output().unwrap();
    let bytes = std::fs::read(&output).unwrap_or_default();
    (
        run.status.code().unwrap(),
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[test]
fn underline_and_strikethrough_draw_two_rules() {
    let input = fixture("both");
    let (code, out) = format(&input, "both", &["--underline", "--strikethrough"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("/Line [/Underline /StrikeOut]"), "{out}");
    assert_eq!(out.matches("<</Rule 1>>").count(), 2, "{out}");
}

#[test]
fn no_decoration_conflicts_with_underline() {
    let input = fixture("conflict");
    let (code, _) = format(&input, "conflict", &["--underline", "--no-decoration"]);
    assert_eq!(code, 2);
}

#[test]
fn without_a_flag_nothing_is_decorated() {
    let input = fixture("none");
    let (code, out) = format(&input, "none", &["--set-size", "14"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("pdfc_Deco"), "{out}");
}

#[test]
fn decoration_metrics_standard_is_recorded_on_the_marker() {
    let input = fixture("standard");
    let (code, out) = format(
        &input,
        "standard",
        &["--underline", "--decoration-metrics", "standard"],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("/M /Standard"), "{out}");
    let (code, out) = format(&input, "font", &["--underline"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("/M /Standard"), "{out}");
}
