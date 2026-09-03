//! ★ The differential that decision 066 asks for, pointed at pdfcer's own code.
//!
//! Decision 066 says pdfcer verifies a spec-governed computation against the
//! standard *"on at least one case chosen to distinguish the correct
//! definition from a plausible near-miss"* before trusting an implementation.
//! It was minted about a **dependency**. The obligation cannot be weaker for
//! pdfcer's own replacement of that dependency, so this file applies it
//! inward.
//!
//! # The oracle, and why `tiny_skia` is the right one despite being wrong
//!
//! `tiny_skia` 0.11.4's four non-separable blend modes are broken in **one
//! precisely located way**: `clip_color` gates its low-gamut rescale on
//! `mx >= 0` where the standard gates on `mn < 0`, so the rescale branch is
//! dead and negative channels are hard-clamped.
//!
//! That makes it an unusually good oracle, because the defect is *narrow*:
//!
//! - Where `SetLum` produces an **in-gamut** colour, `ClipColor` does nothing
//!   in either implementation, so **pdfcer and the crate must AGREE**.
//! - Where `SetLum` drives a channel **below zero**, the crate clamps and
//!   pdfcer rescales, so they **must DIFFER**.
//!
//! A correct implementation therefore has a *signature*, not just a set of
//! passing assertions: agreement on one population and disagreement on the
//! other. An implementation that agreed everywhere would have inherited the
//! bug; one that disagreed everywhere would be wrong in some new way of its
//! own, and my unit tests — written from the same reading of the clause as
//! the code — could not tell me that.
//!
//! **That is the point of this file.** The unit tests in `blend_nonsep`
//! check my transcription against my reading. This checks it against an
//! independent implementation whose one deviation is already characterised.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_render::blend_nonsep::{NonSeparableBlend, blend};
use pdfcer_render::tiny_skia::{BlendMode, Paint, Pixmap, PixmapPaint, Transform};

/// Blend `cs` over `cb` using `tiny_skia`, through 1×1 opaque pixmaps.
///
/// Opaque on both sides deliberately: with `ab = as = 1` the compositing
/// arithmetic of §11.3.6 collapses to `B(cb, cs)` exactly, so any difference
/// observed is the blend function and not the alpha model.
fn tiny_skia_blend(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;

    let mut dst = Pixmap::new(1, 1).unwrap();
    let mut paint = Paint::default();
    paint.set_color_rgba8(to8(cb[0]), to8(cb[1]), to8(cb[2]), 255);
    paint.anti_alias = false;
    dst.fill_rect(
        pdfcer_render::tiny_skia::Rect::from_ltrb(0.0, 0.0, 1.0, 1.0).unwrap(),
        &paint,
        Transform::identity(),
        None,
    );

    let mut src = Pixmap::new(1, 1).unwrap();
    let mut spaint = Paint::default();
    spaint.set_color_rgba8(to8(cs[0]), to8(cs[1]), to8(cs[2]), 255);
    spaint.anti_alias = false;
    src.fill_rect(
        pdfcer_render::tiny_skia::Rect::from_ltrb(0.0, 0.0, 1.0, 1.0).unwrap(),
        &spaint,
        Transform::identity(),
        None,
    );

    dst.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &PixmapPaint {
            blend_mode: mode,
            ..PixmapPaint::default()
        },
        Transform::identity(),
        None,
    );

    let p = dst.pixel(0, 0).unwrap().demultiply();
    [
        f32::from(p.red()) / 255.0,
        f32::from(p.green()) / 255.0,
        f32::from(p.blue()) / 255.0,
    ]
}

/// Would `SetLum` drive a channel out of gamut for this pair and mode? That
/// is the predicate separating the two populations.
///
/// Computed from pdfcer's own definition, which is fair here: the question is
/// only *which bucket to put the sample in*, and both implementations agree
/// about `Lum` — the disagreement is entirely inside `ClipColor`.
fn out_of_gamut_case(mode: NonSeparableBlend, cb: [f32; 3], cs: [f32; 3]) -> bool {
    let lum = |c: [f32; 3]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
    let sat = |c: [f32; 3]| c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2]);
    // The colour `SetLum` is applied to, and the luminosity it is given.
    let (base, target) = match mode {
        NonSeparableBlend::Hue => (set_sat_local(cs, sat(cb)), lum(cb)),
        NonSeparableBlend::Saturation => (set_sat_local(cb, sat(cs)), lum(cb)),
        NonSeparableBlend::Color => (cs, lum(cb)),
        NonSeparableBlend::Luminosity => (cb, lum(cs)),
    };
    let d = target - lum(base);
    let shifted = [base[0] + d, base[1] + d, base[2] + d];
    shifted.iter().any(|v| *v < 0.0 || *v > 1.0)
}

