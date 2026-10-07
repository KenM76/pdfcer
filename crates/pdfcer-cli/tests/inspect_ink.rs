//! `pdfcer inspect --ink`: whether each page composites in ink, without a
//! render, and where its blending space came from.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

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

/// Page 1 declares a CMYK group, page 2 an RGB group, page 3 nothing; the
/// catalog optionally carries a four-component output intent.
fn three_pages(tag: &str, cmyk_intent: bool) -> PathBuf {
    let intent = if cmyk_intent {
        " /OutputIntents [<< /Type /OutputIntent /S /GTS_PDFX /DestOutputProfile 7 0 R >>]"
    } else {
        ""
    };
    let page = |group: &str| {
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60] /Contents 6 0 R{group} >>")
    };
    let pdf = build_pdf(&[
        (1, format!("<< /Type /Catalog /Pages 2 0 R{intent} >>")),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".into(),
        ),
        (3, page(" /Group << /S /Transparency /CS /DeviceCMYK >>")),
        (4, page(" /Group << /S /Transparency /CS /DeviceRGB >>")),
        (5, page("")),
        (6, "<< /Length 0 >>\nstream\n\nendstream".into()),
        (7, "<< /N 4 /Length 4 >>\nstream\nicc!\nendstream".into()),
    ]);
    let path = std::env::temp_dir().join(format!("pdfcer_ink_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, pdf).unwrap();
    path
}

fn inspect(pdf: &PathBuf, extra: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(BIN)
        .arg("inspect")
        .arg(pdf)
        .arg("--ink")
        .args(extra)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn each_page_says_whether_it_composites_in_ink_and_why() {
    let pdf = three_pages("plain", false);
    let (code, out, err) = inspect(&pdf, &["--no-settings"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(out.contains("page=1 ink=1 source=page_group"), "{out}");
    assert!(out.contains("page=2 ink=0 source=page_group"), "{out}");
    assert!(out.contains("page=3 ink=0 source=device_native"), "{out}");
    assert!(
        out.contains("pages=3 in_ink=1 inferred_from_output_intent=0"),
        "{out}"
    );
    assert!(!err.contains("output intent"), "{err}");
    let _ = std::fs::remove_file(pdf);
}

#[test]
fn a_space_taken_from_the_output_intent_is_disclosed() {
    let pdf = three_pages("intent", true);
    let (code, out, err) = inspect(&pdf, &["--no-settings", "--pages", "3,1"]);
    assert_eq!(code, Some(0), "{err}");
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("page=")).collect();
    assert_eq!(
        lines,
        [
            "page=3 ink=1 source=output_intent",
            "page=1 ink=1 source=page_group"
        ]
    );
    assert!(
        out.contains("pages=2 in_ink=2 inferred_from_output_intent=1"),
        "{out}"
    );
    assert!(err.contains("page(s) 3 declare no blending space"), "{err}");
    let _ = std::fs::remove_file(pdf);
}
