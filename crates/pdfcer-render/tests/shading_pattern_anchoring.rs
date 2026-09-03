//! # `PatternType 2` shading-pattern fills, and the anchoring that defines them
//!
//! A shading pattern's coordinates are **pattern space**, mapped to the
//! **default** coordinate system of its parent content stream by the
//! pattern's own `/Matrix` — *not* by the CTM in force where the fill
//! happens. ISO 32000-1 §8.7.2 NOTE 1 says so in as many words, and PM5's
//! `shall` is the binding form of it: *"the pattern matrix maps pattern
//! space to the default coordinate system of the pattern's parent content
//! stream"*.
//!
//! That single sentence is the whole difficulty of this feature, and it is
//! the opposite of the sibling route: the `sh` operator paints in CURRENT
//! user space (Table 77). Both routes share this crate's shading painter
//! and differ only in the matrix handed to it, so the failure mode is
//! specific and quiet — swap the two and every page still renders a
//! gradient, in the right place, until something applies a `cm`.
//!
//! ## Why the fixtures put a `cm` between `scn` and the fill
//!
//! Because without one the two anchorings are INDISTINGUISHABLE. On a page
//! whose content stream never transforms anything, `base_ctm` and the
//! current CTM are the same matrix, every assertion passes under either
//! implementation, and the suite reports success while testing nothing.
//! That is not hypothetical — it is the exact shape of two blind spots this
//! project has already paid for (a radial shading whose fixtures admitted
//! only one root, and a Lab fixture whose symmetric `/Range` made an a/b
//! transposition a no-op). A fixture has to be able to tell the right
//! answer from the wrong one before its passing means anything.
//!
//! The `cm` here scales x by 0.5. Correct anchoring leaves the gradient
//! spanning the full page; CTM anchoring compresses it into the left half,
//! so the page's right side saturates early. `the_gradient_is_not_squeezed_
//! by_a_cm` is the assertion that separates them, and it was verified to
//! FAIL under a deliberate `base_ctm` → `current.ctm` sabotage before being
//! trusted (pdfium cross-check over the same fixture moved from mean 1.31 /
//! max 2 to mean 65.06 / max 128).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Assemble a classic single-page PDF with a correct xref table.
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

