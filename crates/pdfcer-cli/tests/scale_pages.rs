//! # `pdfcer scale-pages` integration tests (`Pass 364.0`)
//!
//! Through the binary, because the per-page disclosure (scale, offset,
//! mode) and the flag wiring are what unit tests on core cannot see.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A1 landscape with a Square annotation — the downscale case.
fn a1_sheet() -> Vec<u8> {
    let content = "0 0 m 2384 1684 l S";
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 2384 1684] /Resources << >> \
         /Contents 4 0 R /Annots [5 0 R] >>"
            .into(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Annot /Subtype /Square /Rect [100 100 300 300] >>".into(),
    ])
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer-scale-pages-{}-{name}", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

#[test]
fn scale_pages_downscales_and_discloses_each_page() {
    let input = tmp("in.pdf");
    let output = tmp("out.pdf");
    std::fs::write(&input, a1_sheet()).unwrap();
    let out = run(&[
        "scale-pages",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--size",
        "letter",
        "--verify-undo",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");

    // Letter, turned to the landscape sheet (the default `match`).
    assert!(stderr.contains("page 1: scale=0.332215"), "{stderr}");
    assert!(
        stderr.contains("mode=fit sheet=792.0000x612.0000"),
        "{stderr}"
    );
    assert!(
        stderr.contains("target turned to match the page"),
        "{stderr}"
    );
    assert!(stdout.contains("pages_scaled=1"), "{stdout}");
    assert!(stdout.contains("annotations=1"), "{stdout}");
    assert!(stdout.contains("undo_identical=1"), "{stdout}");

    // The saved file IS the new sheet: scaling it to the same size again
    // is the identity.
    let again = tmp("again.pdf");
    let out = run(&[
        "scale-pages",
        output.to_str().unwrap(),
        "-o",
        again.to_str().unwrap(),
        "--size",
        "792x612",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("page 1: scale=1.000000 offset=(0.0000, 0.0000)"),
        "{stderr}"
    );
    let _ = std::fs::remove_file(&again);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn scale_pages_takes_a_custom_size_and_exact_orientation() {
    let input = tmp("in2.pdf");
    let output = tmp("out2.pdf");
    std::fs::write(&input, a1_sheet()).unwrap();
    let out = run(&[
        "scale-pages",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--size",
        "400x800",
        "--orientation",
        "exact",
        "--scale-mode",
        "fill",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("mode=fill sheet=400.0000x800.0000"),
        "{stderr}"
    );
    assert!(!stderr.contains("turned"), "{stderr}");
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn scale_pages_refuses_an_unknown_size() {
    let input = tmp("in3.pdf");
    std::fs::write(&input, a1_sheet()).unwrap();
    let out = run(&[
        "scale-pages",
        input.to_str().unwrap(),
        "-o",
        tmp("out3.pdf").to_str().unwrap(),
        "--size",
        "b99",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("neither a sheet name"), "{stderr}");
    assert!(!tmp("out3.pdf").exists());
    let _ = std::fs::remove_file(&input);
}
