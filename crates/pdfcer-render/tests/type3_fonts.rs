//! `Pass 126.0` — Type 3 fonts render, and this file is the end-to-end
//! evidence that the four rules that are invisible in a casual render are
//! honoured.
//!
//! # Why an integration test rather than more unit tests
//!
//! `crates/pdfcer-render/src/type3.rs` unit-tests the pieces that are pure
//! arithmetic: the width through `FontMatrix` (including the rotation
//! case), the all-zero `/FontBBox` sentinel, the total encoding. Those
//! prove the **model**.
//!
//! They cannot prove the model is **reached**. Between a correct
//! `Type3Font` and a painted pixel sit the font dispatch, the empty
//! `data` field a Type 3 font necessarily carries, the show path's early
//! exit, the glyph-procedure transform, a nested interpreter, the
//! `/Resources` fallback and a recursion guard. A mistake in any of them
//! renders *something*, which is the failure mode this project keeps
//! meeting and which no unit test can see.
//!
//! # The controls are the design, not decoration
//!
//! Every assertion below is paired with one that must come out
//! **differently**. A `d1` glyph coming out blue proves nothing on its
//! own — a renderer that painted every Type 3 glyph in the text colour
//! would pass it. It is the `d0` twin coming out **red**, from the
//! identical colour operator on the identical box over the identical
//! page colour, that carries the result.
//!
//! # The Acrobat half, and why it is stated here rather than asserted
//!
//! Rows A–D of `colour_and_advance.pdf` were rendered in Acrobat Reader
//! on 2026-08-25, because the Acrobat-parity corpus had the `d1` colour
//! question recorded as an open `GAP` — no source established whether
//! Acrobat honours the clause or lets the procedure win. It honours it:
//! A blue, C red, D blue. So pdfcer follows §9.6.5 here **knowing** the
//! clause and Acrobat agree, rather than hoping.
//!
//! That measurement is not re-run as a test. It is a fact about another
//! program on one machine on one day, and pinning it in `cargo test`
//! would make this suite fail when Acrobat changes rather than when
//! pdfcer does.
//!
//! Fixtures and their construction: `tools/gen-type3-fixtures.py`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

/// The fixtures are 612×720 pt; everything is measured in those units.
const SCALE: f32 = 2.0;

fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/type3")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], SCALE).expect("renders")
}

/// The pixel at a point in the fixtures' own page coordinates.
fn at(p: &RenderedPage, x_pt: f32, y_pt: f32) -> (u8, u8, u8) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let x = (x_pt * SCALE) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let y = ((720.0 - y_pt) * SCALE) as u32;
    let px = p.pixmap.pixel(x, y).expect("point is on the page");
    (px.red(), px.green(), px.blue())
}

/// The horizontal runs of blue on a scanline, in page points.
///
/// Used to measure an ADVANCE, which is the only way to check the width
/// rule end to end: a glyph positioned by `Td` never consults `/Widths`
/// at all.
fn blue_runs(p: &RenderedPage, y_pt: f32) -> Vec<(f32, f32)> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let y = ((720.0 - y_pt) * SCALE) as u32;
    let mut runs = Vec::new();
    let mut start: Option<u32> = None;
    for x in 0..p.pixmap.width() {
        let px = p.pixmap.pixel(x, y).expect("in bounds");
        let is_blue = px.blue() > 200 && px.red() < 60;
        match (is_blue, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s as f32 / SCALE, x as f32 / SCALE));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s as f32 / SCALE, p.pixmap.width() as f32 / SCALE));
    }
    runs
}

// ===========================================================================
// Table 113 — where a glyph's colour comes from
// ===========================================================================

