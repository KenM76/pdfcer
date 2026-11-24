//! `pdfcer convert-colours` converts RGB colours to a CMYK profile,
//! reports per source space and refuses a missing destination.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_convcol_{tag}_{}.pdf", std::process::id()))
}

/// One page with a DeviceRGB fill and stroke.
fn fixture(tag: &str) -> PathBuf {
    let content = "0.2 0.4 0.8 rg 10 10 50 40 re f 1 0 0 RG 5 5 20 20 re S\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 80] /Contents 4 0 R >>".to_owned(),
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
fn converts_reports_and_embeds_the_intent() {
    let input = fixture("in");
    let out = path("out");
    let profile = root().join("fixtures/synthetic/icc-rgb/dest-cmyk.icc");
    let run = pdfcer(&[
        "convert-colours",
        input.to_str().unwrap(),
        "--profile",
        profile.to_str().unwrap(),
        "--embed-intent",
        "GTS_PDFX",
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
    assert!(stdout.contains("source=\"DeviceRGB\" paints=2"), "{stdout}");
    assert!(stdout.contains("output-intent added"), "{stdout}");
    assert!(
        stdout.contains("note: DeviceRGB was read as sRGB"),
        "{stdout}"
    );
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    let before = std::fs::read(&input).unwrap();
    let after = std::fs::read(&out).unwrap();
    assert_eq!(&after[..before.len()], &before[..], "incremental");

    // The converted file has no RGB left, and its output intent is now the
    // destination.
    let again = path("again");
    let run = pdfcer(&[
        "convert-colours",
        out.to_str().unwrap(),
        "-o",
        again.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{stdout}");
    assert!(!stdout.contains("source=\"DeviceRGB\""), "{stdout}");
    assert!(stdout.contains("changed=0 "), "{stdout}");
    for p in [input, out, again] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn no_destination_is_refused() {
    let input = fixture("nodest");
    let out = path("nodest_out");
    let run = pdfcer(&[
        "convert-colours",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(!run.status.success());
    assert!(!out.exists());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("output intent"), "{stderr}");
    let _ = std::fs::remove_file(input);
}
