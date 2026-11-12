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
                 /Resources << /ExtGState << /GS0 << /ca 0.5 >> /GS1 << /BM /Multiply >> >> >> >>",
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
/// Black ink under a yellow `/Multiply` mark: a highlight over text.
const HIGHLIGHT: &str = "0 g 20 20 20 20 re f /GS1 gs 1 1 0 rg 10 10 40 40 re f";

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
    assert!(lines[1].contains(" exact=1 "), "{}", lines[1]);
    assert!(
        lines[1].ends_with(" shadings_as_gradients=0 blends_flattened=0 blends_underlaid=0"),
        "{}",
        lines[1]
    );
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

#[test]
fn flatten_blends_bakes_a_highlight_into_the_svg() {
    let dir = TempDir::new("export-image-flatten");
    let input = dir.write("one.pdf", &multipage_pdf(&[HIGHLIGHT]));
    let out = dir.join("one.svg");
    let args = |extra: &[&'static str]| {
        let mut a = vec![
            "export-image".to_owned(),
            input.to_str().unwrap().to_owned(),
            "-o".to_owned(),
            out.to_str().unwrap().to_owned(),
        ];
        a.extend(extra.iter().map(|s| (*s).to_owned()));
        a
    };
    let plain = Command::new(BIN)
        .args(args(&["--format", "svg"]))
        .output()
        .unwrap();
    assert!(
        stdout(&plain).contains(" blends_flattened=0"),
        "{}",
        stdout(&plain)
    );
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("mix-blend-mode")
    );

    let flat = Command::new(BIN)
        .args(args(&["--format", "svg", "--flatten-blends"]))
        .output()
        .unwrap();
    assert_eq!(flat.status.code(), Some(0), "stderr: {}", stderr(&flat));
    assert!(
        stdout(&flat).contains(" blends_flattened=1 blends_underlaid=1"),
        "{}",
        stdout(&flat)
    );
    let svg = std::fs::read_to_string(&out).unwrap();
    assert!(!svg.contains("mix-blend-mode"));
    assert!(
        !svg.contains("<image"),
        "the highlight stays a shape: {svg}"
    );

    let raster = Command::new(BIN)
        .args(args(&[
            "--format",
            "svg",
            "--flatten-blends",
            "--raster-blends",
        ]))
        .output()
        .unwrap();
    assert_eq!(raster.status.code(), Some(0), "stderr: {}", stderr(&raster));
    assert!(
        stdout(&raster).contains(" blends_flattened=1 blends_underlaid=0"),
        "{}",
        stdout(&raster)
    );
    assert!(std::fs::read_to_string(&out).unwrap().contains("<image"));

    let png = Command::new(BIN)
        .args(args(&["--format", "png", "--flatten-blends"]))
        .output()
        .unwrap();
    assert_ne!(png.status.code(), Some(0));
    assert!(stderr(&png).contains("--flatten-blends applies to --format svg only"));
}

