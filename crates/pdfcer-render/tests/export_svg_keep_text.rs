//! SVG export with text kept as text (G033).
//!
//! The oracle is the outline export of the same page. With text kept, each
//! glyph is drawn by an SVG consumer from the embedded web font at the
//! `<text>` element's transform and per-character `x`; these tests redo that
//! placement from the embedded font's own outlines and require it to land
//! where the outline export put the same glyph. They also require the
//! embedded font to map each character, through its new `cmap`, to the
//! same outline the source program had.
//!
//! Fixtures: the synthetic donor face from `tools/gen-subset-font-fixtures.py`
//! embedded by pdfcer's own add-text (`docs/LEGAL.md` §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::addtext::{self, AddTextRequest};
use pdfcer_render::RenderOptions;
use pdfcer_render::font::subset::plan_subset;
use pdfcer_render::svg::{SvgExport, SvgOptions, SvgText, export_svg};
use skrifa::outline::DrawSettings;
use skrifa::outline::pen::OutlinePen;
use skrifa::prelude::{LocationRef, Size};
use skrifa::raw::TableProvider as _;
use skrifa::{FontRef, MetadataProvider};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../fixtures/synthetic/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {path}: {e}"))
}

/// A page showing `text` in the donor face, embedded as a Type0 font.
fn page_with(text: &str, render_mode: u8) -> Document {
    let plan = plan_subset(
        &fixture("text/subset-donor.ttf"),
        0,
        &['A', 'B', 'C'],
        "pdfceSubsetDemo",
        "ABCDEF",
    )
    .unwrap();
    let doc = Document::from_bytes(fixture("hello.pdf")).unwrap();
    let req = AddTextRequest::new(0, (72.0, 500.0), text)
        .with_embedded_face(plan)
        .with_size(36.0)
        .with_render_mode(render_mode);
    Document::from_bytes(addtext::add_text(&doc, &req).unwrap().bytes).unwrap()
}

fn export(doc: &Document, text: SvgText) -> SvgExport {
    let page = page_tree::pages(doc).unwrap().remove(0);
    export_svg(
        doc,
        &page,
        &RenderOptions::default(),
        &SvgOptions::default().with_raster_dpi(144.0).with_text(text),
    )
    .unwrap()
}

/// The value of `name="…"` in `element`.
fn attr<'a>(element: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let at = element
        .find(&key)
        .unwrap_or_else(|| panic!("no {name} in {element}"))
        + key.len();
    let len = element[at..].find('"').unwrap();
    &element[at..at + len]
}

fn floats(s: &str) -> Vec<f32> {
    s.split([' ', ',', '(', ')'])
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("bad base64 {c}"),
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= u32::from(val(c)) << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// Every on- and off-curve point of a glyph, font units.
#[derive(Default)]
struct Points(Vec<(f32, f32)>);

impl OutlinePen for Points {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.0.extend([(a, b), (x, y)]);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.0.extend([(a, b), (c, d), (x, y)]);
    }
    fn close(&mut self) {}
}

fn outline(font: &FontRef<'_>, c: char) -> Vec<(f32, f32)> {
    let gid = font
        .charmap()
        .map(c)
        .unwrap_or_else(|| panic!("{c} unmapped"));
    let glyph = font.outline_glyphs().get(gid).unwrap();
    let mut pen = Points::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        )
        .unwrap();
    pen.0
}

