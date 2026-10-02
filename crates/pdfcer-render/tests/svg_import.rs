//! SVG import parity: an SVG placed with `EditSession::add_svg`, saved,
//! reopened and rendered by pdfcer must match resvg's raster of the same
//! SVG. resvg is the oracle because it consumes the very `usvg` tree the
//! importer reads, so a mismatch is the importer's translation, not a
//! parsing disagreement.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::svg_import;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

/// Device pixels per SVG px (the page is 200 × 100 pt, the SVG 200 × 100 px).
const SCALE: f32 = 2.0;

fn blank_page() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
        "<< /Length 0 >>\nstream\n\nendstream".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// Place `svg` over the whole page, save, reopen; the saved bytes.
fn place(svg: &str) -> Vec<u8> {
    let imported = svg_import::import(svg.as_bytes()).expect("the SVG imports");
    let mut session = EditSession::new(Document::from_bytes(blank_page()).expect("blank page"));
    let rect = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 200.0,
        ury: 100.0,
    };
    session.add_svg(0, rect, &imported).expect("add_svg");
    session
        .to_full_bytes(&SaveOptions::default())
        .expect("save")
        .0
}

fn pdfcer_raster(bytes: Vec<u8>) -> Pixmap {
    let doc = Document::from_bytes(bytes).expect("reopens");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let opts = RenderOptions::default().with_backdrop(PageBackdrop::Transparent);
    render_page_with(&doc, &p, SCALE, &opts)
        .expect("render")
        .pixmap
}

fn resvg_raster(svg: &str) -> Pixmap {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("usvg parses");
    let mut pixmap = Pixmap::new(400, 200).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );
    pixmap
}

/// Fraction of pixels differing by more than `tol` in any premultiplied
/// channel, and the worst difference.
fn diff(a: &Pixmap, b: &Pixmap, tol: u8) -> (f64, u8) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut bad = 0usize;
    let mut worst = 0u8;
    for (pa, pb) in a.pixels().iter().zip(b.pixels()) {
        let m = [
            pa.red().abs_diff(pb.red()),
            pa.green().abs_diff(pb.green()),
            pa.blue().abs_diff(pb.blue()),
            pa.alpha().abs_diff(pb.alpha()),
        ]
        .into_iter()
        .max()
        .unwrap();
        worst = worst.max(m);
        if m > tol {
            bad += 1;
        }
    }
    (bad as f64 / a.pixels().len() as f64, worst)
}

/// Fraction of pixels with any ink — guards against two empty rasters
/// agreeing.
fn inked(p: &Pixmap) -> f64 {
    p.pixels().iter().filter(|px| px.alpha() > 0).count() as f64 / p.pixels().len() as f64
}

fn assert_parity(name: &str, svg: &str, max_bad: f64) {
    let ours = pdfcer_raster(place(svg));
    let oracle = resvg_raster(svg);
    assert!(
        inked(&oracle) > 0.05,
        "{name}: the fixture must draw something"
    );
    if let Ok(dir) = std::env::var("SVG_PARITY_DUMP") {
        ours.save_png(format!("{dir}/{name}-ours.png")).unwrap();
        oracle.save_png(format!("{dir}/{name}-resvg.png")).unwrap();
        std::fs::write(format!("{dir}/{name}.pdf"), place(svg)).unwrap();
    }
    let (bad, worst) = diff(&ours, &oracle, 24);
    assert!(
        bad <= max_bad,
        "{name}: {:.2}% of pixels differ by more than 24 levels (worst {worst})",
        bad * 100.0
    );
}

const BASIC: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#ff0000"/>
      <stop offset="0.5" stop-color="#00ff00"/>
      <stop offset="1" stop-color="#0000ff"/>
    </linearGradient>
  </defs>
  <rect x="10" y="10" width="80" height="40" fill="#336699"/>
  <path d="M 110 10 L 190 50 Q 150 90 110 60 Z" fill="none" stroke="#cc3300" stroke-width="6" stroke-linejoin="round"/>
  <rect x="10" y="60" width="180" height="30" fill="url(#g)"/>
