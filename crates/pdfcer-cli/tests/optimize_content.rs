//! `pdfcer optimize --content`: lossless content-stream compaction, its
//! report and JSON, per-object control, and an unchanged rendering.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-optcontent-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One page whose content (object 4) repeats a fill colour, an identity
/// `cm` and long numbers, and draws form 5, which is the same.
fn write_pdf(dir: &Path) -> PathBuf {
    let page =
        b"q\n0.500 g\n0.500 g\n1 0 0 1 0 0 cm\n10.000 10.000 80.000 80.000 re f\nQ\n/Fm0 Do\n"
            .repeat(40);
    let form = b"0.250 G\n0.250 G\n2.0 w 0.0 0.0 m 100.0 100.0 l S\n".repeat(40);
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << /XObject << /Fm0 5 0 R >> >> >>".to_vec(),
    ];
    for (dict, data) in [
        (String::new(), &page[..]),
        (
            "/Type /XObject /Subtype /Form /BBox [0 0 100 100] ".to_owned(),
            &form[..],
        ),
    ] {
        let mut s = format!("<< {dict}/Length {} >>\nstream\n", data.len()).into_bytes();
        s.extend_from_slice(data);
        s.extend_from_slice(b"\nendstream");
        bodies.push(s);
    }
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
    let path = dir.join("in.pdf");
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    let o = Command::new(BIN).args(args).output().unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn render(pdf: &Path, png: &Path) -> Vec<u8> {
    run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    let decoder = png::Decoder::new(std::fs::File::open(png).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut pixels).unwrap();
    pixels
}

/// The decoded data of every stream in `pdf`, by way of `export-structure`.
fn decoded(pdf: &Path, d: &Path) -> String {
    let qdf = d.join("qdf.pdf");
    run(&[
        "export-structure",
        pdf.to_str().unwrap(),
        "-o",
        qdf.to_str().unwrap(),
    ]);
    String::from_utf8_lossy(&std::fs::read(qdf).unwrap()).into_owned()
}

#[test]
fn content_compacts_page_and_form_and_renders_the_same() {
    let d = dir("basic");
    let input = write_pdf(&d);
    let out = d.join("out.pdf");
    let o = run(&[
        "optimize",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--preset",
        "lossless",
        "--content",
    ]);
    let report = text(&o);
    assert!(report.contains("content(numbers=shortest"), "{report}");
    // Page: per repeat the second `g` and the identity `cm` (`Q` restores
    // the colour). Form: the second `G`, then every later `G` and `w` too,
    // since nothing between repeats changes them: 1 + 3 x 39.
    assert!(report.contains("80 operator(s) removed"), "{report}");
    assert!(report.contains("118 operator(s) removed"), "{report}");
    let qdf = decoded(&out, &d);
    assert!(
        // `Q/Fm0`: a name starts with a delimiter, so needs no separator.
        qdf.contains("q\n.5 g\n10 10 80 80 re\nf\nQ/Fm0 Do\nq\n"),
        "{qdf}"
    );
    assert!(qdf.contains(".25 G\n2 w\n0 0 m\n100 100 l\nS\n"), "{qdf}");
    assert_eq!(
        render(&input, &d.join("a.png")),
        render(&out, &d.join("b.png"))
    );
}

#[test]
fn content_json_and_per_object_control() {
    let d = dir("json");
    let input = write_pdf(&d);
    let o = run(&[
        "optimize",
        input.to_str().unwrap(),
        "--dry-run",
        "--json",
        "--preset",
        "lossless",
        "--content",
        "whitespace=keep",
        "--object",
        "5=leave",
    ]);
    let json = text(&o);
    assert!(
        json.contains("\"action\":\"content(numbers=shortest,redundant=on,whitespace=keep)\""),
        "{json}"
    );
    assert!(json.contains("\"operators_removed\":80"), "{json}");
    assert!(json.contains("\"encoded_as\":\"recompress-9\""), "{json}");
    assert!(!json.contains("\"id\":\"5 0\""), "{json}");
}

#[test]
fn a_bad_content_spec_is_a_usage_error() {
    let d = dir("bad");
    let input = write_pdf(&d);
    let o = Command::new(BIN)
        .args([
            "optimize",
            input.to_str().unwrap(),
            "--dry-run",
            "--content",
            "numbers=round",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("keep or shortest"));
}