/// `d1` ignores the procedure's own colour; `d0` keeps it.
///
/// Table 113: a `d1` glyph description "should not execute any operators
/// that set the colour … any use of such operators **shall be ignored**.
/// The glyph description is executed solely to determine the glyph's
/// shape. Its colour shall be determined by the graphics state in effect
/// each time this glyph is painted." `d0` declares the opposite — "both
/// its shape and its colour".
///
/// The fixture sets the page colour to blue **once**, before the text
/// object, and puts an identical `1 0 0 rg` inside two identical boxes.
/// The only difference is the first operator.
///
/// Row B is what licenses reading row A as a result: without it, a blue
/// row A could equally mean "this renderer paints every Type 3 glyph
/// blue".
#[test]
fn d1_ignores_the_procedures_colour_and_d0_keeps_it() {
    let p = render("colour_and_advance.pdf");
    assert_eq!(
        at(&p, 70.0, 654.0),
        (0, 0, 255),
        "row A: a d1 glyph must take the graphics-state colour, not the red \
         its own procedure set"
    );
    assert_eq!(
        at(&p, 70.0, 574.0),
        (0, 0, 255),
        "row B (control): a d1 glyph with no colour operator at all must be blue"
    );
    assert_eq!(
        at(&p, 70.0, 494.0),
        (255, 0, 0),
        "row C: a d0 glyph declares its own colour and must be RED. If this is \
         blue, the colour gag is being applied to d0 as well and the two \
         operators have been collapsed into one"
    );
}

/// The gag is counted, and counted **once** — which is the discriminating
/// number.
///
/// The fixture contains two colour operators inside glyph procedures: one
/// in a `d1` (ignored) and one in a `d0` (honoured). A renderer that
/// suppressed colour in both would paint row C blue *and* report 2 here;
/// one that suppressed neither reports 0. Only the correct
/// implementation reports 1.
///
/// Rule 4: the ignore is the standard's defined behaviour rather than a
/// shortfall, but it is still pdfcer doing something the file's bytes did
/// not ask for, so it is disclosed rather than silent.
#[test]
fn the_ignored_colour_operator_is_counted_and_only_the_d1_one() {
    let p = render("colour_and_advance.pdf");
    assert_eq!(
        p.diagnostics.type3_colors_ignored, 1,
        "exactly one colour operator sits inside a d1 procedure; the other is \
         inside a d0 and must be honoured"
    );
}

/// The bitmap flavour: a `d1` procedure whose body is an inline
/// `ImageMask`.
///
/// §9.6.5 forbids a glyph description from including an *image* and
/// permits an **image mask**, "since it merely defines a region of the
/// page to be painted with the current colour" — which makes this the
/// vehicle every bitmap Type 3 font uses (old TeX/`dvips` output, PK
/// conversions, some fax-to-PDF converters).
///
/// The mask is an 8×8 hollow square with a diagonal, written with
/// `/Decode [1 0]` so a **set** bit paints — the inverse of the default,
/// and the polarity real producers emit. Asymmetric on purpose: a
/// transposed or bit-reversed decode would be visible rather than
/// plausible.
#[test]
fn a_bitmap_glyph_paints_its_stencil_in_the_graphics_state_colour() {
    let p = render("colour_and_advance.pdf");
    // The glyph's box is 28 pt from its origin at (60, 400). The mask's
    // top and bottom rows are solid, so the top edge is blue across.
    let runs = blue_runs(&p, 426.0);
    assert!(
        !runs.is_empty(),
        "the image-mask glyph painted nothing: the bitmap flavour is not \
         reaching the inline-image path"
    );
    let (lo, hi) = runs[0];
    assert!(
        (lo - 60.0).abs() < 2.0 && (hi - 88.0).abs() < 2.0,
        "the stencil must span the glyph's own 28 pt box at its origin; got \
         {lo}..{hi}"
    );
}

