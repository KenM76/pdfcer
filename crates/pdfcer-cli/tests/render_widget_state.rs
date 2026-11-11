//! `render-page --widget-state normal|rollover|down`: a push button with only
//! an `/N` appearance asked for `down` is drawn from `/N`, the fallback is on
//! the stable line and stderr says so; the default asks nothing of it.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A one-page push button whose only appearance is a red `/N`.
fn normal_only_button_pdf() -> Vec<u8> {
    let ap = "1 0 0 rg 0 0 200 40 re f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
         /Annots [4 0 R] >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Widget /F 4 /FT /Btn /Ff 65536 /T (go) /H /P \
         /Rect [20 220 220 260] /P 3 0 R /AP << /N 5 0 R >> >>"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 200 40] /Length {} >>\nstream\n{ap}\nendstream",
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
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-widget-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

fn render(pdf: &std::path::Path, png: &std::path::Path, extra: &[&str]) -> Output {
    Command::new(BIN)
        .args([
            "render-page",
            pdf.to_str().unwrap(),
            "-o",
            png.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .expect("could not spawn pdfcer")
}

fn metric(out: &Output, key: &str) -> u64 {
    let line = String::from_utf8(out.stdout.clone()).expect("utf-8");
    line.trim_end()
        .split("; ")
        .nth(1)
        .expect("metrics half")
        .split(' ')
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key} in {line}"))
        .parse()
        .expect("an integer")
}

#[test]
fn down_on_a_normal_only_button_reports_the_fallback() {
    let pdf = scratch("normal-only.pdf");
    std::fs::write(&pdf, normal_only_button_pdf()).expect("write fixture");
    let png = scratch("down.png");
    let out = render(&pdf, &png, &["--widget-state", "down"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(metric(&out, "annots_state_fallback"), 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--widget-state"), "{stderr}");
}

#[test]
fn the_default_state_asks_nothing_of_the_button() {
    let pdf = scratch("normal-only-default.pdf");
    std::fs::write(&pdf, normal_only_button_pdf()).expect("write fixture");
    let png = scratch("normal.png");
    let out = render(&pdf, &png, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(metric(&out, "annots_state_fallback"), 0);
    let rollover = render(&pdf, &png, &["--widget-state", "rollover"]);
    assert_eq!(metric(&rollover, "annots_state_fallback"), 1);
}
