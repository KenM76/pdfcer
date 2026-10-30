#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)] // a panic here is the test failing

use super::super::*;
use crate::xfa::{LayoutOptions, XdpDocument, layout_template, read_template};

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="0pt" y="0pt" w="540pt" h="720pt"/></pageArea></pageSet>"#;

/// A `w` by `h` 24-bit BMP of one colour; `ppm` pixels per metre (0: none).
fn bmp(w: u32, h: u32, rgb: [u8; 3], ppm: u32) -> Vec<u8> {
    let stride = (w * 3).div_ceil(4) * 4;
    let size = 54 + stride * h;
    let mut b = b"BM".to_vec();
    for v in [size, 0, 54, 40, w, h] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    for v in [0, stride * h, ppm, ppm, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for _ in 0..h {
        let mut row: Vec<u8> = (0..w).flat_map(|_| [rgb[2], rgb[1], rgb[0]]).collect();
        row.resize(stride as usize, 0);
        b.extend_from_slice(&row);
    }
    b
}

/// A 2x1 RGBA PNG: one opaque red pixel, one fully transparent one.
fn png_with_alpha() -> Vec<u8> {
    png(6, &[0, 255, 0, 0, 255, 0, 0, 255, 0])
}

/// A 2x1 8-bit PNG of colour type `colour` from filtered scanline `raw`.
fn png(colour: u8, raw: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(payload);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(raw).unwrap();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(
        &mut out,
        b"IHDR",
        &[0, 0, 0, 2, 0, 0, 0, 1, 8, colour, 0, 0, 0],
    );
    chunk(&mut out, b"IDAT", &z.finish().unwrap());
    chunk(&mut out, b"IEND", &[]);
    out
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=c.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

/// Paint every page of `body` under a root subform laid out `layout`.
fn paint_all(
    body: &str,
    layout: &str,
    opts: &PaintOptions,
    images: &XfaImages,
) -> Vec<(String, Vec<PaintNote>)> {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f" layout="{layout}">{PAGE}{body}</subform></template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    l.pages
        .iter()
        .map(|page| {
            let out = paint_page_with_images(&t, page, opts, images);
            let ops = String::from_utf8_lossy(&out.content);
            (
                ops.split_whitespace().collect::<Vec<_>>().join(" "),
                out.notes,
            )
        })
        .collect()
}

fn paint(body: &str, opts: &PaintOptions, images: &XfaImages) -> (String, Vec<PaintNote>) {
    paint_all(body, "position", opts, images).remove(0)
}

/// A 100pt-square imageEdit field at (10, 20) holding `image`.
fn field(image: &str) -> String {
    format!(
        r#"<field name="f1" x="10pt" y="20pt" w="100pt" h="100pt"><ui><imageEdit/></ui><value>{image}</value></field>"#
    )
}

/// An inline 20x10 px image, recording no resolution, with `aspect`.
fn inline(aspect: &str) -> String {
    format!(
        r#"<image aspect="{aspect}">{}</image>"#,
        base64(&bmp(20, 10, [255, 0, 0], 0))
    )
}

/// The `cm` that sizes the image: `w 0 0 h x y cm`, page height 792.
fn cm(w: f64, h: f64, x: f64, top: f64) -> String {
    format!("{w} 0 0 {h} {x} {} cm", 792.0 - top - h)
}

#[test]
fn each_aspect_sizes_an_inline_image_as_p750_says() {
    let images = XfaImages::new();
    for (aspect, w, h) in [
        ("fit", 100.0, 50.0),
        ("none", 100.0, 100.0),
        ("actual", 20.0, 10.0),
        ("width", 100.0, 50.0),
        ("height", 200.0, 100.0),
    ] {
        let (ops, notes) = paint(&field(&inline(aspect)), &PaintOptions::new(), &images);
        assert!(ops.contains(&cm(w, h, 10.0, 20.0)), "{aspect}: {ops}");
        assert!(ops.contains("BI /W 20 /H 10 /BPC 8 /CS /RGB"), "{ops}");
        assert!(ops.contains("re W n"), "clipped to the box: {ops}");
        assert!(notes.is_empty(), "{aspect}: {notes:?}");
    }
}

#[test]
fn para_placement_and_show_overflow_are_settings() {
    let images = XfaImages::new();
    let body = field(&inline("actual")).replace(
        "<value>",
        r#"<para hAlign="center" vAlign="bottom"/><value>"#,
    );
    let (ops, _) = paint(&body, &PaintOptions::new(), &images);
    assert!(
        ops.contains(&cm(20.0, 10.0, 10.0, 20.0)),
        "top-left by default: {ops}"
    );
    let para = PaintOptions::new().with_image_placement(ImagePlacement::Para);
    let (ops, _) = paint(&body, &para, &images);
    assert!(ops.contains(&cm(20.0, 10.0, 50.0, 110.0)), "{ops}");
    let show = PaintOptions::new().with_image_overflow(ImageOverflow::Show);
    let (ops, _) = paint(&field(&inline("height")), &show, &images);
    assert!(!ops.contains("W n"), "not clipped: {ops}");
}

#[test]
fn an_image_without_a_resolution_takes_the_assumed_one() {
    let img = format!(
        r#"<image aspect="actual">{}</image>"#,
        base64(&bmp(20, 10, [0, 0, 255], 0))
    );
    let images = XfaImages::new();
    let (ops, _) = paint(&field(&img), &PaintOptions::new(), &images);
    assert!(ops.contains(&cm(20.0, 10.0, 10.0, 20.0)), "72 dpi: {ops}");
    let dpi = PaintOptions::new().with_image_dpi(144);
    let (ops, _) = paint(&field(&img), &dpi, &images);
    assert!(ops.contains(&cm(10.0, 5.0, 10.0, 20.0)), "{ops}");
    let own = format!(
        r#"<image aspect="actual">{}</image>"#,
        base64(&bmp(20, 10, [0, 0, 255], 2835))
    );
    let (ops, _) = paint(&field(&own), &dpi, &images);
    assert!(ops.contains("19.99"), "the image's own ~72 dpi wins: {ops}");
}

#[test]
fn an_href_resolves_only_in_xfa_images_and_wins_over_inline() {
    let src = crate::image_import::import(&bmp(8, 8, [0, 128, 0], 2835)).unwrap();
    let jpeg = crate::image_import::jpeg_encode::to_jpeg(&src, 90)
        .unwrap()
        .data;
    let mut images = XfaImages::new();
    images.insert("a.jpg", jpeg);
    let held = r#"<image href="a.jpg" contentType="image/jpg" aspect="actual">AAAA</image>"#;
    let (ops, notes) = paint(&field(held), &PaintOptions::new(), &images);
    assert!(
        ops.contains("/F [/AHx /DCT]"),
        "JPEG sniffed, href first: {ops}"
    );
    assert!(notes.is_empty(), "{notes:?}");
    let (ops, notes) = paint(
        &field(r#"<image href="b.png"/>"#),
        &PaintOptions::new(),
        &images,
    );
    assert!(!ops.contains("BI"), "{ops}");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].kind, PaintNoteKind::Image);
    let (_, notes) = paint(
        &field(r#"<image href="./a.jpg"/>"#),
        &PaintOptions::new(),
        &images,
    );
    assert_eq!(notes.len(), 1, "exact match only: {notes:?}");
}

#[test]
fn bad_inline_content_is_noted_and_an_empty_image_is_silent() {
    let images = XfaImages::new();
    let (ops, notes) = paint(
        &field("<image>not*base64</image>"),
        &PaintOptions::new(),
        &images,
    );
    assert!(!ops.contains("BI"));
    assert_eq!(notes.len(), 1, "{notes:?}");
    let (_, notes) = paint(&field("<image>AAAA</image>"), &PaintOptions::new(), &images);
    assert_eq!(notes.len(), 1, "decodes but is no image: {notes:?}");
    let (_, notes) = paint(&field("<image/>"), &PaintOptions::new(), &images);
    assert!(
        notes.is_empty(),
        "an empty image field is not an omission: {notes:?}"
    );
}

#[test]
fn transparency_is_flattened_onto_white_and_noted() {
    let img = format!(
        r#"<image aspect="actual">{}</image>"#,
        base64(&png_with_alpha())
    );
    let (ops, notes) = paint(&field(&img), &PaintOptions::new(), &XfaImages::new());
    assert!(
        ops.contains("BI /W 2 /H 1 /BPC 8 /CS /RGB /F [/AHx /Fl] ID"),
        "{ops}"
    );
    let hex = ops.split(" ID ").nth(1).unwrap().split('>').next().unwrap();
    let packed: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    let samples = crate::filters::flate::decode(&packed, None).unwrap();
    assert_eq!(samples, [255, 0, 0, 255, 255, 255], "red, then white");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].kind, PaintNoteKind::ImageTransparency);
}