/// `Pass 126.1` — a MAGNIFIED bitmap glyph is a stencil and is never
/// smoothed.
///
/// Acrobat Reader does not interpolate a bitmap Type 3 glyph on the way
/// up: it gets jaggier as you zoom in. The measurement is a colour census
/// inside the glyph's box: a nearest-neighbour stencil shows exactly two
/// colours, and one intermediate pixel fails it.
///
/// The mask is 8x8 samples in a 28 pt box, so every scale here draws it
/// larger than its grid. Shrunk glyphs are area-averaged instead
/// (`a_shrunk_bitmap_glyph_keeps_a_stroke_thinner_than_the_ratio`).
#[test]
fn a_magnified_bitmap_glyph_is_never_smoothed() {
    for scale in [1.0f32, 4.0, 16.0] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/type3/colour_and_advance.pdf");
        let doc = Document::from_bytes(std::fs::read(&path).expect("fixture file"))
            .expect("fixture parses");
        let pages = page_tree::pages(&doc).expect("page tree");
        let p = pdfcer_render::render_page(&doc, &pages[0], scale).expect("renders");

        // The mask glyph's box, in page points, inset by one point so the
        // silhouette's own edge pixels are not the thing being counted.
        let mut seen: Vec<(u8, u8, u8)> = Vec::new();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x0, x1) = ((61.0 * scale) as u32, (87.0 * scale) as u32);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (y0, y1) = (
            ((720.0 - 427.0) * scale) as u32,
            ((720.0 - 401.0) * scale) as u32,
        );
        for y in y0..y1.max(y0 + 1) {
            for x in x0..x1.max(x0 + 1) {
                let Some(px) = p.pixmap.pixel(x, y) else {
                    continue;
                };
                let c = (px.red(), px.green(), px.blue());
                if !seen.contains(&c) {
                    seen.push(c);
                }
            }
        }
        assert!(
            seen.len() <= 2,
            "at scale {scale} the stencil shows {} distinct colours; more than two means it was interpolated, and a bitmap glyph is a region of the page to be painted rather than a picture to be resampled: {seen:?}",
            seen.len()
        );
    }
}

/// A one-page 40x40 pt PDF showing one `d1` Type 3 glyph at 8 pt, at
/// (10, 10). The glyph is a 40x40 1-bit image mask, the shape a 300-600 dpi
/// TeX PK font emits, painting only rows 19 and 20: a horizontal bar two
/// samples high.
fn pk_style_glyph_pdf() -> Vec<u8> {
    let mut mask = vec![0xFFu8; 5 * 40];
    for row in [19usize, 20] {
        mask[row * 5..row * 5 + 5].fill(0x00);
    }
    let content = b"BT /F1 8 Tf 10 10 Td (A) Tj ET".to_vec();
    let proc = b"40 0 0 0 40 40 d1 40 0 0 40 0 0 cm /Im0 Do".to_vec();
    let stream = |dict: &str, data: &[u8]| {
        let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        s.extend_from_slice(data);
        s.extend_from_slice(b"\nendstream");
        s
    };
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        stream("", &content),
        b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 40 40] /FontMatrix [0.025 0 0 0.025 0 0] /CharProcs << /A 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [40] /Resources << /XObject << /Im0 7 0 R >> >> >>".to_vec(),
        stream("", &proc),
        stream(
            "/Type /XObject /Subtype /Image /Width 40 /Height 40 /ImageMask true /BitsPerComponent 1",
            &mask,
        ),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

fn render_pk_glyph(scale: f32) -> RenderedPage {
    let doc = Document::from_bytes(pk_style_glyph_pdf()).expect("synthetic PDF parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], scale).expect("renders")
}

/// X008 — a SHRUNK bitmap glyph is area-averaged, so a stroke thinner than
/// the reduction ratio survives.
///
/// At scale 1 the 40-sample glyph is 8 device px high (5:1), and at 2 it is
/// 16 px (2.5:1). Point sampling reads one sample row in five and misses a
/// two-row bar outright, which is how an `e` loses its bar in a pdfTeX PK
/// font. Acrobat Reader shows such glyphs grey-edged with every stroke
/// present. The assertion: some device row across the glyph's interior is
/// inked at every column.
#[test]
fn a_shrunk_bitmap_glyph_keeps_a_stroke_thinner_than_the_ratio() {
    for scale in [1.0f32, 2.0] {
        let p = render_pk_glyph(scale);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x0, x1, y0, y1) = (
            (11.0 * scale) as u32,
            (17.0 * scale) as u32,
            ((40.0 - 18.0) * scale) as u32,
            ((40.0 - 10.0) * scale) as u32,
        );
        let inked_row = (y0..y1).find(|&y| {
            (x0..x1).all(|x| {
                let px = p.pixmap.pixel(x, y).expect("in bounds");
                px.red() < 250
            })
        });
        assert!(
            inked_row.is_some(),
            "at scale {scale} no device row across the glyph is inked: the two-sample bar was dropped by point sampling"
        );
    }
}

