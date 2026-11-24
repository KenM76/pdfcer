//! `pdfcer fix-pdfua` applies the fixups, prints one line per fixup and a
//! verdict, and writes the claim only for a file that then passes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(objects: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
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

/// A tagged page holding one tagged figure and one untagged square.
fn tagged() -> Vec<u8> {
    let content = "/Figure << /MCID 0 >> BDC 0 0 10 10 re f EMC 20 20 5 5 re f";
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 5 0 R >>"
            .into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
         /Contents 4 0 R /StructParents 0 >>"
            .into(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        "<< /Type /StructTreeRoot /K 6 0 R /ParentTree 8 0 R >>".into(),
        "<< /Type /StructElem /S /Document /P 5 0 R /K 7 0 R >>".into(),
        "<< /Type /StructElem /S /Figure /P 6 0 R /Pg 3 0 R /K 0 /Alt (A square) >>".into(),
        "<< /Nums [0 [7 0 R]] >>".into(),
    ])
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-fix-pdfua-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN)
        .arg("fix-pdfua")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

fn validate(path: &Path) -> i32 {
    let out = Command::new(BIN)
        .args(["validate-pdfua", path.to_str().unwrap()])
        .output()
        .unwrap();
    out.status.code().unwrap()
}

#[test]
fn a_fixable_file_is_fixed_claimed_and_then_validates() {
    let input = temp("tagged.pdf");
    std::fs::write(&input, tagged()).unwrap();
    let output = temp("tagged-fixed.pdf");
    let (code, out) = run(&[
        input.to_str().unwrap(),
        "--lang",
        "en-US",
        "--title",
        "A square",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("fix-pdfua "), "{out}");
    for line in [
        "fixup language: applied",
        "fixup display-doc-title: applied",
        "fixup artifacts: applied",
        "fixup identification: applied",
    ] {
        assert!(out.contains(line), "{line}: {out}");
    }
    assert!(out.contains("disclosure: Treated 1 untagged"), "{out}");
    assert!(
        out.contains("verdict: no machine-checkable failure"),
        "{out}"
    );
    assert_eq!(validate(&output), 0);
}

#[test]
fn an_untagged_file_is_saved_unclaimed_and_exits_18() {
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    let output = temp("hello-fixed.pdf");
    let (code, out) = run(&[
        hello.to_str().unwrap(),
        "--lang",
        "en",
        "--json",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code, 18, "{out}");
    assert!(out.contains("\"identified\": false"), "{out}");
    assert!(out.contains("\"passes\": false"), "{out}");
    assert!(
        out.contains("\"fixup\": \"identification\", \"outcome\": \"refused\""),
        "{out}"
    );
    assert!(output.exists());
}
