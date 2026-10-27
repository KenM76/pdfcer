//! # A sub-pixel stroke stays visible (ISO 32000-1 §8.4.3.2)
//!
//! §8.4.3.2 makes `0 w` mean *"the thinnest line the device can render"*
//! — a statement about DEVICE space. pdfcer mapped it to a fixed `0.1`
//! **user** units, which is wrong in both directions: at low zoom that is
//! a fraction of a pixel and anti-aliases to near-invisible, and at high
//! zoom it becomes a visibly thick line.
//!
//! The same floor rescues thin-but-nonzero widths. The benign-bucket
//! audit measured `0.1 w` landing at 0.17 device pixels, which pdfcer
//! anti-aliased to grey 233 — roughly 9% contrast — across nine qpdf
//! `form-*.pdf` files with an identical 482-pixel signature, silently.
//! pdfium and Acrobat both draw a solid ~1 px line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

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

/// One horizontal black line at the given width.
fn page(width: &str) -> Vec<u8> {
    sized_page(&format!("0 G {width} w 5 30 m 95 30 l S\n"), 100, 60, "")
}

/// A `w` x `h` page drawing `content`, with `page_extra` inside the page
/// dictionary (a `/Group`, for one).
fn sized_page(content: &str, w: u32, h: u32, page_extra: &str) -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            &format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {w} {h}] \
                 /Resources << >> >>"
            ),
        ),
        (
            3,
            &format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page_extra} >>"),
        ),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
    ])
}

fn render_at(bytes: Vec<u8>, scale: f32) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, scale, &RenderOptions::default()).expect("render")
}

/// The darkest pixel anywhere on the page, as a 0–255 grey.
///
/// Contrast, not coverage: the defect was a line that was *drawn* but
/// nearly the colour of the paper, so counting inked pixels would have
/// reported success.
fn darkest(r: &RenderedPage) -> u8 {
    let pm = &r.pixmap;
    (0..pm.height())
        .flat_map(|y| (0..pm.width()).map(move |x| (x, y)))
        .filter_map(|(x, y)| pm.pixel(x, y).map(|p| p.demultiply().red()))
        .min()
        .unwrap_or(255)
}

#[test]
fn a_sub_pixel_stroke_renders_like_a_one_pixel_one() {
    // ASSERTED RELATIVELY, and the first version of this test was wrong
    // for instructive reasons. It demanded grey < 80 and got 127 — but
    // 127 is CORRECT: a one-device-pixel line centred on a pixel boundary
    // anti-aliases across two rows at half coverage each. The absolute
    // threshold was measuring anti-alias placement, not the defect.
    //
    // Comparing against a stroke that was already one device pixel wide
    // removes that entirely: if the floor works, `0.1 w` and `1 w` at
    // scale 1 are the same line.
    let thin = darkest(&render_at(page("0.1"), 1.0));
    let one = darkest(&render_at(page("1"), 1.0));
    assert_eq!(
        thin, one,
        "a sub-pixel stroke must render exactly as a one-pixel stroke does"
    );
    // And it must be a real line, not a hint of one. Grey 233 — about 9%
    // contrast — was the shipped behaviour.
    assert!(thin < 160, "still too faint at grey {thin}");
}

#[test]
fn a_zero_width_stroke_is_dark_at_every_zoom() {
    // §8.4.3.2's actual requirement, and the half the old fixed 0.1
    // user-space constant got wrong at BOTH ends of the zoom range.
    for scale in [0.5_f32, 1.0, 4.0] {
        let d = darkest(&render_at(page("0"), scale));
        assert!(
            d < 160,
            "`0 w` at scale {scale} rendered at grey {d}; \
             §8.4.3.2 asks for the thinnest line the device CAN render, \
             not the thinnest it can hint at"
        );
    }
}