/// X008's magnified twin: the same PK-style glyph drawn at 16x (3.2 device
/// px per sample) stays a two-colour stencil.
#[test]
fn the_pk_style_glyph_magnified_is_still_two_colours() {
    let scale = 16.0f32;
    let p = render_pk_glyph(scale);
    let mut seen: Vec<(u8, u8, u8)> = Vec::new();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x0, x1, y0, y1) = (
        (11.0 * scale) as u32,
        (17.0 * scale) as u32,
        ((40.0 - 17.0) * scale) as u32,
        ((40.0 - 11.0) * scale) as u32,
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let px = p.pixmap.pixel(x, y).expect("in bounds");
            let c = (px.red(), px.green(), px.blue());
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
    }
    assert_eq!(seen.len(), 2, "magnified stencil colours: {seen:?}");
}

// ===========================================================================
// Table 112 — the width rule, which no `Td`-positioned glyph can test
// ===========================================================================

/// THE NUMBER-ONE TYPE 3 BUG, end to end.
///
/// Table 112: Type 3 widths "shall be interpreted in glyph space as
/// specified by `FontMatrix` (**unlike** the widths of a Type 1 font,
/// which are in thousandths of a unit of text space)".
///
/// Row E shows **four glyphs in one string**, so the pen is moved by
/// `/Widths` and by nothing else. 750 glyph units under a
/// `[0.001 …]` matrix at 40 pt is a **30 pt** advance, and the box is 400
/// units = **16 pt**. So: four 16 pt boxes at 30 pt pitch.
///
/// A renderer that divides by 1000 *as well* advances 0.03 pt and stacks
/// all four glyphs into one box — which `blue_runs` reports as a single
/// run, not four.
///
/// Rows A–D cannot catch this, and that is why row E exists: they
/// position every glyph with `Td`, so a renderer that ignored `/Widths`
/// entirely would render them perfectly.
#[test]
fn four_glyphs_in_one_string_advance_by_the_width_through_the_font_matrix() {
    let p = render("colour_and_advance.pdf");
    let runs = blue_runs(&p, 334.0);
    assert_eq!(
        runs.len(),
        4,
        "four glyphs must produce four separate boxes; {} run(s) means the \
         advance is wrong (one run = the /1000 mistake, stacking them)",
        runs.len()
    );
    for (i, (lo, hi)) in runs.iter().enumerate() {
        assert!(
            (hi - lo - 16.0).abs() < 1.0,
            "box {i} is {:.2} pt wide, want 16 pt (400 glyph units at 40 pt)",
            hi - lo
        );
    }
    for i in 0..3 {
        let pitch = runs[i + 1].0 - runs[i].0;
        assert!(
            (pitch - 30.0).abs() < 1.0,
            "advance {i} is {pitch:.2} pt, want 30 pt (750 glyph units at 40 pt)"
        );
    }
}

/// §9.6.5 step (b): a missing glyph paints nothing **and still advances**.
///
/// The clause says "if the name is not present as a key in `CharProcs`,
/// no glyph shall be painted" and stops there. `/Widths` supplies the
/// advance independently, so the pen still moves.
///
/// The fixture shows three codes as one string; the middle one is named
/// in `/Differences` and absent from `/CharProcs`. So there must be
/// **two** boxes, and the second must sit **two** advances (60 pt) from
/// the first. A renderer that treats a missing glyph as a skipped code
/// puts it at 30 pt, and every later glyph on the line is wrong by the
/// same amount — which reads as a layout bug rather than a missing
/// glyph, and is the more expensive way to be wrong.
#[test]
fn a_missing_glyph_paints_nothing_and_still_moves_the_pen() {
    let p = render("missing_glyph_advances.pdf");
    assert_eq!(
        p.diagnostics.type3_glyphs_missing, 1,
        "the middle code's glyph is absent and must be reported"
    );
    let runs = blue_runs(&p, 614.0);
    assert_eq!(runs.len(), 2, "two of the three codes have a glyph");
    let gap = runs[1].0 - runs[0].0;
    assert!(
        (gap - 60.0).abs() < 1.0,
        "the third glyph must be TWO advances along ({gap:.2} pt, want 60). \
         30 pt means the missing code was skipped instead of advanced"
    );
}

