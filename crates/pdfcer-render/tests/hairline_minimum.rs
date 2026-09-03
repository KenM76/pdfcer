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
    let content = format!("0 G {width} w 5 30 m 95 30 l S\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 60] \
             /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
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
