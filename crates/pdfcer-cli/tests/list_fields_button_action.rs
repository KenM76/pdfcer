//! `pdfcer list-fields` appends `action=` to every field line: what a push
//! button does when pressed (`EditSession::button_action`), `-` for any
//! other field.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// One page, three fields: `Plain` (text), `Script` (a push button whose
/// `/A` is a JavaScript action) and `Idle` (a push button with no `/A`).
fn form_pdf() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 5 0 R 6 0 R] >>",
        "<< /FT /Tx /T (Plain) /Type /Annot /Subtype /Widget /Rect [10 10 90 30] /P 3 0 R >>",
        "<< /FT /Btn /Ff 65536 /T (Script) /Type /Annot /Subtype /Widget /Rect [10 40 90 60] \
/P 3 0 R /A << /S /JavaScript /JS (app.alert(1)) >> >>",
        "<< /FT /Btn /Ff 65536 /T (Idle) /Type /Annot /Subtype /Widget /Rect [10 70 90 90] /P 3 0 R >>",
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
    std::env::temp_dir().join(format!(
        "pdfcer_button_action_{tag}_{}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> String {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn field_line<'a>(listing: &'a str, name: &str) -> &'a str {
    let key = format!("field name=\"{name}\" ");
    listing
        .lines()
        .find(|l| l.starts_with(&key))
        .unwrap_or_else(|| panic!("no line for {name}: {listing}"))
}

#[test]
fn each_push_button_states_its_action_and_other_fields_print_a_dash() {
    let input = scratch("in");
    std::fs::write(&input, form_pdf()).unwrap();
    let listing = run(&["list-fields", input.to_str().unwrap()]);
    assert!(
        field_line(&listing, "Plain").ends_with(" action=-"),
        "{listing}"
    );
    assert!(
        field_line(&listing, "Script").ends_with(" action=foreign:JavaScript"),
        "{listing}"
    );
    assert!(
        field_line(&listing, "Idle").ends_with(" action=none"),
        "{listing}"
    );
    let _ = std::fs::remove_file(&input);
}

#[test]
fn an_action_set_by_set_button_action_reads_back() {
    let input = scratch("set_in");
    let output = scratch("set_out");
    std::fs::write(&input, form_pdf()).unwrap();
    run(&[
        "set-button-action",
        input.to_str().unwrap(),
        "--name",
        "Idle",
        "--reset",
        "-o",
        output.to_str().unwrap(),
    ]);
    let listing = run(&["list-fields", output.to_str().unwrap()]);
    assert!(
        field_line(&listing, "Idle").ends_with(" action=ResetForm"),
        "{listing}"
    );
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}