// ===========================================================================
// Table 112 — resources, and the fallback that is easy to miss
// ===========================================================================

/// A glyph procedure finds a resource the FONT does not carry.
///
/// Table 112: "If any glyph descriptions refer to named resources but
/// this dictionary is absent, the names shall be looked up in the
/// resource dictionary of the **page** on which the font is used."
///
/// The fixture's font has no `/Resources` at all and its glyph invokes a
/// form XObject by name; the name resolves only through the page. Old
/// files routinely omit the entry, so a reader without the fallback
/// reports "resource not found" on a document that is perfectly
/// well-formed.
#[test]
fn a_glyph_procedure_falls_back_to_the_pages_resources() {
    let p = render("resources_fallback.pdf");
    assert_eq!(
        p.diagnostics.forms_rendered, 1,
        "the glyph's `Do` must resolve through the page's /XObject"
    );
    assert_eq!(
        at(&p, 70.0, 614.0),
        (0, 0, 255),
        "the form the glyph invoked must be painted, in the text colour"
    );
}

// ===========================================================================
// ARCHITECTURE.md §10.1 — the recursion that the standard permits
// ===========================================================================

/// A glyph procedure that shows its own font terminates.
///
/// §9.6.5 does not forbid a glyph description from showing text in the
/// same Type 3 font, and Annex C sets no limit — so an unbounded reader
/// has a **guaranteed stack overflow sitting in the standard**. This
/// fixture is that input.
///
/// Two assertions, and the second is the one that keeps the first
/// honest: it must **terminate**, and it must still have **drawn**. A
/// reader that refused Type 3 outright, or that bailed at the first
/// nesting, also terminates.
///
/// 64 is [`pdfcer_render::interpret::MAX_XOBJECT_DEPTH`], shared with
/// form XObjects on purpose: the two nest through each other (a glyph
/// may `Do` a form; a form may show a Type 3 string), so two independent
/// budgets would each stay under their own limit while the real stack
/// did not.
#[test]
fn a_self_recursive_glyph_is_bounded_and_still_draws() {
    let p = render("self_recursive.pdf");
    assert_eq!(
        p.diagnostics.type3_glyph_procs_run,
        pdfcer_render::interpret::MAX_XOBJECT_DEPTH,
        "one shown character must expand to exactly the depth limit's worth of \
         procedures - fewer means it stopped early, more means the guard is \
         not the one form XObjects use"
    );
    assert!(
        p.diagnostics.xobject_depth_overflows > 0,
        "hitting the limit must be DISCLOSED, not silently absorbed"
    );
    assert_eq!(
        at(&p, 66.0, 614.0),
        (0, 0, 255),
        "the outermost glyph must still be painted: a bounded reader draws \
         what it can rather than refusing the whole string"
    );
}

// ===========================================================================
// The census counter
// ===========================================================================

/// The glyph census counts procedures **run**, not codes shown.
///
/// `colour_and_advance.pdf` shows eight codes — four singly by `Td` and
/// four in row E's one string — and every one has a procedure. A count
/// of 8 therefore says the show path reached the runner for each of
/// them, which a count of 5 (one per `Tj`) would not.
#[test]
fn the_glyph_census_counts_every_procedure_run() {
    let p = render("colour_and_advance.pdf");
    assert_eq!(p.diagnostics.type3_glyph_procs_run, 8);
    assert_eq!(p.diagnostics.type3_glyphs_missing, 0);
}
