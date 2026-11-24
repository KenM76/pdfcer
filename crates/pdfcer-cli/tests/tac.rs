//! `pdfcer tac`, and a `total-area-coverage` rule inside `pdfcer preflight`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A 144x72 page drawing `content`, written to a temp file named by `tag`.
fn pdf(tag: &str, content: &str) -> PathBuf {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 144 72] /Contents 4 0 R >>".to_owned(),
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
    let path = std::env::temp_dir().join(format!("pdfcer_tac_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&OsStr]) -> (Option<i32>, String) {
    let out = Command::new(BIN).args(args).output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

fn tac(input: &Path, limit: &str, ppi: &str) -> (Option<i32>, String) {
    run(&[
        "tac".as_ref(),
        input.as_os_str(),
        "--limit".as_ref(),
        limit.as_ref(),
        "--ppi".as_ref(),
        ppi.as_ref(),
    ])
}

#[test]
fn rich_black_over_the_limit_reports_its_region_and_exits_18() {
    let input = pdf("over", "1 1 1 1 k 10 20 30 20 re f");
    let (code, out) = tac(&input, "300", "72");
    assert_eq!(code, Some(18), "{out}");
    assert!(
        out.contains("tac page=1 max=400.0 over=600 of=10368"),
        "{out}"
    );
    assert!(out.contains("ppi=72 limit=300"), "{out}");
    assert!(
        out.contains(
            "tac-region page=1 rect=10.00,20.00,40.00,40.00 max=400.0 pixels=600 merged=1"
        ),
        "{out}"
    );
}

#[test]
fn under_the_limit_exits_0_and_rgb_is_disclosed() {
    let input = pdf("rgb", "0 0 1 rg 0 0 144 72 re f");
    let (code, out) = tac(&input, "300", "36");
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("tac page=1 ") && out.contains(" over=0 "),
        "{out}"
    );
    assert!(
        out.contains("tac-caveat page=1") && out.contains("convert-colours"),
        "{out}"
    );
}

#[test]
fn a_bad_ppi_is_refused() {
    let input = pdf("badppi", "0 0 0 1 k 0 0 10 10 re f");
    let (code, out) = tac(&input, "300", "5000");
    assert_eq!(code, Some(9), "{out}");
}

#[test]
fn a_profile_rule_runs_inside_preflight() {
    let input = pdf("profile", "1 1 1 1 k 10 20 30 20 re f");
    let profile =
        std::env::temp_dir().join(format!("pdfcer_tac_profile_{}.json", std::process::id()));
    std::fs::write(
        &profile,
        br#"{"name": "tac", "rules": [{"id": "tac-300", "severity": "error",
            "check": "total-area-coverage", "limit_percent": 300, "ppi": 72}]}"#,
    )
    .unwrap();
    let (code, out) = run(&[
        "preflight".as_ref(),
        input.as_os_str(),
        "--profile".as_ref(),
        profile.as_os_str(),
    ]);
    assert_eq!(code, Some(18), "{out}");
    assert!(out.contains("tac-300") && out.contains("400.0%"), "{out}");
}