</svg>"##;

#[test]
fn a_plain_svg_renders_like_resvg() {
    assert_parity("basic", BASIC, 0.01);
}

#[test]
fn gradient_spread_and_radial_render_like_resvg() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <defs>
        <linearGradient id="r" x1="0.4" x2="0.6" spreadMethod="reflect">
          <stop offset="0" stop-color="yellow"/><stop offset="1" stop-color="purple"/>
        </linearGradient>
        <radialGradient id="c" cx="0.5" cy="0.5" r="0.2" fx="0.45" fy="0.5" spreadMethod="repeat">
          <stop offset="0" stop-color="white"/><stop offset="1" stop-color="teal"/>
        </radialGradient>
      </defs>
      <rect width="100" height="100" fill="url(#r)"/>
      <circle cx="150" cy="50" r="45" fill="url(#c)"/>
    </svg>"##;
    assert_parity("spread", svg, 0.02);
}

#[test]
fn opacity_clip_and_mask_render_like_resvg() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <defs>
        <clipPath id="k"><circle cx="50" cy="50" r="40"/></clipPath>
        <mask id="m"><rect width="200" height="100" fill="white"/><rect x="140" width="20" height="100" fill="black"/></mask>
        <linearGradient id="fade"><stop offset="0" stop-color="navy" stop-opacity="1"/><stop offset="1" stop-color="navy" stop-opacity="0"/></linearGradient>
      </defs>
      <rect width="200" height="100" fill="url(#fade)"/>
      <g clip-path="url(#k)" opacity="0.6"><rect width="100" height="100" fill="red"/><rect x="40" width="20" height="100" fill="green"/></g>
      <g mask="url(#m)"><rect x="110" y="10" width="80" height="80" fill="orange"/></g>
    </svg>"##;
    assert_parity("clip-mask", svg, 0.02);
}

#[test]
fn transforms_and_dashes_render_like_resvg() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <g transform="translate(60 50) skewX(15)"><circle r="35" fill="none" stroke="teal" stroke-width="8" stroke-linecap="round" stroke-dasharray="20 10"/></g>
      <g transform="translate(150 50) rotate(30) scale(1.5 0.8)">
        <rect x="-25" y="-25" width="50" height="50" fill="crimson" stroke="black" stroke-width="2" stroke-dasharray="6 3"/>
      </g>
    </svg>"##;
    assert_parity("transforms", svg, 0.02);
}

/// An SVG pattern becomes a tiling pattern (ISO 32000-1 §8.7.3) whose
/// matrix carries the pattern transform into form space. Checked
/// structurally: pdfcer-render does not paint `PatternType 1` yet, so it
/// cannot be the oracle here (pdfium renders this output like resvg).
#[test]
fn a_pattern_fill_becomes_a_tiling_pattern() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <defs>
        <pattern id="p" width="10" height="10" patternUnits="userSpaceOnUse">
          <rect width="5" height="5" fill="black"/><rect x="5" y="5" width="5" height="5" fill="gray"/>
        </pattern>
      </defs>
      <rect x="10" y="10" width="80" height="80" fill="url(#p)"/>
    </svg>"##;
    let text = String::from_utf8_lossy(&place(svg)).into_owned();
    for needle in [
        "/PatternType 1",
        "/PaintType 1",
        "/XStep 10",
        "/YStep 10",
        "/BBox [0 0 10 10]",
        "/Matrix [1 0 0 -1 0 100]",
    ] {
        assert!(text.contains(needle), "the tiling pattern carries {needle}");
    }
}

/// The placement is vector: a Form XObject of path and shading operators,
/// no image XObject.
#[test]
fn the_placed_drawing_is_a_form_with_no_image() {
    let bytes = place(BASIC);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Form"), "a Form XObject is written");
    assert!(!text.contains("/Subtype /Image"), "no image XObject");
    assert!(
        text.contains("/ShadingType 2"),
        "the gradient is an axial shading"
    );
}