#[test]
fn a_caption_image_takes_its_natural_width_when_nothing_is_reserved() {
    let body = format!(
        r#"<field name="f1" x="10pt" y="20pt" w="100pt" h="40pt"><ui><textEdit/></ui><caption><value>{}</value></caption><value><text>x</text></value></field>"#,
        inline("actual")
    );
    let (ops, notes) = paint(&body, &PaintOptions::new(), &XfaImages::new());
    assert!(ops.contains(&cm(20.0, 10.0, 10.0, 20.0)), "{ops}");
    assert!(
        ops.contains("1 0 0 1 30 "),
        "value text starts after the image: {ops}"
    );
    assert!(
        notes.iter().all(|n| n.kind == PaintNoteKind::Font),
        "{notes:?}"
    );
}

#[test]
fn a_master_page_image_paints_on_every_page() {
    let page = format!(
        r#"<pageSet><pageArea name="P"><draw name="logo" x="0pt" y="0pt" w="40pt" h="20pt"><value>{}</value></draw><contentArea x="0pt" y="40pt" w="540pt" h="600pt"/></pageArea></pageSet>"#,
        inline("fit")
    );
    let body = r#"<field name="a" w="100pt" h="500pt"/><field name="b" w="100pt" h="500pt"/>"#;
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f" layout="tb">{page}{body}</subform></template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    assert_eq!(l.pages.len(), 2);
    for page in &l.pages {
        let out = paint_page_with_images(&t, page, &PaintOptions::new(), &XfaImages::new());
        let ops = String::from_utf8_lossy(&out.content);
        assert_eq!(ops.matches("BI ").count(), 1, "{ops}");
    }
}

#[test]
fn a_draw_with_an_inline_png_is_painted_at_its_box() {
    let img = base64(&png(2, &[0, 0, 0, 255, 255, 0, 0]));
    let body = format!(
        r#"<draw name="f1" x="10pt" y="20pt" w="100pt" h="100pt"><value><image>{img}</image></value></draw>"#
    );
    let (ops, notes) = paint(&body, &PaintOptions::new(), &XfaImages::new());
    assert!(ops.contains(&cm(100.0, 50.0, 10.0, 20.0)), "fit: {ops}");
    assert!(
        ops.contains("/Predictor 15 /Colors 3"),
        "PNG passed through: {ops}"
    );
    assert!(notes.is_empty(), "{notes:?}");
}
