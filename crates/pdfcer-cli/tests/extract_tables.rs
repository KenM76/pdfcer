//! `pdfcer extract-tables` (`G055`): ruled tables as a cell grid, with
//! every inference counted on the result line.

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
    let dir = std::env::temp_dir().join(format!("pdfcer-extract-tables-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.pdf");
    let content = "0 G 0.5 w\n\
        72 700 m 328 700 l S 72 680 m 328 680 l S 72 660 m 328 660 l S 72 640 m 328 640 l S\n\
        72 640 m 72 700 l S 328 640 m 328 700 l S 200 640 m 200 680 l S\n\
        BT /F1 10 Tf 76 686 Td (Parts list) Tj ET\n\
        BT /F2 10 Tf 76 666 Td (Part) Tj ET BT /F2 10 Tf 204 666 Td (Qty) Tj ET\n\
        BT /F1 10 Tf 76 646 Td (Bolt M6) Tj ET BT /F1 10 Tf 204 646 Td (12) Tj ET\n";
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
fn extract_tables_prints_the_grid_and_counts_each_inference() {
    let path = input("text");
    let o = Command::new(BIN)
        .args(["extract-tables", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.starts_with("page 1 table 1 ruled rows=3 cols=2 header=none\n  r0c0[1x2] \"Parts list\"\n  r1c0 \"Part\"\n  r1c1 \"Qty\"\n  r2c0 \"Bolt M6\"\n  r2c1 \"12\"\n"),
        "{stdout}"
    );
    let last = stdout.lines().last().unwrap();
    assert!(
        last.contains(" pages=1 tables=1 inferred=2 ruled=1 aligned=0 aligned_rejected=0 cells=5 merged_cells=1 header_rows=0 "),
        "{last}"
    );
    assert!(last.contains(" single_cell_frames=0 "), "{last}");
}

#[test]
fn extract_tables_json_carries_spans_bands_and_glyph_refs() {
    let path = input("json");
    let o = Command::new(BIN)
        .args(["extract-tables", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("\"row\": 0, \"col\": 0, \"row_span\": 1, \"col_span\": 2, \"bbox\": [72.00, 680.00, 328.00, 700.00], \"text\": \"Parts list\", \"glyphs\": [[0, 0], "),
        "{stdout}"
    );
    assert!(stdout.contains("\"source\": \"ruled\""), "{stdout}");
    assert!(
        stdout.contains(
            "\"columns\": [[72.00, 640.00, 200.00, 700.00], [200.00, 640.00, 328.00, 700.00]]"
        ),
        "{stdout}"
    );
}
