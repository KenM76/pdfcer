//! # The CAD "line weights off" display mode ([`StrokeDisplay::Hairline`])
//!
//! `pdfcer-gui` request 2026-09-05, asked for by the operator by name:
//! *"show all lines without their thickness — thin lines or something like
//! CAD has."* On a dense A1 sheet at 200–400 % zoom a 0.7 mm stroke is 4–8 px
//! of solid black and adjacent geometry merges into a blob; a draughtsman
//! reads such a drawing with line weights **off**, seeing the geometry rather
//! than the drafting standard. AutoCAD spells it `LWDISPLAY` off, and it is
//! the default there.
//!
//! ## The two conventions this must not confuse
//!
//! | | what it does |
//! |---|---|
//! | **line weights OFF** — what this is | every stroke draws at one device pixel, **whatever** width the file declares |
//! | *enhance thin lines* — Acrobat's preference of that name, **not** this | strokes below one pixel are bumped **up** so they do not vanish |
//!
//! The second makes thin things thicker; this one makes thick things thinner.
//! Shipping the wrong one would be worse than shipping nothing, so
//! [`hairline_never_thickens_anything`] pins the direction directly.
//!
//! ## What the standard does and does not say
//!
//! ISO 32000-1 §8.4.3.2 gives `0 w` the meaning *"the thinnest line that can
//! be rendered at device resolution: 1 device pixel wide"*, and §10.6.4 *Scan
//! Conversion Rules* (§10.7.4 in ISO 32000-2) requires that *"the area covered
//! by painted pixels shall always be at least as large as the area of the
//! original shape"*, explicitly *"both to fill operations and to strokes with
//! nonzero width"* — so that *"no shape ever disappears"*. Those are **floors**
//! and pdfcer honours them in [`StrokeDisplay::Actual`]; `tests/hairline_minimum.rs`
//! is their test.
//!
//! **No clause authorises a ceiling**, and this file does not pretend one
//! does. Hairline display is an operator-selected reading aid applied at
//! render time; it never reaches emitted bytes. (The standard does not even
//! contain the word *hairline* — zero hits in either edition.)
//!
//! ## The three things the requester asked this to be honest about
//!
//! 1. **Fills are untouched** — [`fills_are_never_touched`],
//!    [`a_hatch_of_thin_fills_does_not_vanish`].
//! 2. **An already sub-pixel stroke** — a **CEILING, not a set**:
//!    [`an_already_sub_pixel_stroke_is_left_exactly_alone`],
//!    [`hairline_never_thickens_anything`].
//! 3. **The result line reports it** — `Diagnostics::strokes_hairlined`,
//!    following `subpixel_culled`'s precedent exactly (present either way):
//!    [`the_ceiling_is_counted_whether_or_not_the_mode_is_on`],
//!    [`the_count_is_of_strokes_thinned_not_strokes_drawn`], and the three
//!    reach tests that pin every route into the interpreter —
//!    [`strokes_inside_a_form_xobject_are_capped_and_counted`],
//!    [`the_tally_survives_a_nested_interpreter_and_its_merge`],
//!    [`strokes_in_an_annotation_appearance_are_capped_and_counted`].
//!
//! ## What is deliberately NOT tested here
//!
//! **That exports ignore the mode.** They do — no export path in this crate
//! sets `stroke_display`, and every one of them renders whatever
//! [`RenderOptions`] it is handed — but that is a property of the CALLER, not
//! of this crate: `pdfcer-render` cannot distinguish "render this page for a
//! canvas" from "render this page for a PNG", and silently overriding an
//! option a caller explicitly set would be a worse behaviour than the one it
//! guards against. The shell owns the constraint (the requester states it as
//! theirs), and the type docs on [`StrokeDisplay`] say so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, StrokeDisplay, render_page_with};

// ---------------------------------------------------------------------------
// Fixtures — built in-memory, byte by byte
// ---------------------------------------------------------------------------

/// Assemble a minimal, valid PDF 1.7 file from numbered object bodies.
///
/// Copied in shape from `tests/hairline_minimum.rs` deliberately rather than
/// factored into a shared helper: these two files test opposite halves of the
/// same code path (that one the FLOOR, this one the CEILING), and a shared
/// builder would make a change made for one silently re-aim the other.
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

/// A 100 x 100 page whose content stream is exactly `content`.
fn page_with(content: &str) -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
             /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
        ),
    ])
}