/// A 200x200 page whose `/P1` is `pattern`, painting `content`.
fn page(pattern: &str, content: &str) -> Vec<u8> {
    let stream = format!("{content}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>",
        ),
        (
            3,
            &format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                 /Resources << /Pattern << /P1 {pattern} >> >> >>"
            ),
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

/// An axial red→blue shading running left to right across x = 0..200 in
/// PATTERN space, extended at both ends so no pixel is left unpainted.
const SHADING: &str = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
     /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
     /Extend [true true] >>";

/// The page content every fixture here paints: select `/P1`, fill the page.
const PAGE_CONTENT: &str = "/Pattern cs /P1 scn 0 0 200 200 re f";

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

/// The demultiplied pixel at (x, y).
fn px(r: &RenderedPage, x: u32, y: u32) -> (u8, u8, u8) {
    let p = r.pixmap.pixel(x, y).expect("in bounds").demultiply();
    (p.red(), p.green(), p.blue())
}

#[test]
fn a_shading_pattern_fill_paints_its_gradient() {
    let r = render(page(
        &format!("<< /PatternType 2 /Shading {SHADING} >>"),
        "/Pattern cs /P1 scn 0 0 200 200 re f",
    ));
    let (lr, _, lb) = px(&r, 4, 100);
    let (rr, _, rb) = px(&r, 195, 100);
    assert!(
        lr > 200 && lb < 55,
        "left edge should be red, got {lr},_,{lb}"
    );
    assert!(
        rr < 55 && rb > 200,
        "right edge should be blue, got {rr},_,{rb}"
    );
    assert_eq!(r.diagnostics.shading.painted, 1, "one shading painted");
    assert_eq!(
        r.diagnostics.shading.via_sh, 0,
        "it arrived via a pattern, not via `sh`"
    );
    assert_eq!(
        r.diagnostics.color.patterns_unpainted, 0,
        "nothing was left unpainted"
    );
}

/// **THE ANCHORING ASSERTION.** §8.7.2 NOTE 1: pattern space is anchored to
/// the stream's default space, so a `cm` applied after the pattern is
/// selected must NOT move or scale the gradient.
///
/// The `cm` halves x, and the rectangle is drawn 400 wide so it still
/// covers the 200pt page after that halving. If the pattern were anchored
/// to the current CTM the gradient would complete within the left half and
/// the page's right side would be saturated blue; anchored correctly, the
/// midpoint is still the gradient's midpoint.
#[test]
fn the_gradient_is_not_squeezed_by_a_cm() {
    let r = render(page(
        &format!("<< /PatternType 2 /Shading {SHADING} >>"),
        "/Pattern cs /P1 scn q 0.5 0 0 1 0 0 cm 0 0 400 200 re f Q",
    ));
    let (mr, _, mb) = px(&r, 100, 100);
    // Mid-gradient: both channels near half. Under CTM anchoring this
    // pixel is fully blue (0, 0, 255).
    assert!(
        (100..=155).contains(&mr) && (100..=155).contains(&mb),
        "page midpoint should be mid-gradient, got ({mr}, _, {mb}) — \
         a value near (0,_,255) means the pattern was anchored to the CTM"
    );
}

/// `/Matrix` maps pattern space to the stream's default space (PM2). A
/// translate must move the gradient by exactly that much, which is the
/// half of the transform chain the anchoring test above cannot see.
#[test]
fn the_pattern_matrix_is_applied() {
    let plain = render(page(
        &format!("<< /PatternType 2 /Shading {SHADING} >>"),
        "/Pattern cs /P1 scn 0 0 200 200 re f",
    ));
    let shifted = render(page(
        &format!("<< /PatternType 2 /Matrix [1 0 0 1 100 0] /Shading {SHADING} >>"),
        "/Pattern cs /P1 scn 0 0 200 200 re f",
    ));
    // Shifting pattern space right by 100 puts the gradient's start at
    // x=100, so x=150 in the shifted render matches x=50 in the plain one.
    let (ar, _, ab) = px(&plain, 50, 100);
    let (br, _, bb) = px(&shifted, 150, 100);
    assert!(
        ar.abs_diff(br) <= 2 && ab.abs_diff(bb) <= 2,
        "a +100 /Matrix translate should shift the gradient by 100: \
         plain@50 = ({ar}, _, {ab}) vs shifted@150 = ({br}, _, {bb})"
    );
}

/// A tiling pattern is not painted this build. The requirement is that it
/// is COUNTED rather than silently skipped — a page that renders blank
/// where a hatch belongs must say why.
///
/// The pattern is an INDIRECT object here, and that is forced rather than
/// stylistic: a tiling pattern is a stream (Table 75), and §7.3.8.1 says
/// "all streams shall be indirect objects". Inlining it in the resource
/// dictionary — as the first draft of this test did — produces a file
/// pdfcer correctly refuses to parse, and the test then measures the
/// refusal instead of the pattern.
#[test]
fn a_tiling_pattern_is_counted_not_silently_skipped() {
    let tile_content = "1 0 0 rg 0 0 5 5 re f";
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Resources << /Pattern << /P1 5 0 R >> >> >>",
        ),
        (
            4,
            // /Length computed, never hand-counted: a wrong one is a
            // `StreamExtentMismatch` at parse time, so the test would
            // measure pdfcer's refusal of the fixture rather than its
            // handling of the pattern.
            &format!(
                "<< /Length {} >>\nstream\n{PAGE_CONTENT}\nendstream",
                PAGE_CONTENT.len()
            ),
        ),
        (
            5,
            &format!(
                "<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
                 /XStep 10 /YStep 10 /Resources << >> /Length {} >>\nstream\n{tile_content}\nendstream",
                tile_content.len()
            ),
        ),
    ]);
    let r = render(bytes);
    assert_eq!(
        r.diagnostics.color.patterns_unpainted, 1,
        "an unpainted tiling pattern must be counted"
    );
    assert_eq!(r.diagnostics.shading.painted, 0);
}

