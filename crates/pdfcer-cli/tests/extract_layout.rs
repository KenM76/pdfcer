//! `pdfcer extract-layout` (`G054`): inferred blocks per page, with every
//! inference counted on the result line.

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

fn line(font: &str, size: f32, x: f32, y: f32, text: &str) -> String {
    format!("BT /{font} {size} Tf {x} {y} Td ({text}) Tj ET\n")
}

/// Two pages: a 20 pt heading and body text on each, and a page number in
/// the bottom margin that only repetition identifies as running text.
fn input(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-extract-layout-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.pdf");
    let page = |n: usize| {
        let mut s = String::from("STREAM:");
        s.push_str(&line("F1", 20.0, 72.0, 700.0, &format!("Chapter {n}")));
        s.push_str(&line(
            "F1",
            10.0,
            72.0,
            660.0,
            "The body of the page begins here and",
        ));
        s.push_str(&line(
            "F1",
            10.0,
            72.0,
            648.0,
            "continues on a second line of text.",
        ));
        s.push_str(&line("F1", 10.0, 300.0, 40.0, &format!("{n}")));
        s
    };
    let res = "/Resources << /Font << /F1 7 0 R >> >>";
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R {res} >>"),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R {res} >>"),
        page(1),
        page(2),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    std::fs::write(&path, build_pdf(&bodies)).unwrap();
    path
}

#[test]
fn extract_layout_prints_blocks_and_counts_each_inference() {
    let path = input("text");
    let o = Command::new(BIN)
        .args(["extract-layout", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("page 2\n  heading1 inferred left \"Chapter 2\"\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("  paragraph inferred left \"The body of the page begins here and continues on a second line of text.\"\n"),
        "{stdout}"
    );
    assert!(stdout.contains("  page-number inferred "), "{stdout}");
    let last = stdout.lines().last().unwrap();
    assert!(last.contains(" pages=2 blocks=6 inferred=6 "), "{last}");
    assert!(last.contains(" headings_from_size=2 "), "{last}");
    assert!(last.contains(" page_numbers=2 "), "{last}");
}

#[test]
fn extract_layout_json_carries_level_lines_and_boxes() {
    let path = input("json");
    let o = Command::new(BIN)
        .args(["extract-layout", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("\"kind\": \"heading\", \"level\": 1, \"marker\": null, \"source\": \"inferred\", \"lines\": [0]"),
        "{stdout}"
    );
    assert!(stdout.contains("\"kind\": \"page-number\""), "{stdout}");
    assert!(
        stdout.contains("\"text\": \"Chapter 1\", \"bbox\": ["),
        "{stdout}"
    );
}
