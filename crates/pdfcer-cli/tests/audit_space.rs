//! `pdfcer audit-space`: where a file's bytes go, and a before/after diff.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn write_pdf(tag: &str, image_side: usize) -> PathBuf {
    let pixels = vec![0x80u8; image_side * image_side];
    let content = b"q 72 0 0 72 0 0 cm /Im1 Do Q";
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>".to_vec(),
    ];
    let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    s.extend_from_slice(content);
    s.extend_from_slice(b"\nendstream");
    bodies.push(s);
    let mut img = format!(
        "<< /Type /XObject /Subtype /Image /Width {image_side} /Height {image_side} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
        pixels.len()
    )
    .into_bytes();
    img.extend_from_slice(&pixels);
    img.extend_from_slice(b"\nendstream");
    bodies.push(img);
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let path = std::env::temp_dir().join(format!("pdfcer-audit-space-{tag}.pdf"));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&std::ffi::OsStr]) -> String {
    let out = Command::new(BIN)
        .arg("audit-space")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn reports_categories_and_image_resolution() {
    let path = write_pdf("report", 300);
    let text = run(&[path.as_os_str()]);
    assert!(text.contains("images"), "{text}");
    assert!(text.contains("300x300 DeviceGray 8bpc"), "{text}");
    assert!(text.contains("p1 72x72pt 300ppi"), "{text}");
    let json = run(&[path.as_os_str(), "--json".as_ref()]);
    assert!(
        json.contains("\"category\":\"images\",\"bytes\":"),
        "{json}"
    );
    assert!(json.contains("\"ppi_x\":300.00"), "{json}");
    assert!(
        json.contains("\"subtype\":\"Image\",\"pages\":[1]"),
        "{json}"
    );
    assert!(text.contains("/XObject /Image  pages 1"), "{text}");
}

#[test]
fn diff_shows_the_category_that_shrank() {
    let big = write_pdf("big", 300);
    let small = write_pdf("small", 100);
    let text = run(&[big.as_os_str(), "--diff".as_ref(), small.as_os_str()]);
    let images = text.lines().find(|l| l.starts_with("images")).unwrap();
    assert!(images.trim_end().ends_with("-80000"), "{text}");
}
