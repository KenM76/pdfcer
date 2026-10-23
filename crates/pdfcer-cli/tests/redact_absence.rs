//! `pdfcer redact-apply` runs the absence proof before writing: a removed
//! string still drawn elsewhere refuses with no output file.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests fail loudly.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A one-page PDF drawing `content` in Helvetica.
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
    std::env::temp_dir().join(format!(
        "pdfcer-redact-absence-{}-{name}",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

/// Write `content`, mark the band around y=100, return the marked path.
fn marked(name: &str, content: &str) -> PathBuf {
    let src = tmp(&format!("{name}-src.pdf"));
    std::fs::write(&src, pdf(content)).unwrap();
    let out = tmp(&format!("{name}-marked.pdf"));
    let mark = run(&[
        "redact-mark",
        src.to_str().unwrap(),
        "--rect",
        "10,90,390,130",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(mark.status.success(), "{mark:?}");
    let _ = std::fs::remove_file(src);
    out
}

fn apply(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "redact-apply",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

const TWICE: &str =
    "BT /F1 12 Tf 20 100 Td (CLASSIFIED) Tj ET BT /F1 12 Tf 20 20 Td (CLASSIFIED) Tj ET";

#[test]
fn a_drawn_survivor_refuses_and_writes_nothing() {
    let input = marked("refuse", TWICE);
    let output = tmp("refuse-out.pdf");
    let _ = std::fs::remove_file(&output);
    let out = apply(&input, &output, &[]);
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(!output.exists(), "a refusal must not write the file");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("survivor: \"CLASSIFIED\""), "{err}");
    assert!(err.contains("--allow-drawn-survivors"), "{err}");
    let _ = std::fs::remove_file(input);
}

#[test]
fn allow_drawn_survivors_writes_and_reports_them() {
    let input = marked("allow", TWICE);
    let output = tmp("allow-out.pdf");
    let out = apply(&input, &output, &["--allow-drawn-survivors"]);
    assert!(output.exists(), "{out:?}");
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(
        s.contains("  absence: NOT verified (drawn survivors allowed) "),
        "{s}"
    );
    assert!(
        s.contains("    residual site=drawn-content text=\"CLASSIFIED\""),
        "{s}"
    );
    for p in [input, output] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn a_single_occurrence_is_verified() {
    let input = marked("verified", "BT /F1 12 Tf 20 100 Td (CLASSIFIED) Tj ET");
    let output = tmp("verified-out.pdf");
    let out = apply(&input, &output, &[]);
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("  absence: verified strings_checked="), "{s}");
    assert!(s.contains(" residuals=0 "), "{s}");
    assert_eq!(out.status.code(), Some(0), "{s}");
    for p in [input, output] {
        let _ = std::fs::remove_file(p);
    }
}
