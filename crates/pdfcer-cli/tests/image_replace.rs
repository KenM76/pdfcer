//! `pdfcer replace-image` (pdfcer-gui request G156).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::vector::{Matrix, VectorObject, decompose_page};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Objects: 0 a path, 1 a 1 × 1 image drawn 60 × 40 at 10,20.
fn input(tag: &str) -> PathBuf {
    let page = "0 0 m 10 10 l S\nq 60 0 0 40 10 20 cm /Im1 Do Q\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /XObject \
         << /Im1 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 \
         /ColorSpace /DeviceGray /Length 1 >>\nstream\nA\nendstream"
            .to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_image_replace_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn picture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images")
        .join(name)
}

fn replace(input: &Path, output: &Path, object: &str, image: &Path, extra: &[&str]) -> Output {
    Command::new(BIN)
        .arg("replace-image")
        .arg(input)
        .args(["--page", "1", "--object", object, "--image"])
        .arg(image)
        .args(extra)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn objects(path: &Path) -> Vec<VectorObject> {
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .unwrap()
        .objects
}

#[test]
fn replace_image_keeps_the_placement() {
    let input = input("ok");
    let output = input.with_extension("out.pdf");
    let out = replace(&input, &output, "1", &picture("rgb8.png"), &["--stretch"]);
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("replaced=5"), "{stdout}");
    let objs = objects(&output);
    assert_eq!(objs.len(), 2);
    let VectorObject::Image(img) = &objs[1] else {
        panic!("{:?}", objs[1])
    };
    assert_eq!(img.pixel_size, Some((6, 4)));
    assert_eq!(
        (img.ctm.a, img.ctm.d, img.ctm.e, img.ctm.f),
        (60.0, 40.0, 10.0, 20.0)
    );

    let contained = input.with_extension("contain.pdf");
    let out = replace(&input, &contained, "1", &picture("icon32.png"), &[]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("letterboxed=1"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("CENTRED"));
}

#[test]
fn a_path_or_a_bad_picture_is_refused_without_output() {
    let input = input("bad");
    let output = input.with_extension("bad_out.pdf");
    for (object, image) in [
        ("0", picture("rgb8.png")),
        ("1", picture("not-an-image.bin")),
        ("1", picture("missing.png")),
    ] {
        let out = replace(&input, &output, object, &image, &[]);
        assert!(!out.status.success(), "{object} {image:?} succeeded");
        assert!(!output.exists(), "{object} {image:?} wrote output");
    }
}
