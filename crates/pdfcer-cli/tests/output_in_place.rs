//! Every CLI output is written through a temporary file renamed into place,
//! so `-o` may name the input and a failed write leaves nothing behind.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn one_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f\r\n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-output-in-place-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn leftover_temps(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[test]
fn an_output_naming_the_input_edits_it_in_place() {
    let dir = fresh_dir("same");
    let path = dir.join("a.pdf");
    let original = one_page_pdf();
    std::fs::write(&path, &original).unwrap();
    let p = path.to_str().unwrap();
    let o = Command::new(BIN)
        .args(["rotate", "--degrees", "90", "-o", p, p])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let after = std::fs::read(&path).unwrap();
    // Incremental: the original bytes are a prefix, and the new revision
    // carries the rotation.
    assert!(after.starts_with(&original));
    assert!(String::from_utf8_lossy(&after[original.len()..]).contains("/Rotate 90"));
    assert_eq!(leftover_temps(&dir), Vec::<String>::new());
}

#[test]
fn a_write_that_cannot_land_leaves_no_temporary_file() {
    let dir = fresh_dir("blocked");
    let input = dir.join("in.pdf");
    std::fs::write(&input, one_page_pdf()).unwrap();
    // The destination is a non-empty directory, so the final rename fails
    // after the temporary file has been written.
    let blocked = dir.join("out.pdf");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(blocked.join("keep.txt"), b"keep").unwrap();
    let o = Command::new(BIN)
        .args([
            "rotate",
            "--degrees",
            "90",
            "-o",
            blocked.to_str().unwrap(),
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert_eq!(std::fs::read(blocked.join("keep.txt")).unwrap(), b"keep");
    assert_eq!(leftover_temps(&dir), Vec::<String>::new());
}
