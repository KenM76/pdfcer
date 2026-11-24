//! `pdfcer convert-pdfx` converts an RGB document to PDF/X-4 and claims
//! it, and refuses the claim (exit 18, nothing written) for one that
//! cannot conform.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_convpdfx_{tag}_{}.pdf", std::process::id()))
}

/// One page drawing `content`, with `extra` objects from number 5.
fn fixture(tag: &str, content: &str, resources: &str, extra: &[&str]) -> PathBuf {
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 80] /Contents 4 0 R \
             /Resources {resources} >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
    ];
    bodies.extend(extra.iter().map(|s| (*s).to_owned()));
    let mut buf = b"%PDF-1.6\n".to_vec();
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
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    let p = path(tag);
    std::fs::write(&p, buf).unwrap();
    p
}

fn pdfcer(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn profile() -> String {
    root()
        .join("fixtures/synthetic/icc-rgb/dest-cmyk.icc")
        .to_str()
        .unwrap()
        .to_owned()
}

#[test]
fn converts_to_x4_and_claims_it() {
    let input = fixture("in", "0.2 0.4 0.8 rg 10 10 50 40 re f", "<< >>", &[]);
    let out = path("out");
    let icc = profile();
    let run = pdfcer(&[
        "convert-pdfx",
        input.to_str().unwrap(),
        "--level",
        "x-4",
        "--icc",
        &icc,
        "--condition",
        "Test CMYK",
        "--verify-undo",
        "-o",
        out.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(stdout.contains("step: "), "{stdout}");
    assert!(stdout.contains("note: "), "{stdout}");
    assert!(
        stdout.contains("before=fails after=unconfirmed claimed=1"),
        "{stdout}"
    );
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    let check = pdfcer(&["validate-pdfx", out.to_str().unwrap(), "--level", "x-4"]);
    let report = String::from_utf8_lossy(&check.stdout);
    assert!(check.status.success(), "{report}");
    assert!(
        report.contains("claim=\"PDF/X-4\" verdict=unconfirmed"),
        "{report}"
    );
    for p in [input, out] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn a_refused_claim_exits_18_and_writes_nothing() {
    let input = fixture(
        "font",
        "BT /F1 12 Tf 10 10 Td (Hi) Tj ET",
        "<< /Font << /F1 5 0 R >> >>",
        &["<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"],
    );
    let out = path("font_out");
    let icc = profile();
    let run = pdfcer(&[
        "convert-pdfx",
        input.to_str().unwrap(),
        "--level",
        "x-4",
        "--icc",
        &icc,
        "--json",
        "-o",
        out.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(run.status.code(), Some(18), "{stdout}");
    assert!(stdout.contains("\"claimed\": false"), "{stdout}");
    assert!(stdout.contains("\"written\": false"), "{stdout}");
    assert!(stdout.contains("NOT written"), "{stdout}");
    assert!(!out.exists(), "nothing written");
    let _ = std::fs::remove_file(input);
}
