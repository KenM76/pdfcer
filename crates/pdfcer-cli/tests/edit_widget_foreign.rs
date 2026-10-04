//! `edit-widget --foreign-appearance`: a check box whose artwork
//! another producer drew is redrawn only when asked, and the run says so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_foreign_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn stream(content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// A one-page form with a check box `cb` whose `/AP` pdfcer did not draw.
fn foreign_check_box() -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /DA (/Helv 10 Tf 0 g) >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        "<< /FT /Btn /T (cb) /V /Off /AS /Off /Type /Annot /Subtype /Widget /P 3 0 R /F 4 \
         /Rect [20 50 40 70] /MK << /BC [0 0 0] /CA (4) >> \
         /AP << /N << /Off 5 0 R /Yes 6 0 R >> >> >>"
            .to_owned(),
        stream("0 0 1 RG 3 w 1.5 1.5 17 17 re S"),
        stream("0 0 1 RG 3 w 1.5 1.5 17 17 re S 0 g 5 5 10 10 re f"),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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
    let path = temp_path("src");
    std::fs::write(&path, buf).unwrap();
    path
}

fn remove_border(extra: &[&str]) -> (Output, String, String) {
    let src = foreign_check_box();
    let out = temp_path("out");
    let mut args = vec![
        "edit-widget",
        src.to_str().unwrap(),
        "--name",
        "cb",
        "--border-width",
        "0",
        "--border-color",
        "unset",
        "--output",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let r = Command::new(BIN).args(&args).output().unwrap();
    let (so, se) = (
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
    );
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
    (r, so, se)
}

#[test]
fn the_flag_replaces_and_discloses() {
    let (r, out, err) = remove_border(&["--foreign-appearance", "replace"]);
    assert_eq!(r.status.code(), Some(0), "{out}{err}");
    assert!(out.contains("regenerated=1"), "{out}");
    assert!(out.contains("foreign_replaced=1"), "{out}");
    assert!(err.contains("was REPLACED with pdfcer's own"), "{err}");
}

#[test]
fn without_the_flag_the_artwork_is_kept_and_disclosed() {
    let (r, out, err) = remove_border(&[]);
    assert_eq!(r.status.code(), Some(0), "{out}{err}");
    assert!(out.contains("regenerated=0"), "{out}");
    assert!(out.contains("foreign_replaced=0"), "{out}");
    assert!(err.contains("which pdfcer did not draw"), "{err}");
    assert!(!err.contains("REPLACED"), "{err}");
}
