//! `render-page --need-appearances honour|as-stored`: a form that sets
//! `/NeedAppearances true` has its text field drawn from `/V` by default,
//! the count is on the stable line and stderr says so; `as-stored` shows the
//! stored drawing. The input file is never changed.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A one-page form: a text field whose stored `/AP` is a red block and whose
/// value is "new".
fn stale_form_pdf() -> Vec<u8> {
    let ap = "1 0 0 rg 0 0 200 40 re f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /NeedAppearances true \
         /DA (/Helv 0 Tf 0 g) >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
         /Annots [4 0 R] >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Widget /F 4 /FT /Tx /T (name) /V (new) \
         /DA (/Helv 24 Tf 0 0 1 rg) /Rect [20 220 220 260] /P 3 0 R /AP << /N 5 0 R >> >>"
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
    let dir = std::env::temp_dir().join(format!("pdfcer-need-ap-{}", std::process::id()));
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
fn honour_is_the_default_and_as_stored_turns_it_off() {
    let original = stale_form_pdf();
    let pdf = scratch("form.pdf");
    std::fs::write(&pdf, &original).expect("write fixture");

    let out = render(&pdf, &scratch("honour.png"), &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(metric(&out, "annots_need_appearances_redrawn"), 1);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("drawn from their value"), "{err}");

    let explicit = render(
        &pdf,
        &scratch("explicit.png"),
        &["--need-appearances", "honour"],
    );
    assert_eq!(metric(&explicit, "annots_need_appearances_redrawn"), 1);

    let stored = render(
        &pdf,
        &scratch("stored.png"),
        &["--need-appearances", "as-stored"],
    );
    assert!(stored.status.success());
    assert_eq!(metric(&stored, "annots_need_appearances_redrawn"), 0);
    assert!(!String::from_utf8_lossy(&stored.stderr).contains("drawn from their value"));

    let honour_png = std::fs::read(scratch("honour.png")).expect("png");
    let stored_png = std::fs::read(scratch("stored.png")).expect("png");
    assert_ne!(
        honour_png, stored_png,
        "the flag changed nothing on the page"
    );
    assert_eq!(std::fs::read(&pdf).expect("reread"), original);
}
