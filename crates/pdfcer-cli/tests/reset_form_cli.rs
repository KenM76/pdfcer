//! `pdfcer reset-form`: the dry-run report, the applied summary and the
//! signature caveat, on one synthetic form exercising every row kind.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `Def` filled with a default, `Bare` filled without one, `Same` already at
/// its default, `Ro` read-only, `Sig` a signed signature field.
fn form_pdf() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
/Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R] >>",
        "<< /FT /Tx /T (Def) /V (typed) /DV (start) /Type /Annot /Subtype /Widget \
/Rect [10 10 90 30] /P 3 0 R >>",
        "<< /FT /Tx /T (Bare) /V (typed) /Type /Annot /Subtype /Widget \
/Rect [10 40 90 60] /P 3 0 R >>",
        "<< /FT /Tx /T (Same) /V (start) /DV (start) /Type /Annot /Subtype /Widget \
/Rect [10 70 90 90] /P 3 0 R >>",
        "<< /FT /Tx /Ff 1 /T (Ro) /V (typed) /Type /Annot /Subtype /Widget \
/Rect [10 100 90 120] /P 3 0 R >>",
        "<< /FT /Sig /T (Sig) /V << /Type /Sig >> /Type /Annot /Subtype /Widget \
/Rect [10 130 90 150] /P 3 0 R >>",
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
    std::env::temp_dir().join(format!("pdfcer_reset_cli_{tag}_{}.pdf", std::process::id()))
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
fn a_dry_run_reports_every_row_kind_and_writes_nothing() {
    let input = scratch("dry_in");
    std::fs::write(&input, form_pdf()).unwrap();
    let out = run(&["reset-form", input.to_str().unwrap()]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in [
        "reset  field=\"Def\" from=\"typed\" to=\"start\" source=default",
        "reset  field=\"Bare\" from=\"typed\" to=<removed> source=none",
        "ok     field=\"Same\" reason=already_default", // string-gap-exempt: the aligned status column
        "skip   field=\"Ro\" reason=read_only", // string-gap-exempt: the aligned status column
        "skip   field=\"Sig\" reason=signature", // string-gap-exempt: the aligned status column
    ] {
        assert!(stdout.lines().any(|l| l == line), "{line}\n{stdout}");
    }
    assert!(
        stdout.contains(" reset=2 defaulted=1 removed=1 skipped=2 applied=0"),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing was written"));
    let _ = std::fs::remove_file(&input);
}

#[test]
fn an_applied_reset_summarises_and_names_the_signature_it_kept() {
    let input = scratch("apply_in");
    let output = scratch("apply_out");
    std::fs::write(&input, form_pdf()).unwrap();
    let out = run(&[
        "reset-form",
        input.to_str().unwrap(),
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(" reset=2 defaulted=1 removed=1 widgets=3 skipped=2 applied=2"),
        "{stdout}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("1 signature field(s) were left alone"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listing =
        String::from_utf8(run(&["list-fields", output.to_str().unwrap()]).stdout).unwrap();
    assert!(
        listing
            .lines()
            .any(|l| l.starts_with("field name=\"Def\" ") && l.contains(" value=\"start\" ")),
        "{listing}"
    );
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}
