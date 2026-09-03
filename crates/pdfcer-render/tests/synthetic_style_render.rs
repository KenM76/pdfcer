//! # The renderer-capability gate for synthetic bold/italic (Pass 19.2)
//!
//! ## Why this file exists, and why it had to be written FIRST
//!
//! Decision 019 §6 slice 19.2 opens with a prerequisite, not with scope:
//!
//! > **Prerequisite check before starting:** confirm `pdfcer-render` honours
//! > **text rendering mode 2** (fill-then-stroke) and a sheared `Tm`. If it
//! > does not, **preview ≠ saved** and R85 is violated the moment synthesis
//! > ships — fix the renderer first or descope synthesis from this slice.
//!
//! The reason that gate is sharp is R85 (preview-equals-saved): pdfcer's
//! synthetic bold and italic are *emission* choices — a `2 Tr` with a stroke
//! width, and a shear premultiplied into the run's `Tm` — and pdfcer's own
//! rasterizer is what the operator sees before the file is saved. If the
//! rasterizer silently ignored `Tr 2`, faux bold would look like ordinary
//! text on screen and arrive bold in every other viewer. If it dropped the
//! `c` term of a `Tm`, faux italic would be invisible in preview and oblique
//! in the file. Both are R85 violations that no *byte*-level test can catch,
//! because the bytes would be perfectly correct.
//!
//! A code reading is not proof of either. `interpret.rs` *looks* like it
//! honours both, and the point of this file is that "looks like" is exactly
//! the belief that ships the bug. So each case below constructs a PDF,
//! rasterizes it through the shipped public API, and interrogates the
//! **pixels**.
//!
//! ## What each case proves, and by what measurement
//!
//! | Case | Content stream difference | Pixel-level claim |
//! |---|---|---|
//! | [`render_mode_2_strokes_as_well_as_fills`] | `2 Tr` + `2 w` | strictly MORE ink than the plain fill, and the extra ink lies OUTSIDE the filled glyph's silhouette |
//! | [`render_mode_2_stroke_uses_the_stroking_colour_not_the_fill`] | `2 Tr` + `1 0 0 RG` on black fill | RED pixels appear — proving §9.3.6's "stroking colour" rule is implemented, and simultaneously proving the hazard is real |
//! | [`a_sheared_tm_obliques_the_glyph`] | `Tm` `c` = tan 12° | ink at the glyph's TOP moves right; ink at the BASELINE does not |
//! | [`rise_shifts_the_glyph_up_the_page`] | `12 Ts` | the whole glyph's ink moves up by ~12 pt at scale 1.0 |
//!
//! The third case's *two* claims are what make it a shear rather than a
//! translation: a translation would move the baseline ink too. Measuring only
//! "the ink moved" would pass for a bug that offset the whole run.
//!
//! ## Why the measurements are shape statistics, not a reference raster
//!
//! There is no committed golden PNG here on purpose. A reference image would
//! bind this gate to the exact bundled fallback face, its hinting, and
//! tiny-skia's anti-aliasing — all of which may legitimately change without
//! the *capability* under test changing. What must not change is that mode 2
//! adds ink outside the fill, that the stroke takes the stroking colour, and
//! that a `Tm` shear leans the glyph. Those are stated here as inequalities
//! over ink masks, which is both the honest claim and the durable one.
//!
//! ## Spec citations
//!
//! - **§9.3.6, Table 106** — text rendering mode 2 is "Fill, then stroke
//!   text". Mode 0 fills only.
//! - **§9.3.6** — "the line width shall be interpreted in *user space*"
//!   for stroked text, and the stroke uses the current **stroking** colour
//!   (set by `RG`/`G`/`K`), not the non-stroking colour the fill uses.
//! - **§8.2, Table 51 / Figure 9** — a text object admits *general graphics
//!   state* (`w`), *colour* (`RG`), *text state* (`Tr`, `Ts`) and
//!   *text positioning* (`Tm`). It does **not** admit *special graphics
//!   state* (`q`, `Q`, `cm`) — which is why every operator these fixtures
//!   place inside `BT … ET` is legal and why the scoping mechanism pdfcer
//!   uses in `text_edit::format` cannot be `q`/`Q`.
//! - **§9.4.2, Table 108** — `Tm` sets the text matrix to
//!   `[a b c d e f]`; the `c` operand is the term that maps `y` into `x`,
//!   i.e. the horizontal shear that leans a glyph.
//! - **§9.3.7** — `Ts` "shall move the baseline up or down", entering the
//!   text rendering matrix as a translation.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::render_page;
use pdfcer_render::tiny_skia::Pixmap;