/// `--format jxl` (Pass 696.0): the lossless file decodes to exactly the
/// pixels the PNG export writes, `--transparent` keeps alpha, a lossy run
/// reports its error, and `--jxl-distance` is refused off JPEG XL.
#[cfg(feature = "jxl-write")]
#[test]
fn jxl_is_lossless_by_default_and_lossy_on_request() {
    use pdfcer_core::image_codec::jxl::decode_pixels;
    let dir = TempDir::new("export-image-jxl");
    let input = dir.write("one.pdf", &multipage_pdf(&[HALF_RED_SQUARE]));
    let (png, jxl) = (dir.join("p.png"), dir.join("p.jxl"));
    let i = input.to_str().unwrap();
    let o = run(&[
        "export-image",
        i,
        "--dpi",
        "72",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let o = run(&[
        "export-image",
        i,
        "--format",
        "jxl",
        "--dpi",
        "72",
        "-o",
        jxl.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(stdout(&o).contains(" format=jxl dpi=72 "), "{}", stdout(&o));
    let (w, h, rgba, _) = decode_png(&png);
    let back = decode_pixels(&std::fs::read(&jxl).unwrap()).unwrap();
    assert_eq!(
        (back.width, back.height, back.alpha.is_none()),
        (w, h, true)
    );
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, _]| [r, g, b])
        .collect();
    assert_eq!(back.colour, rgb, "lossless JPEG XL differs from the PNG");

    let o = run(&[
        "export-image",
        i,
        "--format",
        "jxl",
        "--transparent",
        "-o",
        jxl.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(
        decode_pixels(&std::fs::read(&jxl).unwrap())
            .unwrap()
            .alpha
            .is_some()
    );

    let o = run(&[
        "export-image",
        i,
        "--format",
        "jxl",
        "--jxl-distance",
        "2",
        "-o",
        jxl.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(
        stdout(&o).contains(" format=jxl max_error="),
        "{}",
        stdout(&o)
    );

    let o = run(&[
        "export-image",
        i,
        "--jxl-distance",
        "2",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_ne!(o.status.code(), Some(0));
    assert!(stderr(&o).contains("--jxl-distance applies to --format jxl only"));
    let o = run(&[
        "export-image",
        i,
        "--format",
        "jxl",
        "--jxl-distance",
        "99",
        "-o",
        jxl.to_str().unwrap(),
    ]);
    assert_ne!(o.status.code(), Some(0));
    assert!(stderr(&o).contains("outside 0.1..=25"));
}

/// `--patch-approximations` (Pass 700.12) reaches the EMF writer: the
/// overprint overlap becomes one bitmap, the `emf-patches:` line counts
/// it, the default run writes neither, and a raster format refuses it.
#[test]
fn patch_approximations_reaches_the_emf_writer() {
    let dir = TempDir::new("export-image-emf-patches");
    let content = "1 0 0 0 k 10 10 50 40 re f /OP gs 0 1 0 0 k 40 20 40 40 re f";
    let input = dir.write(
        "op.pdf",
        &build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
                 /Resources << /ExtGState << /OP << /OP true /op true /OPM 1 >> >> >> >>"
                    .into(),
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
            (
                4,
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len() + 1
                ),
            ),
        ]),
    );
    let out = dir.join("p.emf");
    let export = |extra: &[&str]| -> Output {
        let mut a = vec![
            "export-image",
            input.to_str().unwrap(),
            "--format",
            "emf",
            "--dpi",
            "144",
            "-o",
            out.to_str().unwrap(),
        ];
        a.extend_from_slice(extra);
        run(&a)
    };
    let plain = export(&[]);
    assert_eq!(plain.status.code(), Some(0), "stderr: {}", stderr(&plain));
    assert!(
        stdout(&plain).contains("emf: ops=2 rasters=0 "),
        "{}",
        stdout(&plain)
    );
    assert!(!stdout(&plain).contains("emf-patches:"));

    let patched = export(&["--patch-approximations"]);
    assert_eq!(
        patched.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&patched)
    );
    assert!(
        stdout(&patched).contains(
            "emf-patches: rasters=1 overprint=1 non_isolated_groups=0 blends=0 blending_space=0 transparency=0"
        ),
        "{}",
        stdout(&patched)
    );
    assert!(
        stdout(&patched).contains("emf: ops=2 rasters=1 "),
        "{}",
        stdout(&patched)
    );

    let png = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--patch-approximations",
        "-o",
        dir.join("p.png").to_str().unwrap(),
    ]);
    assert_ne!(png.status.code(), Some(0));
    assert!(stderr(&png).contains("--patch-approximations applies to --format emf only"));

    // --vector-transparency (Pass 700.13): the same overprint as regions.
    let vector = export(&["--vector-transparency"]);
    assert_eq!(vector.status.code(), Some(0), "stderr: {}", stderr(&vector));
    let text = stdout(&vector);
    assert!(
        text.contains(
            "emf-vector: ops=1 regions=2 alpha=0 groups=0 overprint=1 blends=0 patched_not_flat=0 patched_too_complex=0 patched_unrecorded=0"
        ),
        "{text}"
    );
    assert!(text.contains("emf-patches: rasters=0 "), "{text}");
    assert!(text.contains("emf: ops=3 rasters=0 "), "{text}");
    assert!(
        stderr(&vector).contains("1 transparent object(s) written as 2 opaque shape(s)"),
        "{}",
        stderr(&vector)
    );
    let png = run(&[
        "export-image",
        input.to_str().unwrap(),
        "--vector-transparency",
        "-o",
        dir.join("p.png").to_str().unwrap(),
    ]);
    assert_ne!(png.status.code(), Some(0));
    assert!(stderr(&png).contains("--vector-transparency applies to --format emf only"));
}

/// `--vector-shadings` (Pass 700.14) writes an axial shading as triangles
/// and counts it on the `emf-shadings:` line; the default run writes a
/// bitmap and no such line; a raster format refuses the flag.
#[test]
fn vector_shadings_reaches_the_emf_writer() {
    let dir = TempDir::new("export-image-emf-shadings");
    let content = "/Sh0 sh";
    let input = dir.write(
        "sh.pdf",
        &build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
                 /Resources << /Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB \
                 /Coords [20 0 80 0] /Extend [true true] /Function << /FunctionType 2 \
                 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >> >>"
                    .into(),
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
            (
                4,
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len() + 1
                ),
            ),
        ]),
    );
    let i = input.to_str().unwrap();
    let out = dir.join("p.emf");
    let o = out.to_str().unwrap();
    let export = |extra: &[&str]| -> Output {
        let mut a = vec!["export-image", i, "--format", "emf", "-o", o];
        a.extend_from_slice(extra);
        run(&a)
    };
    let plain = export(&[]);
    assert_eq!(plain.status.code(), Some(0), "stderr: {}", stderr(&plain));
    assert!(
        !stdout(&plain).contains("emf-shadings:"),
        "{}",
        stdout(&plain)
    );
    assert!(
        !stdout(&plain).contains("emf: ops=1 rasters=0 "),
        "{}",
        stdout(&plain)
    );

    let vector = export(&["--vector-shadings"]);
    assert_eq!(vector.status.code(), Some(0), "stderr: {}", stderr(&vector));
    let text = stdout(&vector);
    assert!(text.contains("emf-shadings: vector=1 triangles="), "{text}");
    assert!(
        text.contains("declined_translucent=0 declined_too_many=0 declined_other=0"),
        "{text}"
    );
    assert!(text.contains(" rasters=0 "), "{text}");

    let png = run(&[
        "export-image",
        i,
        "--vector-shadings",
        "-o",
        dir.join("p.png").to_str().unwrap(),
    ]);
    assert_ne!(png.status.code(), Some(0));
    assert!(stderr(&png).contains("--vector-shadings applies to --format emf only"));
}

