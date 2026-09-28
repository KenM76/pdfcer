//! `pdfcer extract-tags` (`G053`): the structure tree in reading order,
//! with the disagreement counters on the result line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => (*body).to_owned(),
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

fn input(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-extract-tags-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.pdf");
    std::fs::write(
        &path,
        build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>",
            "STREAM:/H1 <</MCID 0>> BDC BT /F1 12 Tf 72 700 Td (Title) Tj ET EMC\n\
             /P <</MCID 1>> BDC BT /F1 12 Tf 72 680 Td (Body) Tj ET EMC",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            "<< /Type /StructTreeRoot /K 7 0 R /RoleMap << /Heading1 /H1 >> >>",
            "<< /S /Document /P 6 0 R /K [8 0 R 9 0 R] >>",
            "<< /S /Heading1 /P 7 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /P /P 7 0 R /Pg 3 0 R /K [1 7] >>",
        ]),
    )
    .unwrap();
    path
}

#[test]
fn extract_tags_prints_the_tree_and_the_counters() {
    let path = input("text");
    let o = Command::new(BIN)
        .args(["extract-tags", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("\n  H1 (Heading1) p1 \"Title\"\n"),
        "{stdout}"
    );
    assert!(stdout.contains("\n  P p1 \"Body\"\n"), "{stdout}");
    let last = stdout.lines().last().unwrap();
    assert!(last.contains(" elements=3 "), "{last}");
    assert!(last.contains(" named_not_declared=1 "), "{last}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("no BDC declares"));
}

#[test]
fn extract_tags_json_carries_the_raw_and_mapped_types() {
    let path = input("json");
    let o = Command::new(BIN)
        .args(["extract-tags", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("\"type\": \"H1\", \"raw_type\": \"Heading1\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"text\": \"Title\""), "{stdout}");
    assert!(stdout.contains("\"mcid\": 7, \"page\": 1, \"form\": null, \"declared\": false"));
}
