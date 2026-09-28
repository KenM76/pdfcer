//! `pdfcer export-xlsx` (`Pass 380.0`): detected tables written as an
//! Excel workbook, every inference and every number decision counted.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => body.clone(),
        };
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

/// A 3x2 grid: a merged title row over two columns, a bold header row,
/// one data row.
fn input(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-export-xlsx-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.pdf");
    let content = "0 G 0.5 w\n\
        72 700 m 328 700 l S 72 680 m 328 680 l S 72 660 m 328 660 l S 72 640 m 328 640 l S\n\
        72 640 m 72 700 l S 328 640 m 328 700 l S 200 640 m 200 680 l S\n\
        BT /F1 10 Tf 76 686 Td (Parts list) Tj ET\n\
        BT /F2 10 Tf 76 666 Td (Part) Tj ET BT /F2 10 Tf 204 666 Td (Qty) Tj ET\n\
        BT /F1 10 Tf 76 646 Td (Bolt M6) Tj ET BT /F1 10 Tf 204 646 Td (1,234) Tj ET\n";
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>"
            .to_owned(),
        format!("STREAM:{content}"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    std::fs::write(&path, build_pdf(&bodies)).unwrap();
    path
}

#[test]
fn export_xlsx_writes_a_workbook_and_counts_ambiguous_numbers() {
    let path = input("auto");
    let out = path.with_file_name("out.xlsx");
    let o = Command::new(BIN)
        .args([
            "export-xlsx",
            path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let line = String::from_utf8_lossy(&o.stdout);
    assert!(
        line.contains(" pages=1 tables=1 inferred=2 ruled=1 aligned=0 merged_cells=1 header_rows=0 pages_unreadable=0 sheets=1 cells=5 numbers=0 ambiguous_numbers=1 characters_dropped=0 cells_truncated=0 cells_beyond_limits=0"),
        "{line}"
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(&[0x50, 0x4b, 3, 4]));
    assert_eq!(
        &bytes[bytes.len() - 22..bytes.len() - 18],
        &[0x50, 0x4b, 5, 6]
    );
}

#[test]
fn export_xlsx_numbers_flag_decides_the_locale() {
    let path = input("us");
    let out = path.with_file_name("out.xlsx");
    let o = Command::new(BIN)
        .args([
            "export-xlsx",
            path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--numbers",
            "us",
            "--sheets",
            "single",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let line = String::from_utf8_lossy(&o.stdout);
    assert!(
        line.contains(" sheets=1 cells=5 numbers=1 ambiguous_numbers=0 "),
        "{line}"
    );
}

#[test]
fn export_xlsx_pages_limits_what_is_read() {
    let path = input("pages");
    let out = path.with_file_name("out.xlsx");
    let o = Command::new(BIN)
        .args([
            "export-xlsx",
            path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--pages",
            "1",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains(" pages=1 tables=1 "),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let o = Command::new(BIN)
        .args([
            "export-xlsx",
            path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--pages",
            "2",
        ])
        .output()
        .unwrap();
    assert!(!o.status.success());
}
