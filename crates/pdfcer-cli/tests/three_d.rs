//! `pdfcer 3d-list` / `3d-extract` on a synthetic page with one U3D `/3D`
//! annotation and one RichMedia PRC asset.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn three_d_pdf(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /3D /Rect [0 0 100 100] /3DD 5 0 R /AP << /N 5 0 R >> >>",
        "<< /Type /3D /Subtype /U3D /Filter /ASCIIHexDecode /VA [<< >>] /Length 13 >>\n\
         stream\n55334400C0FFEE>\nendstream",
        "<< /Type /Annot /Subtype /RichMedia /Rect [0 0 1 1] /RichMediaContent \
         << /Configurations [<< /Subtype /3D /Instances [<< /Subtype /3D /Asset 7 0 R >>] >>] >> >>",
        "<< /Type /Filespec /UF (../evil.u3d) /EF << /F 8 0 R >> >>",
        "<< /Type /EmbeddedFile /Length 4 >>\nstream\nPRC!\nendstream",
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
    let path = std::env::temp_dir().join(format!("pdfcer_3d_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

#[test]
fn both_models_are_listed() {
    let input = three_d_pdf("list");
    let out = run(&["3d-list", input.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("3d index=0 page=1 format=U3D views=1 poster=yes source=stream\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "3d index=1 page=1 format=U3D views=0 poster=no source=richmedia name=\"../evil.u3d\"\n"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("count=2\n"), "{stdout}");
}

#[test]
fn extraction_writes_the_decoded_bytes() {
    let input = three_d_pdf("ext");
    let output = input.with_extension("u3d");
    let out = run(&[
        "3d-extract",
        input.to_str().unwrap(),
        "--index",
        "0",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"U3D\0\xC0\xFF\xEE");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("note:"));
}

/// The asset's name says `.u3d`, its bytes say PRC: written anyway, and
/// said out loud.
#[test]
fn a_mislabelled_model_is_disclosed() {
    let input = three_d_pdf("lie");
    let output = input.with_extension("bin");
    let out = run(&[
        "3d-extract",
        input.to_str().unwrap(),
        "--index",
        "1",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"PRC!");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("note: the document declares U3D but the bytes are PRC"),
        "{stdout}"
    );
}

#[test]
fn an_out_of_range_index_is_refused_and_writes_nothing() {
    let input = three_d_pdf("oob");
    let output = input.with_extension("none");
    let out = run(&[
        "3d-extract",
        input.to_str().unwrap(),
        "--index",
        "5",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("has 2"));
}

fn model(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pdfcer_3d_{tag}_{}.bin", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Dry run: the summary, the inferred format and the version note are
/// printed, and nothing is written.
#[test]
fn an_embed_dry_run_discloses_the_inferred_format_and_the_version() {
    let input = three_d_pdf("emb_dry");
    let prc = model("emb_dry", b"PRC\x08\x00\x01");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        prc.to_str().unwrap(),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("format=PRC bytes=6 poster=placeholder activate=XA"),
        "{stdout}"
    );
    assert!(stdout.contains("applied=0"), "{stdout}");
    assert!(
        stdout.contains("inferred: format PRC from the file's signature\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("note: the document is PDF 1.7 and PRC needs PDF 2.0"),
        "{stdout}"
    );
    assert!(!output.exists(), "a dry run wrote a file");
}

/// Applied with a stated format: the model lists and extracts back from
/// the written file, and nothing is inferred or noted.
#[test]
fn an_applied_embed_round_trips_through_list_and_extract() {
    let input = three_d_pdf("emb_apply");
    let u3d = model("emb_apply", b"U3D\0\x10\x20\x30");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        u3d.to_str().unwrap(),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "--format",
        "u3d",
        "--activate",
        "page-visible",
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("activate=PV"), "{stdout}");
    assert!(!stdout.contains("inferred:"), "{stdout}");
    assert!(!stdout.contains("note:"), "{stdout}");

    let listed = run(&["3d-list", output.to_str().unwrap()]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("count=3\n"),
        "{}",
        String::from_utf8_lossy(&listed.stdout)
    );
    let extracted = input.with_extension("back.u3d");
    let ext = run(&[
        "3d-extract",
        output.to_str().unwrap(),
        "--index",
        "2",
        "-o",
        extracted.to_str().unwrap(),
    ]);
    assert!(
        ext.status.success(),
        "{}",
        String::from_utf8_lossy(&ext.stderr)
    );
    assert_eq!(std::fs::read(&extracted).unwrap(), b"U3D\0\x10\x20\x30");
}

#[test]
fn a_step_model_is_refused_and_writes_nothing() {
    let input = three_d_pdf("emb_step");
    let step = model("emb_step", b"ISO-10303-21;\nHEADER;");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        step.to_str().unwrap(),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("STEP"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
