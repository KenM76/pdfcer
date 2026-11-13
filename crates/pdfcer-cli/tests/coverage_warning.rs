//! `pdfcer coverage-warning`: total area coverage and rich black masks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A 60×60 page: the left half is 50/50/50/100 CMYK (250%, rich black),
/// the right half 100% K alone.
fn halves(tag: &str) -> PathBuf {
    let content = "0.5 0.5 0.5 1 k 0 0 30 60 re f 0 0 0 1 k 30 0 30 60 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path =
        std::env::temp_dir().join(format!("pdfcer_coverage_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> (Option<i32>, String) {
    let tag: Vec<&str> = args
        .iter()
        .filter(|a| a.chars().all(char::is_alphanumeric))
        .copied()
        .collect();
    let pdf = halves(&tag.join("_"));
    let out = Command::new(BIN)
        .arg("coverage-warning")
        .arg(&pdf)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[test]
fn total_area_coverage_flags_only_the_heavy_half() {
    let (code, out) = run(&["--tac", "240"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("rule=tac:240 flagged=1800 of=3600 percent=50.00"),
        "{out}"
    );
}

#[test]
fn exactly_the_limit_is_not_flagged() {
    let (code, out) = run(&["--tac", "250"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("flagged=0 of=3600"), "{out}");
}

#[test]
fn rich_black_skips_plain_black_and_writes_the_mask() {
    let png = std::env::temp_dir().join(format!("pdfcer_coverage_{}.png", std::process::id()));
    let (code, out) = run(&["--rich-black", "95", "-o", png.to_str().unwrap()]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("rule=rich-black:95 flagged=1800"), "{out}");
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn a_hidden_plate_counts_as_no_ink() {
    let (code, out) = run(&["--rich-black", "95", "--hide-ink", "Black"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("flagged=0"), "{out}");
}

#[test]
fn both_rules_print_one_line_each_tac_first() {
    let (code, out) = run(&["--rich-black", "95", "--tac", "240"]);
    assert_eq!(code, Some(0), "{out}");
    let tac = out.find("rule=tac:240 flagged=1800").expect(&out);
    let rich = out.find("rule=rich-black:95 flagged=1800").expect(&out);
    assert!(tac < rich, "{out}");
}

#[test]
fn both_rules_with_a_mask_file_are_refused() {
    let png = std::env::temp_dir().join(format!("pdfcer_coverage_both_{}.png", std::process::id()));
    let (code, out) = run(&[
        "--tac",
        "240",
        "--rich-black",
        "95",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code, Some(9), "{out}");
    assert!(out.contains("not both"), "{out}");
    assert!(!png.exists());
}