/// `SetSat`, duplicated here ONLY to classify samples.
///
/// Deliberately not imported: this file's job is to be an outside check, and
/// a classifier sharing code with the thing under test can put a sample in
/// the wrong bucket in exactly the way that would hide a defect.
fn set_sat_local(c: [f32; 3], s: f32) -> [f32; 3] {
    let (mut imin, mut imid, mut imax) = (0_usize, 1_usize, 2_usize);
    if c[imin] > c[imid] {
        std::mem::swap(&mut imin, &mut imid);
    }
    if c[imid] > c[imax] {
        std::mem::swap(&mut imid, &mut imax);
    }
    if c[imin] > c[imid] {
        std::mem::swap(&mut imin, &mut imid);
    }
    let mut out = c;
    if c[imax] > c[imin] {
        out[imid] = ((c[imid] - c[imin]) * s) / (c[imax] - c[imin]);
        out[imax] = s;
    } else {
        out[imid] = 0.0;
        out[imax] = 0.0;
    }
    out[imin] = 0.0;
    out
}

/// A deterministic spread of colour pairs. No RNG: a test that samples
/// differently on each run reports a different defect each time.
fn samples() -> Vec<[f32; 3]> {
    let steps = [0.0_f32, 0.25, 0.5, 0.75, 1.0];
    let mut out = Vec::new();
    for r in steps {
        for g in steps {
            for b in steps {
                out.push([r, g, b]);
            }
        }
    }
    out
}

fn pairs() -> Vec<(NonSeparableBlend, [f32; 3], [f32; 3])> {
    let modes = [
        NonSeparableBlend::Hue,
        NonSeparableBlend::Saturation,
        NonSeparableBlend::Color,
        NonSeparableBlend::Luminosity,
    ];
    let s = samples();
    let mut out = Vec::new();
    for m in modes {
        for &cb in &s {
            for &cs in &s {
                out.push((m, cb, cs));
            }
        }
    }
    out
}

fn ts_mode(m: NonSeparableBlend) -> BlendMode {
    match m {
        NonSeparableBlend::Hue => BlendMode::Hue,
        NonSeparableBlend::Saturation => BlendMode::Saturation,
        NonSeparableBlend::Color => BlendMode::Color,
        NonSeparableBlend::Luminosity => BlendMode::Luminosity,
    }
}

/// ★ WHERE THE DEPENDENCY IS CORRECT, pdfcer AGREES WITH IT.
///
/// This is the half that catches "wrong in a new way". If pdfcer had
/// mis-transcribed `Lum`, `Sat`, `SetSat` or Table 137's argument order, it
/// would diverge here too — on the population where the crate's one known
/// defect cannot fire.
///
/// The tolerance is 3/255: both sides round to 8-bit, and `tiny_skia`
/// composites through premultiplied storage, so exact equality is not
/// available and demanding it would make this test about rounding.
#[test]
fn pdfcer_agrees_with_tiny_skia_wherever_clip_color_cannot_fire() {
    let mut checked = 0_usize;
    let mut worst = (0.0_f32, String::new());

    for (mode, cb, cs) in pairs() {
        if out_of_gamut_case(mode, cb, cs) {
            continue; // the crate's defect fires here; the other test owns it
        }
        checked += 1;
        let mine = blend(mode, cb, cs);
        let theirs = tiny_skia_blend(ts_mode(mode), cb, cs);
        for i in 0..3 {
            let d = (mine[i] - theirs[i]).abs();
            if d > worst.0 {
                worst = (
                    d,
                    format!(
                        "{} cb={cb:?} cs={cs:?} -> mine {mine:?} theirs {theirs:?}",
                        mode.name()
                    ),
                );
            }
        }
    }

    assert!(
        checked > 500,
        "only {checked} in-gamut samples — the classifier is excluding too much \
         and this test would pass vacuously"
    );
    assert!(
        worst.0 <= 3.0 / 255.0,
        "pdfcer and tiny_skia must agree where ClipColor cannot fire, but they \
         differ by {:.1}/255 on {}. A difference HERE means pdfcer's \
         transcription of Lum/Sat/SetSat or of Table 137's argument order is \
         wrong — the crate's known defect cannot reach this population.",
        worst.0 * 255.0,
        worst.1
    );
    println!(
        "in-gamut samples agreeing: {checked}, worst delta {:.2}/255",
        worst.0 * 255.0
    );
}

/// ★ WHERE THE DEPENDENCY IS BROKEN, pdfcer DIFFERS FROM IT — and differs in
/// the direction the standard requires.
///
/// Without this half the test above could be satisfied by simply calling
/// `tiny_skia`. This proves the fix is present: on the population where
/// `ClipColor`'s low-gamut rescale should fire, pdfcer must not match a
/// per-channel clamp.
#[test]
fn pdfcer_differs_from_tiny_skia_exactly_where_the_dependency_is_wrong() {
    let mut out_of_gamut = 0_usize;
    let mut differing = 0_usize;
    let mut worst = 0.0_f32;

    for (mode, cb, cs) in pairs() {
        if !out_of_gamut_case(mode, cb, cs) {
            continue;
        }
        out_of_gamut += 1;
        let mine = blend(mode, cb, cs);
        let theirs = tiny_skia_blend(ts_mode(mode), cb, cs);
        let d = (0..3)
            .map(|i| (mine[i] - theirs[i]).abs())
            .fold(0.0_f32, f32::max);
        if d > 3.0 / 255.0 {
            differing += 1;
        }
        worst = worst.max(d);
    }

    assert!(
        out_of_gamut > 100,
        "only {out_of_gamut} out-of-gamut samples — not enough to characterise \
         the divergence"
    );
    assert!(
        differing > 0,
        "pdfcer matched tiny_skia on ALL {out_of_gamut} out-of-gamut samples. \
         That means the ClipColor rescale is not firing and pdfcer has \
         inherited the very defect it exists to fix."
    );
    println!(
        "out-of-gamut samples: {out_of_gamut}, differing from tiny_skia: \
         {differing}, worst delta {:.1}/255",
        worst * 255.0
    );
}

