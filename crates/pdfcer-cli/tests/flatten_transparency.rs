//! `pdfcer flatten-transparency` composites a page's transparency into
//! opaque content, prints what it inferred, and refuses a bad resolution.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_flattr_{tag}_{}.pdf", std::process::id()))
}

/// One page: an opaque square, then a half-transparent one over it.
fn fixture(tag: &str) -> PathBuf {
    let content = "1 0.8 0 rg 10 10 50 40 re f /X gs 0 0.4 1 rg 30 20 40 40 re f\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 80] /Contents 4 0 R \
/Resources << /ExtGState << /X << /ca 0.5 >> >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let p = path(tag);
    std::fs::write(&p, buf).unwrap();
    p
}

fn pdfcer(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

#[test]
fn flattens_reports_and_keeps_the_original_revision() {
    let input = fixture("in");
    let out = path("out");
    let run = pdfcer(&[
        "flatten-transparency",
        input.to_str().unwrap(),
        "--ppi",
        "144",
        "--verify-undo",
        "-o",
        out.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(stdout.contains("page=1 mode=overlay"), "{stdout}");
    assert!(!stdout.contains("changed=0 "), "{stdout}");
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    let before = std::fs::read(&input).unwrap();
    let after = std::fs::read(&out).unwrap();
    // Incremental: the original revision is a byte-identical prefix.
    assert!(after.len() > before.len());
    assert_eq!(&after[..before.len()], &before[..]);

    // The flattened file has nothing left to flatten.
    let again = path("again");
    let run = pdfcer(&[
        "flatten-transparency",
        out.to_str().unwrap(),
        "-o",
        again.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("page=1 mode=untouched"), "{stdout}");
    assert!(stdout.contains("changed=0 "), "{stdout}");
    for p in [input, out, again] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn refuses_a_resolution_out_of_range() {
    let input = fixture("in_low");
    let out = path("low");
    let run = pdfcer(&[
        "flatten-transparency",
        input.to_str().unwrap(),
        "--ppi",
        "10",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        run.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(!out.exists());
    let _ = std::fs::remove_file(input);
}