/// Device pixels per PDF point. 1.0 (72 DPI) keeps the gate fast; the
/// glyph is drawn at 48 pt so it is ~35 px tall, which is ample for the
/// inequalities below.
const SCALE: f32 = 1.0;

/// A one-page PDF whose single content stream is `content`, carrying one
/// non-embedded `/Helvetica` simple font as `/F1`.
///
/// Deliberately hand-built rather than loaded from `fixtures/synthetic`:
/// this gate is about the *renderer*, and the four content streams differ by
/// one operator each. Keeping them adjacent in one file is what makes the
/// comparison legible; a committed fixture per case would hide the single
/// changed token in a diff of PDF structure. Everything here is synthetic,
/// so `LEGAL.md` §5 is satisfied by construction.
///
/// The font is non-embedded on purpose: `FontEnvironment::bundled()` then
/// substitutes a deterministic bundled face (decision 012, R63), so the
/// rasterized shapes are identical on every machine without shipping a font
/// program in the test.
fn one_glyph_pdf(content: &str) -> Vec<u8> {
    let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
    objects.push((1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()));
    objects.push((
        2,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] \
          /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
    ));
    objects.push((
        3,
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
    ));
    let body = content.as_bytes();
    let mut stream = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    stream.extend_from_slice(body);
    stream.extend_from_slice(b"\nendstream");
    objects.push((4, stream));
    objects.push((
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ));

    let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = std::collections::BTreeMap::new();
    for (num, obj) in &objects {
        offsets.insert(*num, out.len());
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(obj);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for num in 1..=5u32 {
        let off = offsets.get(&num).copied().unwrap_or(0);
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Rasterize a content stream's only page.
fn raster(content: &str) -> Pixmap {
    let doc = Document::from_bytes(one_glyph_pdf(content)).expect("probe PDF parses");
    let pages = page_tree::pages(&doc).expect("probe page tree walks");
    render_page(&doc, &pages[0], SCALE)
        .expect("probe page rasterizes")
        .pixmap
}

/// One pixel's RGBA at `(x, y)`.
fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * p.width() + x) * 4) as usize;
    let d = p.data();
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

/// Whether a pixel carries ink — i.e. is not the white page.
///
/// The threshold is generous (any channel below 250) because the claim is
/// "something was painted here", and anti-aliased glyph edges are painted
/// very lightly. A tighter threshold would make the inequalities below
/// depend on the rasterizer's AA ramp rather than on the capability.
fn inked(p: &Pixmap, x: u32, y: u32) -> bool {
    let [r, g, b, _] = px(p, x, y);
    r < 250 || g < 250 || b < 250
}

/// The set of inked pixel coordinates.
fn ink_mask(p: &Pixmap) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in 0..p.height() {
        for x in 0..p.width() {
            if inked(p, x, y) {
                out.push((x, y));
            }
        }
    }
    out
}

/// The horizontal centre of the ink on one raster row, or `None` if the row
/// is blank. Used to distinguish a *shear* (rows move by different amounts)
/// from a *translation* (all rows move equally).
fn row_ink_centre(p: &Pixmap, y: u32) -> Option<f64> {
    let xs: Vec<u32> = (0..p.width()).filter(|&x| inked(p, x, y)).collect();
    if xs.is_empty() {
        return None;
    }
    Some(f64::from(xs.iter().sum::<u32>()) / xs.len() as f64)
}

/// The vertical extent of the ink, as `(min_y, max_y)`.
fn ink_rows(p: &Pixmap) -> Option<(u32, u32)> {
    let rows: Vec<u32> = (0..p.height())
        .filter(|&y| (0..p.width()).any(|x| inked(p, x, y)))
        .collect();
    Some((*rows.first()?, *rows.last()?))
}

/// A plain filled glyph — the control every case is measured against.
/// `Tr` is left at its Table 105 initial value of 0 (fill only).
const PLAIN: &str = "BT /F1 48 Tf 40 60 Td (H) Tj ET\n";

