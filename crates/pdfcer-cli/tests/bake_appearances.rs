//! `pdfcer bake-appearances` (decision 203, opt-in half; pdfcer-gui
//! request G236).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// One page: an `/AP`-less Ink (bakeable), a pdf dimension (declined) and
/// a FreeText (bakeable, with a substituted font to report).
fn fixture(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [4 0 R 5 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /Ink /C [1 0 0] /InkList [[50 50 250 250]] \
         /Rect [40 40 260 260] >>",
        "<< /Type /Annot /Subtype /Line /C [0 0 0] /L [50 150 250 150] /LL 10 \
         /Rect [40 140 260 160] >>",
        "<< /Type /Annot /Subtype /FreeText /Contents (Hi) /DA (/Helv 12 Tf 0 g) \
         /Rect [40 40 260 120] >>",
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
        "pdfcer_bake_appearances_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

#[test]
fn bakes_what_it_can_and_names_what_it_skipped() {
    let input = fixture("mixed");
    let output = input.with_extension("out.pdf");
    let out = Command::new(BIN)
        .arg("bake-appearances")
        .arg(&input)
        .args(["--free-text-c", "border", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("baked 4 0 R /Ink ap=7 0 R"), "{stdout}");
    assert!(stdout.contains("skipped 5 0 R /Line: ") && stdout.contains("pdf dimension"));
    let free_text = stdout
        .lines()
        .find(|l| l.starts_with("baked 6 0 R"))
        .unwrap();
    assert!(
        free_text.contains("Helvetica"),
        "the substitution is printed"
    );
    assert!(stdout.contains("baked=2 skipped=1"), "{stdout}");
    let saved = std::fs::read(&output).unwrap();
    let original = std::fs::read(&input).unwrap();
    assert!(saved.starts_with(&original), "an incremental append");
    let tail = String::from_utf8_lossy(&saved[original.len()..]).into_owned();
    assert!(tail.contains("/AP") && !tail.contains("5 0 obj"));
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}
