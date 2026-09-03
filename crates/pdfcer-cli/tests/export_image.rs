//! # `pdfcer export-image` integration tests (`Pass 248.0`)
//!
//! Black-box, against the real binary (`CARGO_BIN_EXE_pdfcer`), on the
//! process contract a script depends on: exit codes, the stable stdout
//! line, the refusals, and — because this verb exists for the bytes it
//! writes — the files themselves, decoded with an independent decoder.
//!
//! The renderer-level guarantees (alpha survives both compositing paths,
//! `pHYs` is written, JPEG flattens over the requested colour) are held in
//! `crates/pdfcer-render/tests/export_image.rs`. What THIS file protects is
//! the shell: that every flag is parsed **and reaches the engine**
//! (`feedback_a_shell_flag_can_be_parsed_and_never_used`), that a
//! multi-page run names its files so they sort, and that the refusals fire
//! before anything touches the disk.
//!
//! Fixtures are built inline (`docs/LEGAL.md` §5), same builder as
//! `render_page.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for num in 1..=objects.len() as u32 {
        let (_, off) = offsets.iter().find(|(n, _)| *n == num).unwrap();
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// N pages of 60 × 60 pt, each painting `content` with `/GS0` = `/ca 0.5`.
fn multipage_pdf(contents: &[&str]) -> Vec<u8> {
    let kids: Vec<String> = (0..contents.len())
        .map(|i| format!("{} 0 R", 3 + 2 * i))
        .collect();
    let mut objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 60 60] \
                 /Resources << /ExtGState << /GS0 << /ca 0.5 >> >> >> >>",
                kids.join(" "),
                contents.len()
            ),
        ),
    ];
    for (i, content) in contents.iter().enumerate() {
        let page_num = 3 + 2 * i as u32;
        let stream_num = page_num + 1;
        objects.push((
            page_num,
            format!("<< /Type /Page /Parent 2 0 R /Contents {stream_num} 0 R >>"),
        ));
        objects.push((
            stream_num,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ));
    }
    build_pdf(&objects)
}

const HALF_RED_SQUARE: &str = "/GS0 gs 1 0 0 rg 20 20 20 20 re f";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "pdfcer-test-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.join(name);
        std::fs::write(&p, bytes).expect("could not write fixture");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Decode a PNG file: `(width, height, rgba, pixels_per_metre)`.
fn decode_png(path: &std::path::Path) -> (u32, u32, Vec<u8>, Option<u32>) {
    let bytes = std::fs::read(path).expect("png exists");
    let decoder = png::Decoder::new(bytes.as_slice());
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let ppm = reader.info().pixel_dims.map(|d| d.xppu);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf, ppm)
}