/// **Mode 2 must add ink.** `2 Tr` with a 2-point line width fills the glyph
/// exactly as mode 0 does and then strokes its outline (§9.3.6 Table 106), so
/// the painted region must strictly grow, and it must grow *outward* — the
/// new ink cannot be explained by the fill alone.
///
/// This is the load-bearing half of the synthetic-bold prerequisite. If this
/// fails, `Tr 2` is being parsed and discarded, faux bold is invisible in
/// preview, and R85 breaks the moment it ships.
#[test]
fn render_mode_2_strokes_as_well_as_fills() {
    let plain = raster(PLAIN);
    let stroked = raster("BT /F1 48 Tf 2 Tr 2 w 40 60 Td (H) Tj ET\n");

    let plain_ink = ink_mask(&plain);
    let stroked_ink = ink_mask(&stroked);
    assert!(
        !plain_ink.is_empty(),
        "the control case painted nothing at all — the probe PDF, not the \
         rendering mode, is what is broken"
    );
    assert!(
        stroked_ink.len() > plain_ink.len(),
        "Tr 2 painted {} inked pixels but a plain fill painted {} — the \
         rasterizer is NOT honouring text rendering mode 2 (§9.3.6 Table \
         106). Synthetic bold cannot ship: it would be invisible in preview \
         and bold in the saved file (R85).",
        stroked_ink.len(),
        plain_ink.len()
    );

    // The growth must be OUTWARD. A mode that merely re-filled more darkly
    // would also raise the count under an "inked" predicate this generous.
    let outside = stroked_ink
        .iter()
        .filter(|c| !plain_ink.contains(c))
        .count();
    assert!(
        outside > 20,
        "Tr 2 added only {outside} inked pixels outside the filled glyph's \
         silhouette; a 2-point stroke on a 48-point glyph must add a visible \
         outline, not a darker fill"
    );
}

/// **§9.3.6's named hazard, proven to be real.** Stroked text takes the
/// current **stroking** colour, which is set by `RG`/`G`/`K` and is a
/// *different* graphics-state parameter from the `rg`/`g`/`k` fill colour.
///
/// The fixture paints a black-filled glyph and sets the stroking colour to
/// red. Red pixels in the result prove two things at once:
///
/// 1. the rasterizer implements the stroking-colour rule (the capability);
/// 2. an emitter that sets `Tr 2` **without** matching the stroking colour to
///    the fill gets outlines in whatever colour was already in force —
///    black by default (Table 52's initial value), which is why coloured text
///    would acquire black outlines. That is the bug decision 019 §3.6 names,
///    and this test is the evidence it is not hypothetical.
#[test]
fn render_mode_2_stroke_uses_the_stroking_colour_not_the_fill() {
    let p = raster("BT /F1 48 Tf 2 Tr 2 w 0 g 1 0 0 RG 40 60 Td (H) Tj ET\n");
    let reddish = ink_mask(&p)
        .into_iter()
        .filter(|&(x, y)| {
            let [r, g, b, _] = px(&p, x, y);
            r > 120 && g < 120 && b < 120
        })
        .count();
    assert!(
        reddish > 20,
        "a black-filled glyph stroked with `1 0 0 RG` produced {reddish} red \
         pixels — the rasterizer is not applying the STROKING colour to the \
         stroke (§9.3.6). Either the capability is missing, or the stroke is \
         silently taking the fill colour."
    );
}

/// **A `Tm` shear must lean the glyph, not slide it.** The distinguishing
/// property of a shear is that its horizontal displacement is proportional to
/// `y`: zero at the baseline, largest at the cap height. A bug that applied
/// the `c` operand as a translation — or dropped it and moved the origin
/// instead — would move every row equally, and the second assertion here is
/// what separates the two.
///
/// `tan 12° ≈ 0.2126` is the oblique angle decision 019 §3.6 chose.
#[test]
fn a_sheared_tm_obliques_the_glyph() {
    // Same origin in both, expressed as a Tm so the ONLY difference between
    // the two streams is the `c` operand. (Using `Td` for the control would
    // change two things at once.)
    let upright = raster("BT /F1 48 Tf 1 0 0 1 40 60 Tm (H) Tj ET\n");
    let sheared = raster("BT /F1 48 Tf 1 0 0.2126 1 40 60 Tm (H) Tj ET\n");

    let (u_top, u_bot) = ink_rows(&upright).expect("upright glyph has ink");
    let (s_top, s_bot) = ink_rows(&sheared).expect("sheared glyph has ink");

    // The glyph occupies the same vertical band: a horizontal shear moves no
    // ink vertically. (A y-shear would have been in the `b` operand.)
    assert!(
        u_top.abs_diff(s_top) <= 1 && u_bot.abs_diff(s_bot) <= 1,
        "a horizontal shear must not move ink vertically: upright rows \
         {u_top}..={u_bot}, sheared rows {s_top}..={s_bot}"
    );

    // Raster y grows DOWNWARD while PDF y grows upward, so the glyph's TOP
    // (cap height, large PDF y) is the SMALL raster row.
    let top_row = u_top + 2;
    let baseline_row = u_bot - 2;

    let u_top_c = row_ink_centre(&upright, top_row).expect("upright top row has ink");
    let s_top_c = row_ink_centre(&sheared, top_row).expect("sheared top row has ink");
    let u_base_c = row_ink_centre(&upright, baseline_row).expect("upright baseline row has ink");
    let s_base_c = row_ink_centre(&sheared, baseline_row).expect("sheared baseline row has ink");

    let top_shift = s_top_c - u_top_c;
    let base_shift = s_base_c - u_base_c;

    // Cap height of Helvetica is ~0.717 em ⇒ ~34 pt at 48 pt, so the top of
    // the glyph should move right by ~34 × 0.2126 ≈ 7 pt. The assertion is a
    // loose floor rather than that figure, because the exact cap height
    // belongs to the bundled fallback face, not to the capability under test.
    assert!(
        top_shift > 3.0,
        "the top of a sheared glyph moved right by only {top_shift:.2} px — \
         the rasterizer is NOT applying the `c` operand of the text matrix \
         (§9.4.2 Table 108). Synthetic italic cannot ship: it would be \
         invisible in preview and oblique in the saved file (R85)."
    );

    // The shear is anchored at the baseline: `x' = x + c·y`, and y = 0 there.
    assert!(
        base_shift.abs() < top_shift / 2.0,
        "near the baseline the sheared glyph moved by {base_shift:.2} px \
         while its top moved by {top_shift:.2} px — a shear must displace \
         proportionally to y, so this looks like a TRANSLATION, not a shear"
    );
}