fn bbox(points: impl IntoIterator<Item = (f32, f32)>) -> [f32; 4] {
    points.into_iter().fold(
        [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        |[x0, y0, x1, y1], (x, y)| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
    )
}

/// The device-space bounding box of each `<path>` the outline export wrote,
/// in document order, skipping the fixture's own Standard-14 text.
fn outline_boxes(svg: &str) -> Vec<[f32; 4]> {
    svg.split("<path ")
        .skip(1)
        .map(|p| {
            let d: Vec<f32> = attr(&format!(" {p}"), "d")
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter_map(|t| t.parse().ok())
                .collect();
            bbox(d.chunks(2).map(|c| (c[0], c[1])))
        })
        .collect()
}

#[test]
fn kept_text_is_one_text_element_with_an_embedded_font_that_maps_each_character() {
    let doc = page_with("ABCA", 0);
    let out = export(&doc, SvgText::KeepText);
    let t = out.outcome.text;
    assert_eq!(t.runs_as_text, 1, "{t:?}");
    assert_eq!(t.fonts_embedded, 1, "{t:?}");

    let svg = &out.svg;
    let start = svg.find("<text ").expect("a <text> element");
    let element = &svg[start..start + svg[start..].find("</text>").unwrap()];
    assert!(element.ends_with(">ABCA"), "{element}");
    assert_eq!(attr(element, "xml:space"), "preserve");
    assert!(attr(element, "font-family").starts_with("pdfcer-f0, "));

    let face = svg.find("@font-face").expect("an @font-face rule");
    assert!(svg[face..].contains("format('truetype')"));
    let b64_at = svg.find("base64,").unwrap() + "base64,".len();
    let b64_len = svg[b64_at..].find(')').unwrap();
    let web = base64_decode(&svg[b64_at..b64_at + b64_len]);
    let web = FontRef::new(&web).expect("the embedded font parses");
    let donor_bytes = fixture("text/subset-donor.ttf");
    let donor = FontRef::new(&donor_bytes).unwrap();
    for c in ['A', 'B', 'C'] {
        assert_eq!(outline(&web, c), outline(&donor, c), "glyph for {c}");
    }
}

#[test]
fn each_kept_glyph_lands_where_the_outline_export_drew_it() {
    let doc = page_with("ABCA", 0);
    let outlines = export(&doc, SvgText::Outlines);
    let kept = export(&doc, SvgText::KeepText);
    assert!(!outlines.svg.contains("<text "));
    assert_eq!(outlines.outcome.text, Default::default());

    // The kept export's paths are the Standard-14 text the fixture already
    // carried; the outline export has those plus one per donor glyph, last.
    let all = outline_boxes(&outlines.svg);
    let others = outline_boxes(&kept.svg).len();
    let glyph_boxes = &all[others..];
    assert_eq!(glyph_boxes.len(), 4, "one path per donor glyph");

    let svg = &kept.svg;
    let start = svg.find("<text ").unwrap();
    let element = &svg[start..start + svg[start..].find("</text>").unwrap()];
    let m = floats(attr(element, "transform"));
    let xs = floats(attr(element, "x"));
    let upem: f32 = attr(element, "font-size").parse().unwrap();
    assert_eq!(xs.len(), 4);

    let donor_bytes = fixture("text/subset-donor.ttf");
    let donor = FontRef::new(&donor_bytes).unwrap();
    assert_eq!(
        upem,
        f32::from(donor.head().unwrap().units_per_em()),
        "font-size is one em in font units"
    );
    for ((c, x), want) in "ABCA".chars().zip(&xs).zip(glyph_boxes) {
        // SVG draws glyph point (px, py) at text-space (x + px, -py), then
        // applies the transform.
        let placed = outline(&donor, c).into_iter().map(|(px, py)| {
            let (u, v) = (x + px, -py);
            (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5])
        });
        let got = bbox(placed);
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g - w).abs() < 0.05,
                "{c}: kept {got:?} vs outlines {want:?}"
            );
        }
    }
}

#[test]
fn stroked_text_stays_outlines_and_is_counted() {
    let doc = page_with("AB", 1);
    let out = export(&doc, SvgText::KeepText);
    assert!(!out.svg.contains("<text "));
    assert!(!out.svg.contains("@font-face"));
    assert_eq!(out.outcome.text.runs_as_text, 0);
    assert_eq!(out.outcome.text.fallback_paint, 1, "{:?}", out.outcome.text);
    assert_eq!(out.outcome.text.fonts_embedded, 0);
}

#[test]
fn a_font_that_is_not_an_sfnt_stays_outlines_and_is_counted() {
    // `hello.pdf` shows Standard-14 text, drawn from a bundled bare-CFF face.
    let doc = Document::from_bytes(fixture("hello.pdf")).unwrap();
    let out = export(&doc, SvgText::KeepText);
    assert!(!out.svg.contains("<text "));
    assert!(
        out.outcome.text.fallback_not_sfnt >= 1,
        "{:?}",
        out.outcome.text
    );
    assert_eq!(
        out.outcome.text.runs_as_outlines(),
        out.outcome.text.fallback_not_sfnt
    );
}
