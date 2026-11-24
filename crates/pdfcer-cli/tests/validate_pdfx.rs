//! `pdfcer validate-pdfx` names each finding's clause, exits 18 on a failing
//! file and 0 on one left only to a person; the same rows run through
//! `preflight --profile pdfx-4`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn hello() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN).args(args).output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

/// A synthetic one-page PDF/X-4 file: CMYK fill, TrimBox, a GTS_PDFX
/// output intent with a printer profile, XMP and Info identification.
fn good_x4() -> PathBuf {
    let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" pdfxid:GTS_PDFXVersion="PDF/X-4"/></rdf:RDF></x:xmpmeta>"#;
    let stream = |dict: &str, data: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /OutputIntents [6 0 R] /Metadata 8 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /TrimBox [10 10 602 782] \
         /Resources << >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", "0 0 0 1 k 0 0 10 10 re f"),
        "<< /GTS_PDFXVersion (PDF/X-4) /Title (t) /CreationDate (D:20260101) \
         /ModDate (D:20260101) /Trapped /False >>"
            .to_owned(),
        "<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (FOGRA39) \
         /DestOutputProfile 7 0 R >>"
            .to_owned(),
        stream("", "000000000000prtr0000"),
        stream("/Type /Metadata /Subtype /XML", xmp),
    ];
    let mut buf = b"%PDF-1.6\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    let path =
        std::env::temp_dir().join(format!("pdfcer_validate_pdfx_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

#[test]
fn a_file_with_no_output_intent_fails_and_names_the_clause() {
    let (code, out) = run(&["validate-pdfx", hello().to_str().unwrap(), "--level", "x-4"]);
    assert_eq!(code, 18, "{out}");
    let header = out.lines().next().unwrap();
    assert!(header.starts_with("validate-pdfx "), "{out}");
    assert!(
        header.contains(" level=x-4 claim=\"-\" verdict=fails "),
        "{out}"
    );
    assert!(
        out.lines().any(
            |l| l.starts_with("error violation rule=PXQ-OI-1 page=- target=document")
                && l.contains("clause=\"ISO 15930-7:2010 §6.4\"")
        ),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.contains("rule=PXQ-FT-1") && l.contains("target=font:")),
        "{out}"
    );
}

#[test]
fn a_file_left_only_to_a_person_exits_zero_unconfirmed() {
    let file = good_x4();
    let (code, out) = run(&["validate-pdfx", file.to_str().unwrap(), "--level", "x-4"]);
    std::fs::remove_file(&file).ok();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.lines()
            .next()
            .unwrap()
            .contains("claim=\"PDF/X-4\" verdict=unconfirmed"),
        "{out}"
    );
    assert!(!out.lines().any(|l| l.starts_with("error ")), "{out}");
}

#[test]
fn json_carries_the_same_findings() {
    let (code, out) = run(&[
        "validate-pdfx",
        hello().to_str().unwrap(),
        "--level",
        "x-1a:2001",
        "--json",
    ]);
    assert_eq!(code, 18, "{out}");
    assert!(
        out.starts_with('{') && out.trim_end().ends_with('}'),
        "{out}"
    );
    assert!(out.contains("\"level\": \"x-1a:2001\""), "{out}");
    assert!(out.contains("\"verdict\": \"fails\""), "{out}");
    assert!(out.contains("\"rule\": \"PXQ-FT-1\""), "{out}");
    assert!(
        out.contains("\"clause\": \"ISO 15930-1:2001 §6.2\""),
        "{out}"
    );
}

#[test]
fn an_unknown_level_is_a_usage_error() {
    let out = Command::new(BIN)
        .args(["validate-pdfx", hello().to_str().unwrap(), "--level", "x-2"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a PDF/X level"));
}

#[test]
fn the_levels_are_preflight_profiles_too() {
    let (code, out) = run(&[
        "preflight",
        hello().to_str().unwrap(),
        "--profile",
        "pdfx-4",
    ]);
    assert_eq!(code, 18, "{out}");
    assert!(
        out.starts_with("preflight ") && out.contains("profile=\"PDF/X-4\""),
        "{out}"
    );
    assert!(out.lines().any(|l| l.contains("rule=PXQ-OI-1")), "{out}");
    let (code, json) = run(&["preflight", "--profile", "pdfx-1a-2003", "--dump-profile"]);
    assert_eq!(code, 0);
    assert!(json.contains("\"check\": \"pdfx-requirement\""), "{json}");
}
