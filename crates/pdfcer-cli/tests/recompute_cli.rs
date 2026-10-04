//! `pdfcer recompute`: the plan report, its stderr caveats, and `--apply`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `Total` = SUM(A, B, C) listed in `/CO`; `C` has no value, so it is
/// coerced to zero. `Twice` = SUM(A) is calculated but missing from `/CO`.
fn calc_pdf() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 9 0 R] \
/CO [7 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
        "<< /FT /Tx /T (A) /V (2) >>",
        "<< /FT /Tx /T (B) /V (3) >>",
        "<< /FT /Tx /T (C) >>",
        "<< /FT /Tx /T (Total) /V (0) /AA << /C 8 0 R >> >>",
        r#"<< /S /JavaScript /JS (AFSimple_Calculate\("SUM", ["A", "B", "C"]\);) >>"#,
        "<< /FT /Tx /T (Twice) /V (0) /AA << /C 10 0 R >> >>",
        r#"<< /S /JavaScript /JS (AFSimple_Calculate\("SUM", ["A"]\);) >>"#,
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_recompute_{tag}_{}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn a_dry_run_reports_each_change_and_every_caveat_and_writes_nothing() {
    let input = scratch("dry");
    std::fs::write(&input, calc_pdf()).unwrap();
    let out = run(&["recompute", input.to_str().unwrap()]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stdout.contains("change field=\"Total\" from=\"0\" to=\"5\" op=SUM operands=3 coerced=1\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("change field=\"Twice\" from=\"0\" to=\"2\" "),
        "{stdout}"
    );
    assert!(
        stdout.contains(" changes=2 skipped=0 order=mixed applied=0\n"),
        "{stdout}"
    );
    assert!(
        stderr.contains("1 calculated field(s) its /CO array does not list"),
        "{stderr}"
    );
    assert!(
        stderr.contains("1 operand(s) were blank or non-numeric"),
        "{stderr}"
    );
    assert!(stderr.contains("nothing was written"), "{stderr}");
    let _ = std::fs::remove_file(&input);
}

#[test]
fn apply_writes_the_computed_values() {
    let input = scratch("apply_in");
    let output = scratch("apply_out");
    std::fs::write(&input, calc_pdf()).unwrap();
    let out = run(&[
        "recompute",
        input.to_str().unwrap(),
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(" applied=2\n"), "{stdout}");
    let listing = run(&["list-fields", output.to_str().unwrap()]);
    let listing = String::from_utf8(listing.stdout).unwrap();
    assert!(
        listing
            .lines()
            .any(|l| l.starts_with("field name=\"Total\" ") && l.contains(" value=\"5\" ")),
        "{listing}"
    );
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}
