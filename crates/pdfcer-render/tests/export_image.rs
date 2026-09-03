//! # Raster export keeps transparency, and says so in the file (`Pass 248.0`)
//!
//! The operator's words: *"there had better be full support (including
//! transparency where supported!)"*. These tests hold the renderer to that
//! on **both** compositing paths — the additive (sRGB) page and the
//! subtractive (`DeviceCMYK` page group) one — because the transparent
//! collapse is a *separate function* on each, and a flag that works on nine
//! pages in ten and silently flattens the tenth is the exact defect a
//! render-then-encode test on one fixture would never see.
//!
//! Every assertion here DECODES the bytes the encoder produced, with an
//! independent decoder (`png`, `zune-jpeg`), rather than inspecting the
//! pixmap that fed it. The property under test is what another program
//! will see when it opens the file — the demultiply, the `pHYs` chunk, the
//! JFIF density — and none of those exist in the pixmap.
//!
//! Fixtures are built inline (`docs/LEGAL.md` §5): a 60 × 60 pt page with a
//! 20 × 20 pt fill in the middle at `/ca 0.5`, and nothing else. Corner
//! pixels are therefore *unpainted*; centre pixels are *half-covered*. The
//! two numbers a transparent export must get right.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::export::{JpegOptions, Rgb, encode_jpeg, encode_png};
use pdfcer_render::{PageBackdrop, RenderOptions, RenderedPage, render_page_with};

fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// A page painting a half-alpha `fill` (a colour operator string) as a
/// 20 × 20 square in the middle of a 60 × 60 page. `group` is spliced into
/// the page dictionary — `""` for an ordinary additive page, or a
/// `/Group` entry naming `DeviceCMYK` to force the subtractive path.
fn page(fill: &str, group: &str) -> Vec<u8> {
    let stream = format!("/GS0 gs {fill} 20 20 20 20 re f\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] \
             /Resources << /ExtGState << /GS0 << /ca 0.5 >> >> >> >>",
        ),
        (
            3,
            &format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {group} >>"),
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

const CMYK_GROUP: &str = "/Group << /S /Transparency /CS /DeviceCMYK >>";

fn render(bytes: Vec<u8>, backdrop: PageBackdrop) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let opts = RenderOptions::default().with_backdrop(backdrop);
    render_page_with(&doc, &p, 1.0, &opts).expect("render")
}

/// Decode a PNG with the `png` crate: straight RGBA8 bytes plus the
/// `pHYs` pixels-per-metre, if any.
fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>, Option<u32>) {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    let ppm = reader.info().pixel_dims.map(|d| {
        assert_eq!(d.unit, png::Unit::Meter);
        assert_eq!(d.xppu, d.yppu);
        d.xppu
    });
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf, ppm)
}

fn rgba_at(w: u32, data: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

// ---------------------------------------------------------------------------
// PNG — additive page
// ---------------------------------------------------------------------------

#[test]
fn a_transparent_png_is_see_through_where_nothing_was_painted() {
    let r = render(page("1 0 0 rg", ""), PageBackdrop::Transparent);
    let png = encode_png(&r.pixmap, None).unwrap();
    let (w, h, data, ppm) = decode_png(&png);
    assert_eq!((w, h), (60, 60));
    assert_eq!(ppm, None, "no dpi asked for, no pHYs written");

    // Unpainted corner: fully transparent, and the colour channels of a
    // transparent pixel are zero (the only premultiplied value they can
    // hold, demultiplied to zero).
    assert_eq!(rgba_at(w, &data, 2, 2), [0, 0, 0, 0]);

    // Centre of the half-alpha red square: STRAIGHT red at alpha 128.
    // `[128, 0, 0, 128]` here would mean the encoder wrote premultiplied
    // bytes — the dark-fringe defect the module docs warn about.
    let c = rgba_at(w, &data, 30, 30);
    assert!((i32::from(c[3]) - 128).abs() <= 1, "alpha {}", c[3]);
    assert!(
        c[0] >= 253,
        "red must be demultiplied to full, got {}",
        c[0]
    );
    assert_eq!((c[1], c[2]), (0, 0));
}

#[test]
fn the_default_backdrop_is_still_paper_everywhere() {
    let r = render(page("1 0 0 rg", ""), PageBackdrop::White);
    let png = encode_png(&r.pixmap, None).unwrap();
    let (w, _, data, _) = decode_png(&png);
    assert_eq!(rgba_at(w, &data, 2, 2), [255, 255, 255, 255]);
    let c = rgba_at(w, &data, 30, 30);
    assert_eq!(c[3], 255, "a white-backed page is opaque everywhere");
    // Half red over white: red stays 255, green/blue drop to ~128.
    assert_eq!(c[0], 255);
    assert!((i32::from(c[1]) - 128).abs() <= 1, "green {}", c[1]);
}

#[test]
fn the_two_backdrops_agree_wherever_the_page_is_opaque() {
    // A fully opaque fill must produce byte-identical pixels under both
    // backdrops: transparency support must not have moved the opaque case.
    let stream_opaque = |group| {
        let stream = "0 0 1 rg 0 0 60 60 re f\n";
        build(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] /Resources << >> >>",
            ),
            (
                3,
                &format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {group} >>"),
            ),
            (
                4,
                &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
            ),
        ])
    };
    for group in ["", CMYK_GROUP] {
        let a = render(stream_opaque(group), PageBackdrop::White);
        let b = render(stream_opaque(group), PageBackdrop::Transparent);
        assert_eq!(a.pixmap.data(), b.pixmap.data(), "group={group:?}");
        assert!(a.pixmap.pixels().iter().all(|p| p.alpha() == 255));
    }
}