fn rgba_at(w: u32, data: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

// ---------------------------------------------------------------------------

#[test]
fn transparent_png_reaches_the_file_and_the_stable_line_says_so() {
    let dir = TempDir::new("export-image-transparent");
    let input = dir.write("one.pdf", &multipage_pdf(&[HALF_RED_SQUARE]));
    let out = dir.join("one.png");
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--transparent",
        "--dpi",
        "72",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let line = stdout(&o);
    assert!(
        line.starts_with("exported "),
        "stable line prefix, got: {line}"
    );
    assert!(line.contains(" 60x60 format=png dpi=72 transparent=1 background=none; substituted="));
    // The counter half is render-page's, verbatim: spot-check its last key.
    assert!(line.contains(" overprint_process_images_unsupported=0"));
    assert!(stderr(&o).contains("transparency kept"));

    let (w, h, data, ppm) = decode_png(&out);
    assert_eq!((w, h), (60, 60));
    assert_eq!(ppm, Some(2835), "72 dpi -> 2835 px/m pHYs");
    assert_eq!(
        rgba_at(w, &data, 2, 2),
        [0, 0, 0, 0],
        "unpainted corner is transparent"
    );
    let c = rgba_at(w, &data, 30, 30);
    assert!((i32::from(c[3]) - 128).abs() <= 1, "half alpha, got {c:?}");
    assert!(c[0] >= 253, "straight (demultiplied) red, got {c:?}");
}

#[test]
fn the_default_is_paper_and_a_background_colour_is_honoured_for_png() {
    let dir = TempDir::new("export-image-bg");
    let input = dir.write("one.pdf", &multipage_pdf(&[HALF_RED_SQUARE]));

    let white = dir.join("white.png");
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "-o",
        white.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(stdout(&o).contains("transparent=0 background=#ffffff;"));
    let (w, _, data, ppm) = decode_png(&white);
    assert_eq!(ppm, Some(5906), "the 150 dpi default is written");
    assert_eq!(rgba_at(w, &data, 2, 2), [255, 255, 255, 255]);

    let blue = dir.join("blue.png");
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--background",
        "#0000ff",
        "-o",
        blue.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(stdout(&o).contains("transparent=0 background=#0000ff;"));
    let (w, _, data, _) = decode_png(&blue);
    assert_eq!(
        rgba_at(w, &data, 2, 2),
        [0, 0, 255, 255],
        "flattened onto blue"
    );
    let c = rgba_at(w, &data, w / 2, w / 2);
    assert_eq!(c[3], 255);
    assert!(
        (i32::from(c[0]) - 128).abs() <= 1 && (i32::from(c[2]) - 127).abs() <= 1,
        "{c:?}"
    );
}

#[test]
fn jpeg_is_written_with_its_density_and_quality_reaches_the_encoder() {
    let dir = TempDir::new("export-image-jpeg");
    let input = dir.write("one.pdf", &multipage_pdf(&[HALF_RED_SQUARE]));
    let hi = dir.join("hi.jpg");
    let lo = dir.join("lo.jpg");
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--format",
        "jpg",
        "--quality",
        "100",
        "--dpi",
        "300",
        "-o",
        hi.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(stdout(&o).contains(" 250x250 format=jpeg dpi=300 transparent=0 background=#ffffff;"));
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--format",
        "jpeg",
        "--quality",
        "10",
        "--dpi",
        "300",
        "-o",
        lo.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));

    let hi_bytes = std::fs::read(&hi).unwrap();
    let lo_bytes = std::fs::read(&lo).unwrap();
    assert_eq!(&hi_bytes[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
    assert_eq!(&hi_bytes[6..11], b"JFIF\0");
    assert_eq!(hi_bytes[13], 1, "density unit = dpi");
    assert_eq!(u16::from_be_bytes([hi_bytes[14], hi_bytes[15]]), 300);
    // The quality flag REACHED the encoder: a quality-10 file of the same
    // page is materially smaller than the quality-100 one.
    assert!(
        lo_bytes.len() * 2 < hi_bytes.len(),
        "quality 10 = {} bytes, quality 100 = {} bytes",
        lo_bytes.len(),
        hi_bytes.len()
    );
}

#[test]
fn a_multi_page_run_names_its_files_to_sort_and_prints_one_line_each() {
    let dir = TempDir::new("export-image-multi");
    let pages: Vec<&str> = (0..11).map(|_| HALF_RED_SQUARE).collect();
    let input = dir.write("many.pdf", &multipage_pdf(&pages));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--pages",
        "1,10-11",
        "--dpi",
        "36",
        "--output-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let lines: Vec<String> = stdout(&o).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    // Zero-padded to the widest page number (11 -> two digits).
    for name in ["many_p01.png", "many_p10.png", "many_p11.png"] {
        assert!(out_dir.join(name).is_file(), "missing {name}");
    }
    assert!(lines[0].contains(" page 1 -> ") && lines[0].contains("many_p01.png 30x30 "));
    assert!(lines[2].contains(" page 11 -> "));
}

#[test]
fn refusals_fire_before_anything_is_written() {
    let dir = TempDir::new("export-image-refusals");
    let input = dir.write(
        "one.pdf",
        &multipage_pdf(&[HALF_RED_SQUARE, HALF_RED_SQUARE]),
    );
    let out = dir.join("never.png");

    // JPEG cannot carry alpha: refused by name, not flattened.
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--format",
        "jpeg",
        "--transparent",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no alpha channel"), "{}", stderr(&o));
    assert!(!out.exists());

    // Two pages into one file.
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--pages",
        "all",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--output-dir"), "{}", stderr(&o));
    assert!(!out.exists());

    // Nowhere to write.
    let o = run(&["export-image", input.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("nowhere to write"), "{}", stderr(&o));

    // A background that is not a colour.
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--background",
        "red",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--background"), "{}", stderr(&o));
    assert!(!out.exists());

    // Quality out of range.
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--format",
        "jpeg",
        "--quality",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--quality"), "{}", stderr(&o));

    // `--transparent` and `--background` contradict; clap refuses (usage
    // exit code 2) before the program runs.
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--transparent",
        "--background",
        "#000000",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());
}

#[test]
fn svg_is_written_with_its_own_disclosure_line() {
    let dir = TempDir::new("export-image-svg");
    let input = dir.write("one.pdf", &multipage_pdf(&[HALF_RED_SQUARE]));
    let out = dir.join("one.svg");
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--format",
        "svg",
        "--dpi",
        "72",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let lines: Vec<String> = stdout(&o).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    // Same prefix and counter half as a raster export; WxH is the
    // recording grid at --dpi (60 pt at 72 dpi = 60 px).
    assert!(
        lines[0].contains(" 60x60 format=svg dpi=72 transparent=1 background=none; substituted=")
    );
    assert!(lines[0].contains(" overprint_process_images_unsupported=0"));
    // The SVG-only second line, prefixed, and exact for a plain fill.
    assert!(
        lines[1].starts_with("svg: ops=1 images=0 dashed_pre_applied=0 blend_modes=0 "),
        "{}",
        lines[1]
    );
    assert!(lines[1].ends_with(" exact=1"), "{}", lines[1]);
    assert!(stderr(&o).contains("glyph OUTLINES"));

    let svg = std::fs::read_to_string(&out).unwrap();
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains(r#"width="60pt" height="60pt" viewBox="0 0 60 60""#));
    assert!(
        svg.contains(r#"fill="rgb(255,0,0)" fill-opacity="0.502""#),
        "{svg}"
    );
    assert!(svg.trim_end().ends_with("</svg>"));

    // `--background` becomes the first element and flips the stable line.
    let out2 = dir.join("two.svg");
    let o = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--format",
        "svg",
        "--background",
        "#00ff00",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(stdout(&o).contains("transparent=0 background=#00ff00;"));
    let svg = std::fs::read_to_string(&out2).unwrap();
    assert!(svg.lines().nth(1).unwrap().starts_with("<rect "), "{svg}");
}

#[test]
fn render_flags_reach_the_engine() {
    // `--no-annotations` must change the counters on the line, which is
    // the cheapest proof the flag is wired rather than merely parsed.
    let dir = TempDir::new("export-image-flags");
    let annot_pdf = build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] /Resources << >> >>"
                .into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Annots [5 0 R] >>".into(),
        ),
        (
            4,
            "<< /Length 0 >>\nstream\n\nendstream".into(),
        ),
        (
            5,
            "<< /Type /Annot /Subtype /Square /Rect [10 10 50 50] /F 4 /AP << /N 6 0 R >> >>"
                .into(),
        ),
        (
            6,
            "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] /Length 24 >>\nstream\n0 0 1 rg 0 0 40 40 re f\nendstream"
                .into(),
        ),
    ]);
    let input = dir.write("annot.pdf", &annot_pdf);
    let out = dir.join("a.png");
    let with = run(&[
        "export-image",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(with.status.code(), Some(0), "stderr: {}", stderr(&with));
    assert!(
        stdout(&with).contains(" annots=1 annots_painted=1 "),
        "{}",
        stdout(&with)
    );
    let without = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--no-annotations",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(without.status.code(), Some(0));
    assert!(
        stdout(&without).contains(" annots=1 annots_painted=0 "),
        "{}",
        stdout(&without)
    );
}
