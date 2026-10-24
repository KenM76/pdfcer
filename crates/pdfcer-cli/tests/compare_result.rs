//! `compare --output` (`Pass 645.1`): the annotated result is written, the
//! sources are untouched, and an output naming a source is refused.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-compare-result-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

/// A one-page PDF drawing `text` in Helvetica 12.
fn pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET");
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
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f\r\n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn pair() -> (PathBuf, PathBuf) {
    let (old, new) = (temp("old.pdf"), temp("new.pdf"));
    std::fs::write(&old, pdf("one two three four")).expect("old");
    std::fs::write(&new, pdf("one three FOUR five")).expect("new");
    (old, new)
}

#[test]
fn the_result_is_written_and_the_sources_are_untouched() {
    let (old, new) = pair();
    let before = (std::fs::read(&old).unwrap(), std::fs::read(&new).unwrap());
    let out = temp("result.pdf");
    let (code, stdout, stderr) = run(&["compare", s(&old), s(&new), "--output", s(&out)]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("2 change(s) marked with 2 annotation(s), 0 unmarked; summary page"),
        "{stdout}"
    );
    assert_eq!(
        before,
        (std::fs::read(&old).unwrap(), std::fs::read(&new).unwrap())
    );
    let result = std::fs::read(&out).expect("written");
    assert!(
        result.starts_with(&before.1),
        "an incremental update of NEW"
    );
    let update = String::from_utf8_lossy(&result[before.1.len()..]);
    assert_eq!(update.matches("/Subtype /Caret").count(), 1, "deleted");
    assert_eq!(update.matches("/Subtype /Highlight").count(), 1, "replaced");
    assert!(update.contains("/Count 2"), "the summary page was added");
}

#[test]
fn no_summary_leaves_the_page_count_alone() {
    let (old, new) = pair();
    let out = temp("nosummary.pdf");
    let (code, stdout, stderr) = run(&[
        "compare",
        s(&old),
        s(&new),
        "--output",
        s(&out),
        "--no-summary",
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(!stdout.contains("summary page"), "{stdout}");
    let result = std::fs::read(&out).expect("written");
    let update = String::from_utf8_lossy(&result[std::fs::read(&new).unwrap().len()..]);
    assert!(
        !update.contains("/Count"),
        "the page tree is untouched: {update}"
    );
}

#[test]
fn an_output_naming_a_source_is_refused_before_anything_runs() {
    let (old, new) = pair();
    let before = std::fs::read(&new).unwrap();
    let (code, stdout, stderr) = run(&["compare", s(&old), s(&new), "--output", s(&new)]);
    assert_eq!(code, 2, "{stdout}\n{stderr}");
    assert!(stdout.is_empty(), "refused before the report: {stdout}");
    assert_eq!(before, std::fs::read(&new).unwrap());
}