/// A 100 x 100 page that invokes a form XObject whose stream is `form`.
///
/// Exists for one assertion only: a form is a SEPARATE interpreter with its
/// own deferred tally, so this is what proves the fold-and-merge actually
/// crosses the stream boundary rather than counting only the page's own
/// strokes.
fn page_with_form(page_content: &str, form: &str) -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Resources << /XObject << /X1 5 0 R >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{page_content}\nendstream",
                page_content.len() + 1
            ),
        ),
        (
            5,
            &format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] \
                 /Resources << >> /Length {} >>\nstream\n{form}\nendstream",
                form.len() + 1
            ),
        ),
    ])
}

// ---------------------------------------------------------------------------
// Rendering + measurement
// ---------------------------------------------------------------------------

/// `RenderOptions::default()` with only `stroke_display` moved.
///
/// Built by field assignment rather than by a struct expression because
/// [`RenderOptions`] is `#[non_exhaustive]`, and its own type docs name field
/// assignment as the supported way for an out-of-crate caller to reach a
/// field. That is not incidental here: this test lives outside the crate on
/// purpose, so it exercises the option surface exactly as `pdfcer-gui` and
/// `pdfcer` must.
fn opts(display: StrokeDisplay) -> RenderOptions {
    let mut o = RenderOptions::default();
    o.stroke_display = display;
    o
}

fn render(bytes: Vec<u8>, scale: f32, display: StrokeDisplay) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, scale, &opts(display)).expect("render")
}

