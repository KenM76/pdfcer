//! `pdfcer print-ready` (pdfcer-gui request G273).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(tag: &str, first_annot_flags: u32) -> PathBuf {
    let ap = "0.11 g 0 0 10 10 re f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R 5 0 R] >>".to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Square /F {first_annot_flags} /Rect [0 0 10 10] \
             /AP << /N 6 0 R >> >>"
        ),
        "<< /Type /Annot /Subtype /Square /F 0 /Rect [0 0 10 10] /AP << /N 6 0 R >> >>".to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length {} >>\nstream\n{ap}\nendstream",
            ap.len()
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
    let path = std::env::temp_dir().join(format!(
        "pdfcer_print_ready_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

#[test]
fn writes_the_copy_and_prints_what_it_did() {
    let input = fixture("ok", 4);
    let output = input.with_extension("out.pdf");
    let before = std::fs::read(&input).unwrap();
    let out = Command::new(BIN)
        .arg("print-ready")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--free-text-c", "fill"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("burned=1 not_printing=1 lost=0"),
        "{stdout}"
    );
    assert!(
        stdout.contains("disclosure: 1 annotation(s) set not to print"),
        "{stdout}"
    );
    let saved = std::fs::read(&output).unwrap();
    let saved = String::from_utf8_lossy(&saved);
    assert!(
        saved.contains(" Do") && !saved.contains("/Annots"),
        "{saved}"
    );
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "the input is untouched"
    );
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn a_password_that_may_not_print_exits_edit_refused_and_writes_nothing() {
    let input = fixture("refused", 4);
    let locked = input.with_extension("locked.pdf");
    let encrypted = Command::new(BIN)
        .args(["encrypt"])
        .arg(&input)
        .arg(&locked)
        .args([
            "--user-password",
            "u",
            "--owner-password",
            "o",
            "--allow",
            "copy",
        ])
        .output()
        .unwrap();
    assert!(encrypted.status.success(), "{encrypted:?}");
    let output = input.with_extension("out.pdf");
    let out = Command::new(BIN)
        .args(["--open-password", "u", "print-ready"])
        .arg(&locked)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not allow printing"),
        "{out:?}"
    );
    assert!(!output.exists());
    for f in [input, locked] {
        let _ = std::fs::remove_file(f);
    }
}

/// `print` refuses before it looks for a printer, on the same permission.
#[cfg(windows)]
#[test]
fn print_refuses_a_password_that_may_not_print() {
    let input = fixture("print-refused", 5);
    let locked = input.with_extension("locked.pdf");
    let encrypted = Command::new(BIN)
        .args(["encrypt"])
        .arg(&input)
        .arg(&locked)
        .args([
            "--user-password",
            "u",
            "--owner-password",
            "o",
            "--allow",
            "copy",
        ])
        .output()
        .unwrap();
    assert!(encrypted.status.success(), "{encrypted:?}");
    let out = Command::new(BIN)
        .args(["--open-password", "u", "print"])
        .arg(&locked)
        .args(["--printer", "no-such-printer-pdfcer-test"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not allow printing"),
        "{out:?}"
    );
    for f in [input, locked] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn the_print_usage_flag_applies_a_layers_print_setting() {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R] /D << /AS \
         [<< /Event /Print /Category [/Print] /OCGs [4 0 R] >>] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
        "<< /Type /OCG /Name (L) /Usage << /Print << /PrintState /OFF >> >> >>",
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
    let input = std::env::temp_dir().join(format!("pdfcer_pr_layers_{}.pdf", std::process::id()));
    std::fs::write(&input, buf).unwrap();
    let output = input.with_extension("out.pdf");
    let run = |flag: Option<&str>| {
        let mut command = Command::new(BIN);
        command
            .arg("print-ready")
            .arg(&input)
            .arg("-o")
            .arg(&output);
        command.args(flag);
        let out = command.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let batch = run(None);
    assert!(batch.contains("not applied"), "{batch}");
    let viewer = run(Some("--apply-print-usage"));
    assert!(viewer.contains("differently"), "{viewer}");
    let written = std::fs::read(&output).unwrap();
    assert!(
        !written.windows(3).any(|w| w == b"/AS"),
        "usage is fixed in the copy"
    );
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}
