//! `pdfcer list-metadata` / `remove-metadata` on a synthetic file whose
//! info dictionary sits inside an object stream: a full save must not leave
//! the old value in the container.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const SECRET: &str = "Ken Example";

/// 1 catalog, 2 pages, 3 page, 4 object stream holding 5 (the info
/// dictionary), 6 the uncompressed cross-reference stream.
fn sample() -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let info = format!("<< /Author ({SECRET}) >>");
    let objstm_data = format!("5 0 {info}");
    let plain = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        format!(
            "<< /Type /ObjStm /N 1 /First 4 /Length {} >>\nstream\n{objstm_data}\nendstream",
            objstm_data.len()
        ),
    ];
    for (i, body) in plain.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = out.len();
    let mut rows: Vec<[u8; 7]> = vec![[0, 0, 0, 0, 0, 0xff, 0xff]];
    for off in offsets.iter().copied().chain([xref_at]) {
        let b = u32::try_from(off).unwrap().to_be_bytes();
        rows.push([1, b[0], b[1], b[2], b[3], 0, 0]);
    }
    rows.insert(5, [2, 0, 0, 0, 4, 0, 0]);
    let data: Vec<u8> = rows.concat();
    out.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /XRef /Size 7 /W [1 4 2] /Root 1 0 R /Info 5 0 R /Length {} >>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&data);
    out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
    out
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_rmmeta_{tag}_{}.pdf", std::process::id()))
}

fn input(tag: &str) -> PathBuf {
    let path = scratch(tag);
    std::fs::write(&path, sample()).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn list_metadata_names_the_compressed_info_entry() {
    let path = input("list");
    let out = run(&["list-metadata", path.to_str().unwrap(), "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"id\":\"info/Author\""), "{stdout}");
    assert!(stdout.contains(SECRET), "{stdout}");
}

#[test]
fn a_dry_run_writes_nothing() {
    let path = input("dry");
    let out_path = scratch("dry_out");
    let _ = std::fs::remove_file(&out_path);
    let out = run(&[
        "remove-metadata",
        path.to_str().unwrap(),
        "--all",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("removed info/Author"));
    assert!(!out_path.exists());
}

#[test]
fn a_full_save_leaves_no_trace_inside_the_object_stream() {
    let path = input("full");
    let out_path = scratch("full_out");
    let out = run(&[
        "remove-metadata",
        path.to_str().unwrap(),
        "--kind",
        "info",
        "--apply",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&out_path).unwrap();
    assert!(!contains(&bytes, SECRET), "the old info value survived");
    let listed = run(&["list-metadata", out_path.to_str().unwrap()]);
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("info/"));
}

#[test]
fn an_incremental_save_warns_and_an_unknown_id_exits_nine() {
    let path = input("incr");
    let out_path = scratch("incr_out");
    let out = run(&[
        "remove-metadata",
        path.to_str().unwrap(),
        "--item",
        "info/Author",
        "--item",
        "info/Nope",
        "--mode",
        "incremental",
        "--apply",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found: info/Nope"), "{stderr}");
    assert!(stderr.contains("earlier revision"), "{stderr}");
    let bytes = std::fs::read(&out_path).unwrap();
    assert!(
        contains(&bytes, SECRET),
        "the base revision is kept verbatim"
    );
}

#[test]
fn naming_nothing_is_refused() {
    let path = input("none");
    let out = run(&["remove-metadata", path.to_str().unwrap()]);
    assert!(!out.status.success());
}