/// Pixels with any appreciable ink on them.
///
/// Counted rather than measured as contrast because the property under test
/// here is AREA — a fat stroke covers many pixels, a hairline covers a row —
/// which is the opposite of what `hairline_minimum.rs` measures on the same
/// geometry (it asks whether a thin line is DARK, not how much it covers).
fn ink(r: &RenderedPage) -> usize {
    let pm = &r.pixmap;
    (0..pm.height())
        .flat_map(|y| (0..pm.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| pm.pixel(x, y).is_some_and(|p| p.demultiply().red() < 128))
        .count()
}

// ---------------------------------------------------------------------------
// 1. The mode does what it says
// ---------------------------------------------------------------------------

#[test]
fn a_fat_stroke_collapses_to_about_one_device_pixel() {
    // A 12-unit horizontal rule across a 100-unit page: 12 device px tall at
    // scale 1, which is exactly the "adjacent geometry merges into a blob"
    // case the operator described.
    let content = "0 G 12 w 20 50 m 80 50 l S";
    let actual = ink(&render(page_with(content), 1.0, StrokeDisplay::Actual));
    let hair = ink(&render(page_with(content), 1.0, StrokeDisplay::Hairline));

    assert!(
        actual > 0,
        "the fixture must paint at all (actual={actual})"
    );
    // The stroke is 60 units long, so ~60 px per device row. 12 rows vs 1-2.
    assert!(
        hair * 4 < actual,
        "line weights off must collapse a 12 px stroke to a hairline \
         (actual={actual} px inked, hairline={hair} px)"
    );
    assert!(
        hair > 0,
        "…to a hairline, NOT to nothing: §10.6.4's floor still applies \
         underneath the ceiling, and \"no shape ever disappears\" is its \
         stated purpose (hairline={hair})"
    );
}

#[test]
fn the_ceiling_is_computed_in_device_space_so_it_holds_at_every_zoom() {
    // The property that makes this a DEVICE-pixel ceiling rather than a
    // user-space constant: zooming in must not make the hairline thicker.
    // A user-space cap would scale with the zoom and the whole feature would
    // stop working at exactly the magnification the operator asked for it at.
    let content = "0 G 12 w 20 50 m 80 50 l S";
    let one = ink(&render(page_with(content), 1.0, StrokeDisplay::Hairline));
    let four = ink(&render(page_with(content), 4.0, StrokeDisplay::Hairline));

    // The line is 4x LONGER at 4x zoom, so its ink grows ~4x if the width
    // stayed at one device pixel — and ~16x if the width scaled too.
    assert!(
        four < one * 8,
        "the hairline thickened with zoom ({one} px at 1x, {four} px at 4x): \
         a ~16x growth means the cap is being applied in user space"
    );
    assert!(
        four > one * 2,
        "sanity: the line should still get ~4x longer at 4x \
         ({one} px at 1x, {four} px at 4x)"
    );
}

// ---------------------------------------------------------------------------
// 2. Criterion 1 — fills are untouched
// ---------------------------------------------------------------------------

#[test]
fn fills_are_never_touched() {
    // A filled region is GEOMETRY, not a drafting weight. Asserted as
    // byte-identity of the whole pixmap rather than as an ink count, because
    // an ink count would tolerate a fill that moved, changed shape or shifted
    // its anti-aliased edge while covering the same number of pixels.
    let content = "0 0 0 rg 20 20 60 30 re f";
    let a = render(page_with(content), 1.0, StrokeDisplay::Actual);
    let h = render(page_with(content), 1.0, StrokeDisplay::Hairline);
    assert_eq!(
        a.pixmap.data(),
        h.pixmap.data(),
        "hairline display changed a FILL; only S/s/B/B* strokes may change"
    );
    assert_eq!(
        h.diagnostics.strokes_hairlined, 0,
        "a page with no strokes cannot have thinned one"
    );
}

#[test]
fn a_hatch_of_thin_fills_does_not_vanish() {
    // The requester's own worst case, named in the request: "a hatch built
    // from thin fills must not become invisible." CAD exporters routinely
    // emit hatching as narrow filled rectangles rather than as strokes, so
    // the general "fills are untouched" claim above is not enough — this
    // pins the specific shape that would be destroyed by a ceiling applied
    // one function too early.
    let mut content = String::from("0 0 0 rg\n");
    for i in 0..20 {
        // 0.4-unit-wide filled bars: narrower than the one-device-pixel
        // ceiling, which is what makes them the dangerous case.
        content.push_str(&format!("{} 20 0.4 60 re f\n", 10 + i * 4));
    }
    let a = render(page_with(&content), 1.0, StrokeDisplay::Actual);
    let h = render(page_with(&content), 1.0, StrokeDisplay::Hairline);
    assert!(ink(&a) > 0, "the hatch must be visible in the first place");
    assert_eq!(
        a.pixmap.data(),
        h.pixmap.data(),
        "a hatch of thin FILLS changed under hairline display"
    );
}

// ---------------------------------------------------------------------------
// 3. Criterion 2 — a CEILING, not a set
// ---------------------------------------------------------------------------

#[test]
fn an_already_sub_pixel_stroke_is_left_exactly_alone() {
    // THE ANSWER TO THE REQUESTER'S QUESTION 2, asserted rather than stated.
    //
    // `0.1 w` at scale 1 is 0.1 device pixels. pdfcer's §8.4.3.2/§10.6.4
    // floor has already raised it to one device pixel before the ceiling is
    // consulted, so the ceiling's `min` is a no-op — and the two rasters must
    // be byte-identical, not merely similar.
    //
    // Byte-identity is the right assertion because the failure mode this
    // guards against is subtle: an implementation that SET the width instead
    // of capping it would produce a raster that also looks like a hairline,
    // and only an exact comparison distinguishes "left alone" from "coincided".
    for width in ["0", "0.1", "0.4"] {
        let content = format!("0 G {width} w 20 50 m 80 50 l S");
        let a = render(page_with(&content), 1.0, StrokeDisplay::Actual);
        let h = render(page_with(&content), 1.0, StrokeDisplay::Hairline);
        assert_eq!(
            a.pixmap.data(),
            h.pixmap.data(),
            "`{width} w` is already sub-pixel; hairline display must be a \
             CEILING (a no-op here), not a set"
        );
        assert_eq!(
            h.diagnostics.strokes_hairlined, 0,
            "`{width} w` was not thinned, so it must not be counted as thinned"
        );
    }
}

#[test]
fn hairline_never_thickens_anything() {
    // The direction guard, and the reason it is its own test: "enhance thin
    // lines" is the OPPOSITE convention with a confusingly similar name, and
    // an implementation that shipped it instead would pass a naive "does the
    // mode change the raster?" test while doing precisely the wrong thing.
    //
    // Swept across the whole interesting range of widths and both sides of
    // the one-device-pixel boundary at two zooms.
    for scale in [0.5_f32, 1.0, 4.0] {
        for width in ["0", "0.1", "1", "4", "12"] {
            let content = format!("0 G {width} w 20 50 m 80 50 l S");
            let a = ink(&render(page_with(&content), scale, StrokeDisplay::Actual));
            let h = ink(&render(page_with(&content), scale, StrokeDisplay::Hairline));
            assert!(
                h <= a,
                "hairline display INKED MORE at `{width} w`, scale {scale} \
                 (actual={a}, hairline={h}) — that is \"enhance thin lines\", \
                 the opposite convention"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Criterion 3 — the result line reports it
// ---------------------------------------------------------------------------

#[test]
fn the_ceiling_is_counted_whether_or_not_the_mode_is_on() {
    // `subpixel_culled` is the precedent the requester named: a counter that
    // is present on the metrics line either way, so a raster always carries
    // the fact that it is not a faithful one. A hairline raster is invisible
    // as such — it looks like a CAD drawing rather than like a renderer
    // decision — which is exactly when rule 4's disclosure obligation bites.
    let content = "0 G 12 w 20 50 m 80 50 l S";
    let a = render(page_with(content), 1.0, StrokeDisplay::Actual);
    let h = render(page_with(content), 1.0, StrokeDisplay::Hairline);

    assert_eq!(
        a.diagnostics.strokes_hairlined, 0,
        "the default mode must never report a thinned stroke"
    );
    assert_eq!(
        h.diagnostics.strokes_hairlined, 1,
        "one stroke was thinned, so the raster must say so"
    );
}

#[test]
fn the_count_is_of_strokes_thinned_not_strokes_drawn() {
    // Three strokes, only two of them fat. Counting paints instead of
    // reductions would report 3 and would answer a question nobody asked;
    // the operator's question is "how much of this drawing am I reading at a
    // width the file did not ask for?".
    let content = "0 G 12 w 10 20 m 90 20 l S \
                   4 w 10 50 m 90 50 l S \
                   0.1 w 10 80 m 90 80 l S";
    let h = render(page_with(content), 1.0, StrokeDisplay::Hairline);
    assert_eq!(
        h.diagnostics.strokes_hairlined, 2,
        "two of the three strokes were over one device pixel; the sub-pixel \
         one was left alone and must not be counted"
    );
}

#[test]
fn strokes_inside_a_form_xobject_are_capped_and_counted() {
    // CAD exporters put most of a sheet inside form XObjects, so a ceiling
    // that stopped at the page's own content stream would do nothing on
    // precisely the documents this feature exists for.
    //
    // `Interpreter::do_form` runs the form's content through a nested
    // interpreter whose diagnostics are merged back, so this exercises the
    // reach of the ceiling and one merge. It is not the only merge shape —
    // see the transparency-group test below — and the distinction is written
    // down because sabotage found it: the two tally folds in `interpret.rs`
    // are NOT equally load-bearing, and a reader could reasonably assume from
    // this test that they were.
    let h = render(
        page_with_form(
            "0 G 12 w 10 20 m 90 20 l S /X1 Do",
            "0 G 8 w 10 70 m 90 70 l S",
        ),
        1.0,
        StrokeDisplay::Hairline,
    );
    assert_eq!(
        h.diagnostics.strokes_hairlined, 2,
        "the stroke inside the form XObject was not thinned and counted"
    );
}

#[test]
fn the_tally_survives_a_nested_interpreter_and_its_merge() {
    // THE CROSS-INTERPRETER CASE, and it needs a construction that actually
    // forces one. A transparency group with a non-opaque constant alpha is
    // rendered into its own buffer by a SEPARATE interpreter (`run_nested`),
    // whose `Diagnostics` are then folded back with `Diagnostics::merge`.
    //
    // Two links in that chain can break independently and neither is visible
    // in the raster: the nested interpreter's deferred tally may never be
    // folded into its own `Diagnostics`, or `merge` may not carry the field.
    // Both would show up as an under-count on real drawings — exactly the
    // half-truth rule 4 exists to prevent — while the picture stayed correct.
    let form = "0 G 8 w 10 70 m 90 70 l S";
    let content = "/GS0 gs 0 G 12 w 10 20 m 90 20 l S /X1 Do";
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /XObject << /X1 5 0 R >> /ExtGState << /GS0 6 0 R >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
        ),
        (
            5,
            &format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] \
                 /Group << /S /Transparency /CS /DeviceRGB /I true >> \
                 /Resources << >> /Length {} >>\nstream\n{form}\nendstream",
                form.len() + 1
            ),
        ),
        // The constant alpha is what makes the group buffered rather than
        // painted inline — see §11.6.6 and `Interpreter::do_form`.
        (6, "<< /Type /ExtGState /ca 0.5 /CA 0.5 >>"),
    ]);

    let h = render(bytes, 1.0, StrokeDisplay::Hairline);
    assert_eq!(
        h.diagnostics.transparency_groups_composited, 1,
        "fixture check: this test is worthless unless the group really was \
         composited through a nested interpreter"
    );
    assert_eq!(
        h.diagnostics.strokes_hairlined, 2,
        "a stroke thinned inside a nested interpreter did not reach the \
         page's diagnostics — the deferred tally is not being folded, or \
         `Diagnostics::merge` is not carrying the field"
    );
}

