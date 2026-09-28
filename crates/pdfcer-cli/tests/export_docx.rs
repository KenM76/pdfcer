//! `pdfcer export-docx` (`Pass 381.0`): block layout written as a Word
//! document, running text moved to the header and footer, every
//! inference counted.

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

fn line(font: &str, size: u32, x: u32, y: u32, text: &str) -> String {
    format!("BT /{font} {size} Tf {x} {y} Td ({text}) Tj ET\n")
}

/// Two pages with a repeated header and page number; page 1 has a
/// heading, a paragraph and a ruled 2x2 table.
fn input(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-export-docx-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.pdf");
    let running = |n: u32| {
        line("F1", 9, 72, 760, "Service Manual") + &line("F1", 9, 290, 30, &format!("Page {n}"))
    };
    let p1 = [
        running(1),
        line("F2", 20, 72, 690, "Maintenance"),
        line("F1", 10, 72, 660, "Check the bolts before every run."),
        "0 G 0.5 w\n72 640 m 328 640 l S 72 620 m 328 620 l S 72 600 m 328 600 l S\n\
         72 600 m 72 640 l S 200 600 m 200 640 l S 328 600 m 328 640 l S\n"
            .to_owned(),
        line("F1", 10, 76, 626, "Bolt"),
        line("F1", 10, 204, 626, "Torque"),
        line("F1", 10, 76, 606, "M6"),
        line("F1", 10, 204, 606, "10 Nm"),
    ]
    .concat();
    let p2 = running(2) + &line("F1", 10, 72, 500, "Second page text.");
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> >>"
            .to_owned(),
        format!("STREAM:{p1}"),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R \
         /Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> >>"
            .to_owned(),
        format!("STREAM:{p2}"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    std::fs::write(&path, build_pdf(&bodies)).unwrap();
    path
}

fn run(name: &str, extra: &[&str]) -> (String, Vec<u8>) {
    let path = input(name);
    let out = path.with_file_name("out.docx");
    let mut args = vec![
        "export-docx".to_owned(),
        path.to_str().unwrap().to_owned(),
        "-o".to_owned(),
        out.to_str().unwrap().to_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let o = Command::new(BIN).args(&args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        std::fs::read(&out).unwrap(),
    )
}

#[test]
fn export_docx_writes_a_document_and_counts_what_it_inferred() {
    let (line, bytes) = run("default", &[]);
    assert!(
        line.contains(
            " pages=2 inferred=8 headings=1 paragraphs=2 list_items=0 captions=0 tables=1 \
             table_cells=4 merged_cells=0 blocks_in_tables=4 tables_too_wide=0 header=1 footer=1 \
             page_number_field=1 running_blocks=4 running_variants_dropped=0 \
             runs_not_horizontal=0 runs_watermark_skipped=0 characters_dropped=0"
        ),
        "{line}"
    );
    assert!(bytes.starts_with(&[0x50, 0x4b, 3, 4]));
    assert_eq!(
        &bytes[bytes.len() - 22..bytes.len() - 18],
        &[0x50, 0x4b, 5, 6]
    );
}

#[test]
fn export_docx_flags_turn_off_tables_and_page_breaks() {
    let (line, _) = run("flags", &["--no-tables", "--no-page-breaks"]);
    assert!(
        line.contains(" tables=0 table_cells=0 merged_cells=0 blocks_in_tables=0 "),
        "{line}"
    );
}
