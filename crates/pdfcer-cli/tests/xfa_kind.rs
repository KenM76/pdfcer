//! `pdfcer list-fields` reports which kind of XFA form a file carries, and
//! says in words when its page is a dynamic form's placeholder (G237).
//! Fixtures are assembled inline (docs/LEGAL.md §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn pdf(catalog_extra: &str) -> Vec<u8> {
    let objects = [
        format!(
            "<< /Type /Catalog /Pages 2 0 R {catalog_extra} \
/AcroForm << /Fields [] /XFA 4 0 R >> >>"
        ),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        "<< /Length 0 >>\nstream\n\nendstream".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn list_fields(name: &str, bytes: &[u8]) -> (String, String) {
    let path =
        std::env::temp_dir().join(format!("pdfcer-xfa-kind-{}-{name}.pdf", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new(BIN)
        .args(["list-fields", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "{out:?}");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_dynamic_form_is_named_and_its_placeholder_disclosed() {
    let (stdout, stderr) = list_fields("dynamic", &pdf("/NeedsRendering true"));
    assert!(stdout.contains("xfa_kind=dynamic"), "{stdout}");
    assert!(stderr.contains("placeholder"), "{stderr}");
}

#[test]
fn a_static_form_is_named_without_a_placeholder_warning() {
    let (stdout, stderr) = list_fields("static", &pdf(""));
    assert!(stdout.contains("xfa_kind=static"), "{stdout}");
    assert!(!stderr.contains("placeholder"), "{stderr}");
}