#[test]
fn strokes_in_an_annotation_appearance_are_capped_and_counted() {
    // A separate top-level entry into the interpreter: an annotation's `/AP`
    // `/N` stream (§12.5.5) is run by `annot.rs` through `run_form_at_on`,
    // not by the page's content-stream walk, and its diagnostics reach the
    // page by a `Diagnostics::merge` of their own.
    //
    // It matters beyond tidiness. Markup on a CAD sheet — leaders, revision
    // clouds, and pdfcer's own ce dimensions — is drawn by appearance
    // streams, so an operator reading a marked-up drawing with line weights
    // off would see the drawing thin while the markup stayed fat, and the
    // count would under-report by exactly the annotations.
    let ap = "0 G 12 w 5 40 m 75 40 l S";
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
             /Resources << >> >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Annots [5 0 R] >>",
        ),
        (4, "<< /Length 1 >>\nstream\n \nendstream"),
        (
            5,
            "<< /Type /Annot /Subtype /Square /Rect [10 10 90 90] /F 4 \
             /AP << /N 6 0 R >> >>",
        ),
        (
            6,
            &format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 80 80] \
                 /Resources << >> /Length {} >>\nstream\n{ap}\nendstream",
                ap.len() + 1
            ),
        ),
    ]);

    let a = render(bytes.clone(), 1.0, StrokeDisplay::Actual);
    let h = render(bytes, 1.0, StrokeDisplay::Hairline);

    assert_eq!(
        a.diagnostics.annotations_painted, 1,
        "fixture check: the appearance stream must actually be painted, or \
         this test proves nothing"
    );
    assert_eq!(
        a.diagnostics.strokes_hairlined, 0,
        "the default mode must not thin an appearance stream either"
    );
    assert_eq!(
        h.diagnostics.strokes_hairlined, 1,
        "the stroke in the annotation's appearance stream was not thinned \
         and counted"
    );
    assert!(
        ink(&h) < ink(&a),
        "the markup did not actually thin (actual={}, hairline={})",
        ink(&a),
        ink(&h)
    );
}

