//! `pdfcer set-text-annot-style --label/--reset-label` change a placed
//! stamp's words (pdfcer-gui request G151).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn blank(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_stamp_label_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn pdfcer(args: &[&str], input: &Path, output: &Path) -> Output {
    Command::new(BIN)
        .arg(args[0])
        .arg(input)
        .args(&args[1..])
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn stamped(tag: &str) -> (PathBuf, PathBuf) {
    let input = blank(tag);
    let stamped = input.with_extension("stamped.pdf");
    let out = pdfcer(
        &[
            "annotate",
            "--type",
            "stamp",
            "--page",
            "1",
            "--rect",
            "20,20,200,60",
            "--text",
            "CHECKED",
            "--note",
            "see sheet 2",
        ],
        &input,
        &stamped,
    );
    assert!(out.status.success(), "{out:?}");
    (input, stamped)
}

fn restyle(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["set-text-annot-style", "--page", "1", "--index", "0"];
    args.extend_from_slice(extra);
    pdfcer(&args, input, output)
}

fn text(path: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
}

#[test]
fn label_replaces_the_words_and_keeps_the_comment() {
    let (input, stamped) = stamped("relabel");
    let output = input.with_extension("out.pdf");
    let out = restyle(&stamped, &output, &["--label", "APPROVED"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("label_written=1"));
    let tail = text(&output);
    let tail = &tail[tail.rfind("/Stamp").unwrap()..];
    assert!(tail.contains("(see sheet 2)"), "comment kept: {tail}");
    assert!(text(&output).contains("(APPROVED)"));
    for f in [input, stamped, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn reset_label_restores_the_default_words() {
    let (input, stamped) = stamped("reset");
    let output = input.with_extension("out.pdf");
    let out = restyle(&stamped, &output, &["--reset-label"]);
    assert!(out.status.success(), "{out:?}");
    assert!(text(&output).contains("(DRAFT)"));
    for f in [input, stamped, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn a_blank_label_or_both_flags_are_refused() {
    let (input, stamped) = stamped("refuse");
    let output = input.with_extension("never.pdf");
    let out = restyle(&stamped, &output, &["--label", " "]);
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    let out = restyle(&stamped, &output, &["--label", "X", "--reset-label"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(!output.exists());
    for f in [input, stamped] {
        let _ = std::fs::remove_file(f);
    }
}