/// A `/Pattern` name with no matching resource paints nothing and is
/// counted. Left unhandled this is a panic or a silent blank.
#[test]
fn a_missing_pattern_resource_is_counted_not_fatal() {
    let r = render(page(
        &format!("<< /PatternType 2 /Shading {SHADING} >>"),
        "/Pattern cs /Nope scn 0 0 200 200 re f",
    ));
    assert_eq!(r.diagnostics.color.patterns_unpainted, 1);
    assert_eq!(r.diagnostics.shading.painted, 0);
}

/// Selecting a pattern and then a plain colour must paint the PLAIN
/// colour. The pattern name lives in the colour half of the graphics
/// state, so failing to clear it on a subsequent `rg` would keep painting
/// the gradient — and the page would look almost right.
#[test]
fn a_later_solid_colour_replaces_the_pattern() {
    let r = render(page(
        &format!("<< /PatternType 2 /Shading {SHADING} >>"),
        "/Pattern cs /P1 scn /DeviceRGB cs 0 1 0 sc 0 0 200 200 re f",
    ));
    let (r0, g0, b0) = px(&r, 100, 100);
    assert!(
        r0 < 20 && g0 > 200 && b0 < 20,
        "the later green must win, got ({r0}, {g0}, {b0})"
    );
    assert_eq!(r.diagnostics.shading.painted, 0, "no gradient was painted");
}

/// **The stale-pattern hazard, and the only fixture that can see it.**
///
/// Selecting a pattern and then a NON-PAINTING space — `/Separation /None`,
/// which §8.6.6.4 says "shall never be painted on the page" — puts the two
/// halves of the colour state into the one combination where a leftover
/// pattern name is destructive: `paints` is false, so the fill site takes
/// the pattern branch, and a name left behind from the earlier `scn` sends
/// it to paint a gradient exactly where the standard requires nothing.
///
/// A test using a normal space cannot detect this. `a_later_solid_colour_
/// replaces_the_pattern` above passes with the clearing REMOVED, because
/// `paints` is true there and the solid path runs regardless — verified by
/// sabotage rather than assumed. Two tests that look like near-duplicates,
/// and only one of them is load-bearing.
#[test]
fn a_pattern_does_not_survive_into_a_non_painting_space() {
    // The four elements of a /Separation array (Table 62), NOT wrapped in a
    // dictionary — an earlier draft wrote `<< … >>` here and then put it
    // inside `[ ]`, producing an array holding one dict. pdfcer correctly
    // reported "colour space is neither a name nor an array", left the
    // space unresolved with `paints = true`, and filled in the default
    // black. The test then measured that black and looked like a real
    // failure of the code under test. A malformed fixture does not fail
    // safe: it fails somewhere else, convincingly.
    let sep_none = "/Separation /None /DeviceRGB \
         << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >>";
    let content = "/Pattern cs /P1 scn /CS0 cs 1 sc 0 0 200 200 re f";
    let page_obj = format!(
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
         << /Pattern << /P1 << /PatternType 2 /Shading {SHADING} >> >> \
         /ColorSpace << /CS0 [{sep_none}] >> >> >>"
    );
    let content_obj = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>",
        ),
        (3, &page_obj),
        (4, &content_obj),
    ]);
    let r = render(bytes);
    // The page must be untouched — white backdrop, nothing painted.
    let (cr, cg, cb) = px(&r, 100, 100);
    assert_eq!(
        (cr, cg, cb),
        (255, 255, 255),
        "/Separation /None must paint nothing, but the page shows ({cr}, {cg}, {cb}) — \
         a coloured pixel means the earlier pattern survived the space change"
    );
    assert_eq!(r.diagnostics.shading.painted, 0, "no gradient was painted");
}
