//! `pdfcer inks`, `alias-ink` and `spot-to-process`: the Ink Manager.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

const CONTENT: &str = "/S1 cs 0.5 scn 0 0 10 10 re f /R1 cs 1 scn 0 0 5 5 re f";

fn build_pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in objects {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// One page filling with spot `Gold` (alternate DeviceCMYK); `Rust` over
/// DeviceRGB also fills.
fn gold(tag: &str) -> PathBuf {
    let pdf = build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Contents 4 0 R \
             /Resources << /ColorSpace << /S1 5 0 R /R1 [/Separation /Rust /DeviceRGB \
             << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 0 0] /N 1 >>] >> >> >>"
                .into(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
                CONTENT.len() + 1
            ),
        ),
        (
            5,
            "[/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
             /C0 [0 0 0 0] /C1 [0 0.2 0.8 0.1] /N 1 >>]"
                .into(),
        ),
    ]);
    let path = std::env::temp_dir().join(format!("pdfcer_inkmgr_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, pdf).unwrap();
    path
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn inks_lists_each_spot_with_its_pages() {
    let pdf = gold("list");
    let (code, out, err) = run(&["inks", s(&pdf)]);
    std::fs::remove_file(&pdf).ok();
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("count=2"), "{out}");
    assert!(
        out.contains("spot \"Gold\" alternates=DeviceCMYK pages=1 uses=1"),
        "{out}"
    );
    assert!(
        out.contains("spot \"Rust\" alternates=DeviceRGB pages=1 uses=1"),
        "{out}"
    );
}

#[test]
fn alias_ink_renames_and_the_listing_follows() {
    let pdf = gold("alias");
    let out_pdf = pdf.with_extension("out.pdf");
    let (code, out, err) = run(&[
        "alias-ink",
        s(&pdf),
        "--from",
        "Gold",
        "--to",
        "Silver",
        "-o",
        s(&out_pdf),
        "--verify-undo",
    ]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.starts_with("alias-ink "), "{out}");
    assert!(out.contains("spaces=1"), "{out}");
    assert!(out.contains("undo_verified=1"), "{out}");
    let (_, listed, _) = run(&["inks", s(&out_pdf)]);
    assert!(listed.contains("spot \"Silver\""), "{listed}");
    assert!(!listed.contains("\"Gold\""), "{listed}");
    let (code, _, err) = run(&[
        "alias-ink",
        s(&pdf),
        "--from",
        "Cyan",
        "--to",
        "Gold",
        "-o",
        s(&out_pdf),
    ]);
    std::fs::remove_file(&pdf).ok();
    std::fs::remove_file(&out_pdf).ok();
    assert_eq!(code, 9, "{err}");
    assert!(err.contains("process ink"), "{err}");
}

#[test]
fn spot_to_process_converts_and_reports_the_refusal() {
    let pdf = gold("s2p");
    let out_pdf = pdf.with_extension("out.pdf");
    let (code, out, err) = run(&["spot-to-process", s(&pdf), "-o", s(&out_pdf)]);
    let (_, listed, _) = run(&["inks", s(&out_pdf)]);
    std::fs::remove_file(&pdf).ok();
    std::fs::remove_file(&out_pdf).ok();
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("spaces=1") && out.contains("refused=1"),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("refused ink=\"Rust\"") && l.contains("icccer")),
        "{out}"
    );
    assert!(
        listed.contains("spot \"Gold\" alternates=DeviceCMYK pages=- uses=0"),
        "{listed}"
    );
}

#[test]
fn inks_says_when_the_list_is_incomplete() {
    let names: Vec<String> = (1..=33).map(|i| format!("/N{i:02}")).collect();
    let pdf = build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources \
                 << /ColorSpace << /D1 [/DeviceN [{}] /DeviceCMYK 4 0 R] >> >> >>",
                names.join(" ")
            ),
        ),
        (
            4,
            "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >>".into(),
        ),
    ]);
    let path = std::env::temp_dir().join(format!("pdfcer_inkmgr_cap_{}.pdf", std::process::id()));
    std::fs::write(&path, pdf).unwrap();
    let (code, out, err) = run(&["inks", s(&path)]);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("count=32 complete=0"), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("note \"") && l.contains("32 colorants")),
        "{out}"
    );
    assert!(!out.contains("\"N33\""), "{out}");
}
