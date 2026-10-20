//! `extract-text --auto-word-gap` (G209) over the real binary.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `extract-text --auto-word-gap` (G209): a page whose word
/// gaps are 0.15 em TJ kerns splits into words, and the JSON reports the
/// measured threshold.
#[test]
fn auto_word_gap_splits_tight_words_and_reports_the_threshold() {
    let dir = std::env::temp_dir().join(format!("pdfcer-wordgap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let content = "BT /F1 12 Tf 72 700 Td [(alfa)-150(brav)-150(char)-150(delt)-150(echo)] TJ \
0 -16 Td [(foxt)-150(golf)-150(hote)-150(indi)-150(juli)] TJ ET";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_owned(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = pdf.len();
    pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f\r\n");
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let input = dir.join("tight.pdf");
    std::fs::write(&input, pdf).unwrap();
    let settings = dir.join("auto.txt");
    std::fs::write(&settings, "word_gap_auto = true\n").unwrap();

    let run = |extra: &[&std::ffi::OsStr]| {
        let o = Command::new(BIN)
            .arg("--no-settings")
            .arg("extract-text")
            .arg(&input)
            .arg("--json")
            .args(extra)
            .output()
            .unwrap();
        let (out, err) = (
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr),
        );
        format!("{out}{err}")
    };
    let fixed = run(&[]);
    assert!(fixed.contains("\"word_gap_measured\": false"), "{fixed}");
    assert!(fixed.contains("alfabrav"), "{fixed}");
    let auto = run(&["--auto-word-gap".as_ref()]);
    assert!(auto.contains("\"word_gap_measured\": true"), "{auto}");
    assert!(!auto.contains("alfabrav"), "{auto}");
    let _ = std::fs::remove_dir_all(&dir);
}
