//! `pdfcer inspect` reports overprint use and the PDF/X claim.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests fail loudly.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn inspect(rel: &str) -> String {
    let path = format!(
        "{}/../../fixtures/synthetic/{rel}",
        env!("CARGO_MANIFEST_DIR")
    );
    let out = Command::new(BIN).args(["inspect", &path]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn an_overprinting_file_says_so_and_names_where() {
    let out = inspect("overprint/cmyk_k_op_over_spot.pdf");
    assert!(
        out.contains("  overprint: requested (first by object "),
        "{out}"
    );
    assert!(!out.contains("PDF/X claim"), "{out}");
}

#[test]
fn an_info_claim_without_an_output_intent_is_printed_with_that_note() {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] >>",
        "<< /GTS_PDFXVersion (PDF/X-3:2003) >>",
    ];
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for o in offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_inspect_prepress_x3_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    let out = Command::new(BIN)
        .arg("inspect")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("  PDF/X claim: PDF/X-3:2003 (no GTS_PDFX output intent)"),
        "{stdout}"
    );
    assert!(!stdout.contains("overprint"), "{stdout}");
}
