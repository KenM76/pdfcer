//! `pdfcer preflight --fix` runs the fixups a profile names, saves, checks
//! the saved file again, and writes the text report of what still stands.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A 0.1-wide stroke and a 1-wide one.
fn fixture() -> Vec<u8> {
    let page = "0.1 w 0 0 20 20 re S\n1 w 50 50 10 10 re S\n";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-preflight-fix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn profile(fixups: &str) -> PathBuf {
    let path = scratch(&format!("profile-{}.json", fixups.len()));
    let json = format!(
        "{{\"name\": \"thin\", \"rules\": [{{\"id\": \"hairline\", \"check\": \"stroke-below\", \
         \"min_width_pt\": 0.25, \"severity\": \"error\", \"fixups\": [{fixups}]}}]}}"
    );
    std::fs::write(&path, json).unwrap();
    path
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN)
        .arg("preflight")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn a_named_fixup_clears_the_rule_and_the_output_passes() {
    let input = scratch("in.pdf");
    std::fs::write(&input, fixture()).unwrap();
    let output = scratch("out.pdf");
    let report = scratch("report.txt");
    let prof = profile(r#"{"fixup": "fix-hairlines", "min_width_pt": 0.25}"#);
    let (code, out) = run(&[
        input.to_str().unwrap(),
        "--profile",
        prof.to_str().unwrap(),
        "--fix",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("fixup fix-hairlines rule hairline: applied (1 changed)"),
        "{out}"
    );
    assert!(out.contains("1 undo step(s)"), "{out}");
    assert!(out.contains("undo_verified=1 undo_identical=1"), "{out}");
    assert!(out.contains("findings=0 dropped=0"), "{out}");
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.starts_with("Preflight report: thin\n"), "{text}");
    assert!(text.contains("Result: passes"), "{text}");
}

#[test]
fn a_rule_naming_no_fixup_is_named_and_still_fails() {
    let input = scratch("in2.pdf");
    std::fs::write(&input, fixture()).unwrap();
    let output = scratch("out2.pdf");
    let prof = profile("");
    let (code, out) = run(&[
        input.to_str().unwrap(),
        "--profile",
        prof.to_str().unwrap(),
        "--fix",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code, 18, "{out}");
    assert!(out.contains("were not fixed: it names no fixup"), "{out}");
    assert!(
        out.contains("error violation rule=hairline page=1"),
        "{out}"
    );
    let (code, out) = run(&[
        input.to_str().unwrap(),
        "--profile",
        prof.to_str().unwrap(),
        "--fix",
        output.to_str().unwrap(),
        "--fix-suggested",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("fixup fix-hairlines"), "{out}");
}

#[test]
fn report_format_json_writes_a_report_a_program_can_read() {
    use pdfcer_core::ocr::json_lite::{Json, parse};
    let input = scratch("in-json.pdf");
    std::fs::write(&input, fixture()).unwrap();
    let report = scratch("report.json");
    let prof = profile("");
    let (code, out) = run(&[
        input.to_str().unwrap(),
        "--profile",
        prof.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
        "--report-format",
        "json",
    ]);
    assert_eq!(code, 18, "{out}");
    let json = parse(&std::fs::read(&report).unwrap()).expect("valid JSON");
    let s = |j: &Json, k: &str| j.get(k).and_then(Json::as_str).map(str::to_owned);
    assert_eq!(s(&json, "profile").as_deref(), Some("thin"));
    assert_eq!(s(&json, "verdict").as_deref(), Some("fails"));
    let findings = json.get("findings").and_then(Json::as_array).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(s(&findings[0], "rule").as_deref(), Some("hairline"));
    assert_eq!(findings[0].get("page").and_then(Json::as_f64), Some(1.0));
    let rules = json.get("rules").and_then(Json::as_array).unwrap();
    assert_eq!(
        s(&rules[0], "suggested_fixup").as_deref(),
        Some("fix-hairlines")
    );
}
