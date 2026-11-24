//! `pdfcer fix-hairlines` widens a thin page stroke and a thin form stroke,
//! prints the reach and the disclosures, and a second run finds nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A 0.1-wide page stroke, a 1-wide one, and a 0.05-wide stroke in a form
/// drawn at scale 2.
fn fixture() -> Vec<u8> {
    let page = "0.1 w 0 0 20 20 re S\n1 w 50 50 10 10 re S\nq 2 0 0 2 10 50 cm /Fm0 Do Q\n";
    let fm0 = "0.05 w 0 0 5 5 re S\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject \
         << /Fm0 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", fm0),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-fix-hairlines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN)
        .arg("fix-hairlines")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn widens_page_and_form_strokes_and_says_what_it_skipped() {
    let input = scratch("in.pdf");
    std::fs::write(&input, fixture()).unwrap();
    let once = scratch("once.pdf");
    let (code, out) = run(&[
        input.to_str().unwrap(),
        "-o",
        once.to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("widened=2 zero_width=0 forms=1"), "{out}");
    assert!(out.contains("undo_identical=1"), "{out}");
    assert!(out.contains("page 1: 2 widened"), "{out}");
    assert!(
        out.contains("form 5 0: drawn 1 time(s) on 1 page(s)"),
        "{out}"
    );
    assert!(
        out.contains("disclosure: Strokes inside Type 3 glyphs"),
        "{out}"
    );
    assert!(out.contains("pdf dimensions"), "{out}");

    let twice = scratch("twice.pdf");
    let (code, out) = run(&[once.to_str().unwrap(), "-o", twice.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("widened=0"), "{out}");
}

#[test]
fn refuses_a_page_past_the_end_and_a_zero_minimum() {
    let input = scratch("refuse.pdf");
    std::fs::write(&input, fixture()).unwrap();
    let out = scratch("refused.pdf");
    let (code, _) = run(&[
        input.to_str().unwrap(),
        "--pages",
        "2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_ne!(code, 0);
    let (code, _) = run(&[
        input.to_str().unwrap(),
        "--min-width",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_ne!(code, 0);
}