fn one_page(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    dir.write(
        name,
        &build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] >>".into(),
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
            (
                4,
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len() + 1
                ),
            ),
        ]),
    )
}

/// `--emf-plates` (Pass 700.11) writes one metafile per ink and a line per
/// plate; `--emf-plates-strict` refuses an image page and writes nothing;
/// a raster format refuses the flag.
#[test]
fn emf_plates_writes_one_metafile_per_ink() {
    let dir = TempDir::new("export-image-emf-plates");
    let input = one_page(&dir, "k.pdf", "0.5 0 0 1 k 10 10 30 30 re f");
    let out = dir.join("p.emf");
    let i = input.to_str().unwrap();
    let o = out.to_str().unwrap();
    let emf = |input: &str, extra: &str, out: &str| -> Output {
        run(&[
            "export-image",
            input,
            "--format",
            "emf",
            "--dpi",
            "144",
            extra,
            "-o",
            out,
        ])
    };
    let r = emf(i, "--emf-plates", o);
    assert_eq!(r.status.code(), Some(0), "stderr: {}", stderr(&r));
    for (ink, ops) in [("Cyan", 1), ("Magenta", 0), ("Yellow", 0), ("Black", 1)] {
        let path = dir.join(&format!("p_{ink}.emf"));
        assert!(path.exists(), "{ink} plate written");
        let line = format!(
            "plate {ink} -> {} 200x160 format=emf dpi=144; plate: ops={ops} ",
            path.display()
        );
        assert!(stdout(&r).contains(&line), "{}", stdout(&r));
    }
    assert!(!out.exists(), "no page metafile beside the plates");

    let image = one_page(
        &dir,
        "img.pdf",
        "q 40 0 0 40 20 20 cm BI /W 2 /H 2 /CS /CMYK /BPC 8 /F /AHx ID \
         FF000000FF000000FF000000FF000000> EI Q",
    );
    let image = image.to_str().unwrap();
    let strict_out = dir.join("s.emf");
    let s = strict_out.to_str().unwrap();
    let r = emf(image, "--emf-plates-strict", s);
    assert_ne!(r.status.code(), Some(0));
    assert!(
        stderr(&r).contains("the Cyan plate needs pixels for image"),
        "{}",
        stderr(&r)
    );
    assert!(
        !dir.join("s_Cyan.emf").exists(),
        "a refused page writes nothing"
    );

    let r = emf(image, "--emf-plates", s);
    assert_eq!(r.status.code(), Some(0), "stderr: {}", stderr(&r));
    let cyan = stdout(&r)
        .lines()
        .find(|l| l.contains(" plate Cyan -> "))
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(
        cyan.ends_with(" images=1 shadings=0 soft_masks=0 transparency=0 rasters=1"),
        "{}",
        stdout(&r)
    );

    let png = run(&[
        "export-image",
        i,
        "--emf-plates",
        "-o",
        dir.join("p.png").to_str().unwrap(),
    ]);
    assert_ne!(png.status.code(), Some(0));
    assert!(stderr(&png).contains("--emf-plates applies to --format emf only"));
}