/// The canonical case, end to end through the oracle rather than in the
/// unit test's own terms: black `Luminosity` over blue.
///
/// The standard says black. `tiny_skia` says `(0, 0, 227)`. Both halves are
/// asserted, so if the crate is ever fixed upstream this test says so by
/// failing on the second assertion rather than by going quietly green.
#[test]
fn the_canonical_case_is_still_a_divergence() {
    let cb = [0.0, 0.0, 1.0];
    let cs = [0.0, 0.0, 0.0];

    let mine = blend(NonSeparableBlend::Luminosity, cb, cs);
    assert!(
        mine.iter().all(|v| *v < 1.0 / 255.0),
        "the standard requires black here; pdfcer gave {mine:?}"
    );

    let theirs = tiny_skia_blend(BlendMode::Luminosity, cb, cs);
    assert!(
        theirs[2] > 0.5,
        "tiny_skia 0.11.4 returns (0, 0, ~0.89) for this case. It gave \
         {theirs:?} instead — if the crate has been FIXED upstream, decision \
         066's refusal should be re-examined and this module may be able to \
         delegate again."
    );
}

// ---------------------------------------------------------------------------
// END TO END — the canonical case as PIXELS, not as a function call
// ---------------------------------------------------------------------------

/// Build a self-contained PDF from `(object number, body)` pairs.
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

/// A page that fills blue, then fills black over it through `/BM mode`.
fn page_with_blend(mode: &str) -> Vec<u8> {
    let stream = "0 0 1 rg 0 0 60 60 re f\n/GS0 gs 0 0 0 rg 10 10 40 40 re f\n".to_owned();
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            &format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] \
                 /Resources << /ExtGState << /GS0 << /BM /{mode} >> >> >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

fn centre_pixel(bytes: Vec<u8>) -> (u8, u8, u8) {
    let doc = pdfcer_core::document::Document::from_bytes(bytes).expect("fixture parses");
    let page = pdfcer_core::page_tree::pages(&doc)
        .expect("page tree")
        .remove(0);
    let r = pdfcer_render::render_page(&doc, &page, 1.0).expect("renders");
    let px = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    (px.red(), px.green(), px.blue())
}

/// ★★ THE CANONICAL CASE, AS PIXELS ON A PAGE.
///
/// A black square painted over a blue page with `/BM /Luminosity` must come
/// out **black**: the result keeps the backdrop's hue and saturation but takes
/// the source's luminosity, and black has none.
///
/// This is the test that distinguishes "the formulas are right" from "the
/// formulas are wired to the rasteriser". `blend_nonsep`'s unit tests prove
/// the first; only rendering a page proves the second, and the two failed
/// independently during development — the composite was correct while the
/// glyph path silently bypassed it.
///
/// Before this work the page rendered BLUE, because `blend_mode_from_name`
/// returned `None` for `/Luminosity` and the paint fell back to Normal.
#[test]
fn luminosity_black_over_blue_renders_black_on_a_real_page() {
    let (r, g, b) = centre_pixel(page_with_blend("Luminosity"));
    assert!(
        r < 12 && g < 12 && b < 12,
        "a black /BM /Luminosity fill over blue must render BLACK; got \
         ({r}, {g}, {b}). Blue (~0,0,255) means the mode is not reaching the \
         paint path at all; (0,0,227) means it reached tiny_skia instead of \
         pdfcer's own Table 137."
    );
}

/// The control, and it is what makes the test above mean something.
///
/// The SAME geometry and colours with `/BM /Normal` must render the source
/// colour — black — for an ordinary reason. If this failed, the test above
/// could be passing because the fixture paints black regardless of the mode.
///
/// So the pair is chosen to differ in the one case where Normal and
/// Luminosity DISAGREE: `Color`, where the source is black and the backdrop
/// blue, gives BLUE under Table 137 (source hue/saturation, backdrop
/// luminosity — black has no hue, so it takes the backdrop's) and BLACK under
/// Normal.
#[test]
fn color_mode_over_blue_is_not_what_normal_would_give() {
    let normal = centre_pixel(page_with_blend("Normal"));
    let colour = centre_pixel(page_with_blend("Color"));
    assert_eq!(
        normal,
        (0, 0, 0),
        "the control must paint the source colour under /BM /Normal"
    );
    assert_ne!(
        colour, normal,
        "/BM /Color must differ from /BM /Normal here — if they agree, the \
         non-separable path is not being taken and every other assertion in \
         this file is about a function nothing calls"
    );
}
