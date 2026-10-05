//! `extract-text --ocr-layer` and `export-docx --ocr-layer` over the real
//! binary: a page with visible "Original" text gains a pdfcer OCR layer
//! reading "INVOICE" (via `add-text --ocr-layer`), and each filter value
//! keeps its side.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pdfcer-cli-ocr-layer-{}-{tag}.pdf",
        std::process::id()
    ))
}

fn pdfcer(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

/// `plain.pdf` with an OCR layer holding "INVOICE"; the caller removes it.
fn mixed_page(tag: &str) -> PathBuf {
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    let out = temp_path(tag);
    let added = pdfcer(&[
        "add-text",
        input.to_str().unwrap(),
        "--at",
        "72,600",
        "--text",
        "INVOICE",
        "--ocr-layer",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    out
}

#[test]
fn extract_text_keeps_the_side_each_value_names() {
    let pdf = mixed_page("extract");
    let text = |value: &str| {
        let out = pdfcer(&["extract-text", pdf.to_str().unwrap(), "--ocr-layer", value]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let s = String::from_utf8_lossy(&out.stdout).into_owned();
        (s.contains("INVOICE"), s.contains("Original"))
    };
    let results = (text("all"), text("only"), text("without"));
    let _ = std::fs::remove_file(&pdf);
    assert_eq!(results.0, (true, true), "all");
    assert_eq!(results.1, (true, false), "only");
    assert_eq!(results.2, (false, true), "without");
}

#[test]
fn the_default_is_all_and_a_bad_value_is_refused() {
    let pdf = mixed_page("default");
    let plain = pdfcer(&["extract-text", pdf.to_str().unwrap()]);
    let all = pdfcer(&["extract-text", pdf.to_str().unwrap(), "--ocr-layer", "all"]);
    let bad = pdfcer(&["extract-text", pdf.to_str().unwrap(), "--ocr-layer", "some"]);
    let _ = std::fs::remove_file(&pdf);
    assert_eq!(plain.stdout, all.stdout);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--ocr-layer"));
}

#[test]
fn export_docx_drops_the_ocr_layer_on_request() {
    let pdf = mixed_page("docx");
    let docx = |value: &str| {
        let out_path = std::env::temp_dir().join(format!(
            "pdfcer-cli-ocr-layer-{}-{value}.docx",
            std::process::id()
        ));
        let out = pdfcer(&[
            "export-docx",
            pdf.to_str().unwrap(),
            "--ocr-layer",
            value,
            "--output",
            out_path.to_str().unwrap(),
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bytes = std::fs::read(&out_path).unwrap();
        let _ = std::fs::remove_file(out_path);
        bytes
    };
    let (all, without) = (docx("all"), docx("without"));
    let _ = std::fs::remove_file(&pdf);
    assert_ne!(all, without, "the filter reaches the document body");
}