/// **`Ts` must move the baseline.** The rise is the other half of what slice
/// 19.2 emits (free-form `Ts`), and the same R85 argument applies: a rise the
/// rasterizer ignored would be invisible in preview and present in the file.
///
/// `Ts` is in unscaled text-space units and enters `Trm` as a translation
/// (§9.3.7), so at scale 1.0 with an identity CTM a `12 Ts` moves the glyph
/// up by 12 device pixels — up the *page*, which is toward row 0 in the
/// raster.
#[test]
fn rise_shifts_the_glyph_up_the_page() {
    let flat = raster(PLAIN);
    let raised = raster("BT /F1 48 Tf 12 Ts 40 60 Td (H) Tj ET\n");

    let (f_top, _) = ink_rows(&flat).expect("flat glyph has ink");
    let (r_top, _) = ink_rows(&raised).expect("raised glyph has ink");

    assert!(
        r_top < f_top,
        "a `12 Ts` rise left the glyph's top row at {r_top} versus {f_top} \
         unraised — the rasterizer is not honouring text rise (§9.3.7)"
    );
    let moved = f_top - r_top;
    assert!(
        (11..=13).contains(&moved),
        "a `12 Ts` at scale 1.0 must raise the glyph by ~12 device pixels; \
         it moved {moved}"
    );
}

/// **The shear × rise interaction, measured.** Decision 019 §3.6 names this
/// as the interaction that "ships as a 'why is my superscript slightly to the
/// right' bug": a shear offsets `x` by `y · tan θ`, which is zero at the
/// baseline and **non-zero for a raised run**, so a superscripted oblique run
/// is displaced horizontally by `Trise · tan θ`.
///
/// This case proves the displacement is real in pdfcer's own rasterizer — the
/// premise the core-side acceptance test rests on. With `Trise = 20` and
/// `tan θ = 0.2126`, the whole glyph must move right by ≈ 4.25 pt relative to
/// the same sheared glyph at rise 0.
#[test]
fn a_rise_under_a_shear_displaces_the_run_horizontally() {
    let sheared_flat = raster("BT /F1 48 Tf 1 0 0.2126 1 40 60 Tm (H) Tj ET\n");
    let sheared_raised = raster("BT /F1 48 Tf 1 0 0.2126 1 40 60 Tm 20 Ts (H) Tj ET\n");

    let (flat_top, flat_bot) = ink_rows(&sheared_flat).expect("flat sheared glyph has ink");
    let mid = (flat_top + flat_bot) / 2;

    // Compare the SAME text-space height in both: the raised glyph's ink at
    // raster row `mid - 20` is the same part of the letterform as the flat
    // glyph's ink at row `mid`, because the rise moved it up by 20 px.
    let flat_c = row_ink_centre(&sheared_flat, mid).expect("flat row has ink");
    let raised_c = row_ink_centre(&sheared_raised, mid - 20).expect("raised row has ink");

    let shift = raised_c - flat_c;
    assert!(
        (2.0..7.0).contains(&shift),
        "a 20-unit rise under a tan-12° shear must displace the run right by \
         ≈ 20 × 0.2126 ≈ 4.25 px; measured {shift:.2} px. This is decision \
         019 §3.6's named `Ts` × synthetic-italic interaction — if the \
         measurement is off, the emitter's compensation assumptions are \
         wrong."
    );
}