#[test]
fn stroked_text_is_covered_by_the_same_ceiling() {
    // §9.3.6: "the graphics state parameters affecting those operations, such
    // as line width, shall be interpreted in user space rather than in text
    // space" — so a stroked glyph uses the same line-width parameter a path
    // does, and must thin with the drawing. If it did not, a stroked title
    // block would stay fat while everything around it went to a hairline,
    // which is the single most visible way this feature could look broken.
    //
    // Render mode 1 (Tr 1) = stroke only.
    let content = "BT /F1 48 Tf 1 Tr 8 w 10 40 Td (H) Tj ET";
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /Font << /F1 5 0 R >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
        ),
        (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]);

    let h = render(bytes.clone(), 1.0, StrokeDisplay::Hairline);
    let a = render(bytes, 1.0, StrokeDisplay::Actual);
    assert!(
        a.diagnostics.strokes_hairlined == 0,
        "the default mode must not thin stroked text either"
    );
    assert!(
        h.diagnostics.strokes_hairlined > 0,
        "a stroked glyph at `8 w` must be thinned and counted (§9.3.6 makes \
         it the same line-width parameter a path stroke uses)"
    );
    assert!(
        ink(&h) < ink(&a),
        "stroked text did not actually thin (actual={}, hairline={})",
        ink(&a),
        ink(&h)
    );
}

// ---------------------------------------------------------------------------
// 5. The default
// ---------------------------------------------------------------------------

#[test]
fn actual_is_the_default_so_no_existing_caller_can_notice_this_exists() {
    assert_eq!(
        StrokeDisplay::default(),
        StrokeDisplay::Actual,
        "faithful widths must be the default; a shell opts its CANVAS in, \
         and no export ever does"
    );
    assert_eq!(
        RenderOptions::default().stroke_display,
        StrokeDisplay::Actual
    );

    // And the default really renders the declared width, rather than the
    // default happening to be spelled `Actual` while the ceiling applies
    // anyway. A 12-unit rule must cover roughly 12 rows of pixels.
    let content = "0 G 12 w 20 50 m 80 50 l S";
    let doc = Document::from_bytes(page_with(content)).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let r = render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render");
    assert!(
        ink(&r) > 400,
        "the default rendered only {} inked pixels for a 12x60 unit rule — \
         the ceiling is leaking into `Actual`",
        ink(&r)
    );
}
