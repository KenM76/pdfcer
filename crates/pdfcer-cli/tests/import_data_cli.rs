//! `pdfcer import-data`: the data format is detected by content (XFDF, FDF,
//! CSV), the summary line counts what was applied, and skipped rich-text and
//! withheld password values are named on stderr.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `Name` plain text, `Pin` a password field, `Notes` a rich-text field.
fn form_pdf() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 5 0 R 6 0 R] >>",
        "<< /FT /Tx /T (Name) /Type /Annot /Subtype /Widget /Rect [10 10 90 30] /P 3 0 R >>",
        "<< /FT /Tx /Ff 8192 /T (Pin) /Type /Annot /Subtype /Widget /Rect [10 40 90 60] \
/P 3 0 R >>",
        "<< /FT /Tx /Ff 33554432 /T (Notes) /V (old) /RV (<body><p>old</p></body>) \
/Type /Annot /Subtype /Widget /Rect [10 70 90 90] /P 3 0 R >>",
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
    std::env::temp_dir().join(format!("pdfcer_import_cli_{tag}_{}", std::process::id()))
}

fn import(input: &Path, data: &Path, output: &Path) -> Output {
    Command::new(BIN)
        .args([
            "import-data",
            input.to_str().unwrap(),
            "--data",
            data.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .expect("the binary runs")
}

fn value_of(pdf: &Path, name: &str) -> String {
    let out = Command::new(BIN)
        .args(["list-fields", pdf.to_str().unwrap()])
        .output()
        .unwrap();
    let listing = String::from_utf8(out.stdout).unwrap();
    let key = format!("field name=\"{name}\" ");
    let line = listing
        .lines()
        .find(|l| l.starts_with(&key))
        .unwrap_or_else(|| panic!("no line for {name}: {listing}"));
    let v = line.split(" value=").nth(1).unwrap();
    v.split(" widgets=").next().unwrap().to_owned()
}

#[test]
fn each_data_format_is_detected_by_content_not_extension() {
    let input = scratch("fmt_in.pdf");
    std::fs::write(&input, form_pdf()).unwrap();
    let cases: [(&str, &[u8], &str); 3] = [
        (
            "xfdf.txt",
            b"<?xml version=\"1.0\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\"><fields>\
<field name=\"Name\"><value>Ann</value></field></fields></xfdf>\n",
            "\"Ann\"",
        ),
        (
            "fdf.dat",
            b"%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [<< /T (Name) /V (Bob) >>] >> >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n",
            "\"Bob\"",
        ),
        ("csv.xfdf", b"name,value\nName,Cy\n", "\"Cy\""),
    ];
    for (file, bytes, want) in cases {
        let data = scratch(file);
        let output = scratch(&format!("{file}.pdf"));
        std::fs::write(&data, bytes).unwrap();
        let out = import(&input, &data, &output);
        assert!(
            out.status.success(),
            "{file}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(
            stdout.contains(" applied=1 skipped=0 mode="),
            "{file}: {stdout}"
        );
        assert_eq!(value_of(&output, "Name"), want, "{file}");
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_file(&output);
    }
    let _ = std::fs::remove_file(&input);
}

#[test]
fn skipped_rich_text_and_withheld_passwords_are_named() {
    let input = scratch("skip_in.pdf");
    let data = scratch("skip.csv");
    let output = scratch("skip_out.pdf");
    std::fs::write(&input, form_pdf()).unwrap();
    std::fs::write(&data, b"name,value\nName,Dee\nPin,1234\nNotes,new\n").unwrap();
    let out = import(&input, &data, &output);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(" applied=2 skipped=1 mode="), "{stdout}");
    assert!(
        stderr.contains("1 rich-text field(s) were left untouched"),
        "{stderr}"
    );
    assert!(
        stderr.contains("1 password field(s) were drawn as asterisks"),
        "{stderr}"
    );
    assert_eq!(value_of(&output, "Notes"), "\"old\"");
    for p in [&input, &data, &output] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn unreadable_data_is_refused_without_writing() {
    let input = scratch("bad_in.pdf");
    let data = scratch("bad.csv");
    let output = scratch("bad_out.pdf");
    std::fs::write(&input, form_pdf()).unwrap();
    std::fs::write(&data, b"one,two,three\n").unwrap();
    let out = import(&input, &data, &output);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("bad.csv"),
        "the refusal names the data file"
    );
    assert!(!output.exists());
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&data);
}