#[test]
fn the_floor_does_not_thicken_a_stroke_that_is_already_wide() {
    // The guard that keeps this a floor rather than a redefinition: a
    // width comfortably over one device pixel must be untouched, or every
    // drawing on screen would silently gain weight.
    let ink = |scale: f32| {
        let r = render_at(page("4"), scale);
        let pm = &r.pixmap;
        (0..pm.height())
            .flat_map(|y| (0..pm.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| pm.pixel(x, y).is_some_and(|p| p.demultiply().red() < 128))
            .count()
    };
    // A 4-unit line at scale 2 must cover about four times the pixels it
    // does at scale 1 — i.e. it scaled, rather than being pinned to a
    // floor that no longer applies.
    let one = ink(1.0);
    let two = ink(2.0);
    assert!(
        two > one * 3,
        "a wide stroke stopped scaling with zoom ({one} px at 1x, {two} px at 2x)"
    );
}

#[test]
fn the_floor_is_computed_in_device_space_not_user_space() {
    // The distinction that makes this a real fix rather than a bigger
    // constant. A sub-pixel width must survive ZOOMING OUT, where a
    // user-space constant cannot help — 0.1 user units at scale 0.5 is
    // 0.05 device px however the constant is chosen.
    let d = darkest(&render_at(page("0.1"), 0.5));
    assert!(
        d < 160,
        "zoomed out, a thin stroke faded to grey {d} — the floor is not \
         being computed through the CTM"
    );
}

/// A DeviceCMYK page group: the page renders through the CMYK buffer.
const CMYK_GROUP: &str = "/Group << /Type /Group /S /Transparency /CS /DeviceCMYK >>";

/// The corner radius of the reported rounded box (G233).
const R: f32 = 4.259;

/// Start abscissae of the diagonals in [`curves_and_lines`].
const DIAG_X0: [f32; 6] = [2.0, 10.1, 18.2, 26.3, 34.4, 42.5];

/// A 340 x 255 page: 24 circles of radius [`R`] stroked at `0.34 w` below
/// y = 150, six 45° lines from (x0, 160) to (x0 + 80, 240) and a horizontal
/// line at y = 200.3 from x = 120 to x = 320, all at the same width. The six
/// [`DIAG_X0`] spread the diagonals over the rasteriser's four vertical
/// sub-samples: at one phase alone a 1 px 45° stroke reads 0.89 or 1.06.
fn curves_and_lines(page_extra: &str) -> Vec<u8> {
    let k = 0.5523 * R;
    let mut s = String::from("0 G 0.34 w\n");
    for i in 0..12 {
        for j in 0..2 {
            let (cx, cy) = (25.0 + i as f32 * 25.0, 60.0 + j as f32 * 40.0);
            s += &format!(
                "{} {cy} m {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c \
                 {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c S\n",
                cx + R,
                cx + R,
                cy + k,
                cx + k,
                cy + R,
                cy + R,
                cx - k,
                cy + R,
                cx - R,
                cy + k,
                cx - R,
                cx - R,
                cy - k,
                cx - k,
                cy - R,
                cy - R,
                cx + k,
                cy - R,
                cx + R,
                cy - k,
                cx + R,
            );
        }
    }
    for x0 in DIAG_X0 {
        s += &format!("{x0} 160 m {} 240 l S\n", x0 + 80.0);
    }
    s += "120 200.3 m 320 200.3 l S\n";
    sized_page(&s, 340, 255, page_extra)
}

/// Ink (0..1 per pixel, from the red channel over white) summed over the
/// pixels whose centres satisfy `keep(u, v)` in user space.
fn ink(r: &RenderedPage, scale: f32, keep: impl Fn(f32, f32) -> bool) -> f32 {
    let pm = &r.pixmap;
    let h = 255.0;
    let mut sum = 0.0;
    for y in 0..pm.height() {
        for x in 0..pm.width() {
            let (u, v) = ((x as f32 + 0.5) / scale, h - (y as f32 + 0.5) / scale);
            if keep(u, v)
                && let Some(p) = pm.pixel(x, y)
            {
                sum += f32::from(255 - p.demultiply().red()) / 255.0;
            }
        }
    }
    sum
}

/// Ink per unit length of the circles and of the 45° lines, each over the
/// horizontal line's.
fn weight_ratios(page_extra: &str, scale: f32) -> (f32, f32) {
    let r = render_at(curves_and_lines(page_extra), scale);
    let straight = ink(&r, scale, |u, v| {
        u > 125.0 && u < 315.0 && (v - 200.3).abs() < 2.0
    }) / 190.0;
    let circles = ink(&r, scale, |_, v| v < 150.0) / (24.0 * std::f32::consts::TAU * R);
    let diag = ink(&r, scale, |u, v| {
        v > 165.0
            && v < 235.0
            && u < 118.0
            && DIAG_X0.iter().any(|x0| (v - u - (160.0 - x0)).abs() < 3.0)
    }) / (6.0 * 70.0 * std::f32::consts::SQRT_2);
    (circles / straight, diag / straight)
}

#[test]
fn a_thin_curve_or_diagonal_carries_the_weight_of_a_thin_straight_line() {
    // G233: a 0.34 pt rounded box drew its corners paler than its sides.
    // A stroke at most one device pixel wide went to tiny-skia's modulated
    // hairline, which inks one pixel per major-axis step: 0.83-0.86 of the
    // straight weight on these circles, 1/√2 on a 45° line. Acrobat draws
    // them uniform.
    for scale in [1.0_f32, 1.3333, 2.0] {
        let (circles, diag) = weight_ratios("", scale);
        assert!(
            (0.93..=1.07).contains(&circles) && (0.93..=1.07).contains(&diag),
            "at scale {scale}, circles {circles:.3} and 45° {diag:.3} of the straight weight"
        );
    }
}

#[test]
fn a_thin_curve_on_a_cmyk_page_carries_the_weight_of_a_straight_line() {
    // The CMYK buffer outlined strokes at a resolution scale of 1 whatever
    // the zoom, so at scale 3 a radius-4.26 curve came out at 0.80 of the
    // straight weight.
    for scale in [1.0_f32, 2.0, 3.0] {
        let (circles, diag) = weight_ratios(CMYK_GROUP, scale);
        assert!(
            (0.93..=1.07).contains(&circles) && (0.93..=1.07).contains(&diag),
            "at scale {scale}, circles {circles:.3} and 45° {diag:.3} of the straight weight"
        );
    }
}

#[test]
fn a_dashed_stroke_keeps_its_gaps_on_every_paint_route() {
    // The CMYK buffer, the overprint composite and the non-separable blend
    // composite each outlined strokes with `Path::stroke`, which ignores the
    // dash array, so `[6 6] 0 d` drew solid on all three.
    let gaps = |gs: &str, extra: &str| {
        let content = "/G1 gs 0 0 0 1 K 3 w [6 6] 0 d 10 30 m 90 30 l S\n";
        let page_extra = format!("/Resources << /ExtGState << /G1 {gs} >> >> {extra}");
        let r = render_at(sized_page(content, 100, 60, &page_extra), 1.0);
        (12..88)
            .filter(|&x| {
                r.pixmap
                    .pixel(x, 30)
                    .is_some_and(|p| p.demultiply().red() > 200)
            })
            .count()
    };
    let plain = gaps("<< >>", "");
    assert!(
        plain > 20,
        "the plain page lost its dash gaps ({plain} light pixels)"
    );
    for (route, gs, extra) in [
        ("CMYK page group", "<< >>", CMYK_GROUP),
        ("overprint", "<< /OP true /OPM 1 >>", ""),
        ("non-separable blend", "<< /BM /Luminosity >>", ""),
    ] {
        assert_eq!(
            gaps(gs, extra),
            plain,
            "a dashed stroke through the {route} route must dash as a plain one does"
        );
    }
}