// ---------------------------------------------------------------------------
// PNG — subtractive page (the SECOND collapse, the one a flag test misses)
// ---------------------------------------------------------------------------

#[test]
fn a_cmyk_page_group_keeps_its_alpha_too() {
    // `/Group /CS /DeviceCMYK` puts the page through `CmykBuffer`, whose
    // opaque collapse bakes the white INTO the pixel arithmetic. The
    // transparent sibling must not.
    let r = render(page("0 0 0 1 k", CMYK_GROUP), PageBackdrop::Transparent);
    let png = encode_png(&r.pixmap, None).unwrap();
    let (w, _, data, _) = decode_png(&png);
    assert_eq!(
        rgba_at(w, &data, 2, 2)[3],
        0,
        "unpainted corner is transparent"
    );
    let c = rgba_at(w, &data, 30, 30);
    assert!(
        (i32::from(c[3]) - 128).abs() <= 1,
        "half-alpha black ink keeps alpha 128 on the ink path, got {c:?}"
    );
    // Straight colour of 100 % K under the calibrated intent is a near
    // black, NOT the grey a white-flattened pixel would show.
    assert!(c[0] < 60 && c[1] < 60 && c[2] < 60, "ink colour {c:?}");

    // And the SAME fixture white-backed is grey and opaque: the two
    // collapses differ only in the term this test exists to exclude.
    let r = render(page("0 0 0 1 k", CMYK_GROUP), PageBackdrop::White);
    let png = encode_png(&r.pixmap, None).unwrap();
    let (w, _, data, _) = decode_png(&png);
    let c = rgba_at(w, &data, 30, 30);
    assert_eq!(c[3], 255);
    assert!(
        c[0] > 100 && c[0] < 160,
        "half black over white is a mid grey, got {c:?}"
    );
}

// ---------------------------------------------------------------------------
// Resolution metadata
// ---------------------------------------------------------------------------

#[test]
fn a_dpi_is_written_as_a_phys_chunk() {
    let r = render(page("1 0 0 rg", ""), PageBackdrop::White);
    let png = encode_png(&r.pixmap, Some(300.0)).unwrap();
    let (_, _, _, ppm) = decode_png(&png);
    // 300 / 0.0254 = 11811.02 → 11811 px/m.
    assert_eq!(ppm, Some(11811));
}

#[test]
fn a_jpeg_carries_its_density_in_the_jfif_header() {
    let r = render(page("1 0 0 rg", ""), PageBackdrop::White);
    // `JpegOptions` is `#[non_exhaustive]`, so a consumer (this test
    // stands where `pdfcer-gui` stands) builds it from `Default` and
    // assigns -- the same shape `RenderOptions` uses.
    let mut opts = JpegOptions::default();
    opts.dpi = Some(150.0);
    let jpeg = encode_jpeg(&r.pixmap, &opts).unwrap();
    // SOI, then APP0: FF E0, length(2), "JFIF\0", version(2), units(1),
    // Xdensity(2), Ydensity(2) — ITU-T T.871 §B.1.
    assert_eq!(&jpeg[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
    assert_eq!(&jpeg[6..11], b"JFIF\0");
    let units = jpeg[13];
    let xd = u16::from_be_bytes([jpeg[14], jpeg[15]]);
    let yd = u16::from_be_bytes([jpeg[16], jpeg[17]]);
    assert_eq!(units, 1, "1 = dots per inch");
    assert_eq!((xd, yd), (150, 150));
}

// ---------------------------------------------------------------------------
// JPEG — no alpha, so the backdrop is the caller's colour
// ---------------------------------------------------------------------------

#[test]
fn a_jpeg_flattens_transparency_over_the_requested_background() {
    let r = render(page("1 0 0 rg", ""), PageBackdrop::Transparent);
    let mut opts = JpegOptions::default();
    opts.quality = 100;
    opts.background = Rgb { r: 0, g: 0, b: 255 };
    let jpeg = encode_jpeg(&r.pixmap, &opts).unwrap();
    let mut decoder =
        zune_jpeg::JpegDecoder::new(zune_jpeg::zune_core::bytestream::ZCursor::new(&jpeg));
    let pixels = decoder.decode().expect("jpeg decodes");
    let info = decoder.info().expect("jpeg info");
    assert_eq!((info.width, info.height), (60, 60));
    let at = |x: usize, y: usize| {
        let i = (y * 60 + x) * 3;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // Unpainted corner became the background, not white.
    let corner = at(2, 2);
    assert!(
        corner[2] > 240 && corner[0] < 15,
        "corner {corner:?} should be blue"
    );
    // Half red over blue: roughly (128, 0, 127), within JPEG's tolerance.
    let centre = at(30, 30);
    assert!(
        (i32::from(centre[0]) - 128).abs() <= 12,
        "centre {centre:?}"
    );
    assert!(
        (i32::from(centre[2]) - 127).abs() <= 12,
        "centre {centre:?}"
    );
}
