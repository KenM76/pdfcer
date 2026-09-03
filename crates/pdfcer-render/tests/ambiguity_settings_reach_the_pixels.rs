//! # The R169 rendering settings are real knobs, not stored preferences
//!
//! Standing rule **R169** (operator, 2026-08-08): *"where standards are
//! ambiguous those should become settings that the user can choose
//! direction one, with the initial installed default as the best guess of
//! what is usually followed."* Four of those settings are rendering
//! decisions, and this file is the evidence that each one travels the
//! whole distance from `RenderOptions` to a pixel.
//!
//! ## What this file is defending against
//!
//! **R83 — no affordance without capability.** `tools/check-settings-consumed.py`
//! catches the cheap failure (nobody wired the setting at all) by grepping
//! for a `settings.<field>` read. It deliberately does **not** judge
//! whether the value reaches the pixels, because a grep-based gate that
//! tried to would produce false confidence. That judgement is a test's
//! job, and this is that test — the same relationship
//! `crates/pdfcer-render/tests/cmyk_intent.rs` has to `cmyk_intent`.
//!
//! The distance each value has to travel is real, and each leg is a place
//! it could be dropped silently:
//!
//! | Setting | Path |
//! |---|---|
//! | `SM-A1` mask resampling | `RenderOptions` → `RenderPolicy` → `Interpreter` → `image::decode_sampled` → `AlphaPlane::at` |
//! | `IM-A1` minification | `RenderOptions` → `RenderPolicy` → `Interpreter::paint_image` → `tiny_skia::FilterQuality` |
//! | `DCT-A1` CMYK-JPEG polarity | `RenderOptions` → `RenderPolicy` → `image::decode` → `image_codec::decode_image_view_with` → `dct::decode` |
//!
//! `AS-A1` (missing `/AS`) is proved in `pdfcer-core`'s own
//! `annot::tests`, at the point where the appearance is *selected* — that
//! is where the decision is actually made, and asserting it there pins the
//! selection rather than a downstream consequence of it.
//!
//! ## The assertions are "differs" plus "the default did not move"
//!
//! Two shapes, deliberately:
//!
//! 1. **The non-default value changes the pixels.** That is the R83
//!    claim, and it is the one that would break if a threading seam
//!    dropped the value.
//! 2. **The default renders byte-identically to a render that never
//!    mentions the setting.** That is R169's non-negotiable — adding the
//!    knob must change no observable behaviour — and it is the assertion
//!    that would catch a default quietly flipped by a later session.
//!
//! Exact filtered values are pinned in `mask.rs`'s unit tests, where the
//! arithmetic lives. Pinning `tiny_skia`'s bilinear output byte-for-byte
//! here would be pinning a dependency's internals, which is a test that
//! fails on an unrelated upgrade and teaches nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::settings::{CmykJpegPolarity, MaskResample, MinifyFilter};
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Build an offset-consistent classic PDF from `(number, body)` pairs.
///
/// A local copy of `cmyk_intent.rs`'s helper rather than a shared one:
/// integration tests are separate binaries with no shared module, and a
/// `tests/common/` module would be compiled into every one of them.
fn build(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
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

/// An 8-bit greyscale image-stream object body with a **raw byte**
/// payload.
///
/// Built as `Vec<u8>` rather than `String` deliberately: a sample value of
/// `0xFF` is not valid UTF-8 on its own, so a `String`-based builder
/// silently re-encodes it as two bytes and the stream's `/Length` stops
/// matching its extent. (Found the hard way — the parser's
/// `StreamExtentMismatch` is exactly this mistake, and it is worth one
/// comment to save the next person the same twenty minutes.)
fn image_object(dict_extra: &str, width: u32, height: u32, samples: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceGray /BitsPerComponent 8 {dict_extra} /Length {} >>\nstream\n",
        samples.len()
    )
    .into_bytes();
    out.extend_from_slice(samples);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A one-page document `w x h` points, whose whole area is one image.
fn page_with_image(w: u32, h: u32, objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let content = format!("q {w} 0 0 {h} 0 0 cm /Im Do Q\n");
    let mut all = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {w} {h}] \
                 /Resources << /XObject << /Im 5 0 R >> >> >>"
            )
            .into_bytes(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            )
            .into_bytes(),
        ),
    ];
    all.extend(objects);
    build(&all)
}

fn render(bytes: Vec<u8>, options: &RenderOptions) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    render_page_with(&doc, &page, 1.0, options).expect("render succeeds")
}

/// Every pixel of a rendered page, as raw premultiplied bytes.
fn pixels(page: &RenderedPage) -> Vec<u8> {
    page.pixmap.data().to_vec()
}

// ---------------------------------------------------------------------------
// SM-A1 — the mask resampling filter
// ---------------------------------------------------------------------------

/// A 4x1 black image behind a **2x1** soft mask reading `[0, 255]`.
///
/// The grids deliberately disagree, which is the only configuration
/// `SM-A1` governs — §8.9.6.3 and Table 145 both say a size-mismatched
/// mask is normal and conformant, and neither says how to resample it.
/// Each of the four output pixels is one base texel, so the rendered row
/// IS the resampled alpha, composited over the page's white paper: alpha
/// 0 shows white, alpha 255 shows the image's black.
fn mismatched_mask_page() -> Vec<u8> {
    page_with_image(
        4,
        1,
        vec![
            (
                5,
                image_object("/SMask 6 0 R", 4, 1, &[0x00, 0x00, 0x00, 0x00]),
            ),
            (6, image_object("", 2, 1, &[0x00, 0xFF])),
        ],
    )
}

#[test]
fn the_mask_resampling_filter_reaches_the_pixels() {
    let nearest = render(
        mismatched_mask_page(),
        &RenderOptions::default().with_mask_resample(MaskResample::Nearest),
    );
    let bilinear = render(
        mismatched_mask_page(),
        &RenderOptions::default().with_mask_resample(MaskResample::Bilinear),
    );
    assert_ne!(
        pixels(&nearest),
        pixels(&bilinear),
        "SM-A1 never left `RenderOptions` — a bilinear resample of a 0/255 \
         mask across four texels cannot produce the same row as a \
         nearest-neighbour one"
    );

    // And the direction is the documented one: nearest-neighbour gives a
    // hard step (two fully transparent texels then two fully opaque),
    // bilinear gives intermediate alphas in between. Asserted as
    // "intermediate exists" rather than as exact bytes — the arithmetic is
    // pinned in `mask.rs`'s unit tests, this only proves it arrived.
    let row = pixels(&bilinear);
    assert!(
        row.chunks_exact(4).any(|px| px[0] > 0x10 && px[0] < 0xF0),
        "bilinear must produce a blended pixel somewhere in {row:?}"
    );
}

#[test]
fn the_default_mask_filter_renders_exactly_as_before_the_setting_existed() {
    // R169's non-negotiable, in its most direct form: a render that names
    // the default must be byte-identical to one that names nothing.
    let implicit = render(mismatched_mask_page(), &RenderOptions::default());
    let explicit = render(
        mismatched_mask_page(),
        &RenderOptions::default().with_mask_resample(MaskResample::Nearest),
    );
    assert_eq!(pixels(&implicit), pixels(&explicit), "the default moved");
}

// ---------------------------------------------------------------------------
// IM-A1 — the minification filter
// ---------------------------------------------------------------------------

/// An 8x1 image of alternating black and white texels squeezed into 2x1
/// points, i.e. four source texels per output pixel.
///
/// §8.9.5.3 defines interpolation only for MAGNIFICATION and never
/// mentions minification, so `/Interpolate` (absent here, therefore false)
/// does not in fact legislate this direction — which is exactly why it is
/// a setting. Point-sampling picks one of the eight texels per pixel and
/// therefore lands on a pure 0 or a pure 255; smoothing blends.
fn minified_image_page() -> Vec<u8> {
    page_with_image(
        2,
        1,
        vec![(
            5,
            image_object("", 8, 1, &[0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF]),
        )],
    )
}

#[test]
fn the_minification_filter_reaches_the_pixels() {
    let point = render(
        minified_image_page(),
        &RenderOptions::default().with_image_minify(MinifyFilter::PointSample),
    );
    let smooth = render(
        minified_image_page(),
        &RenderOptions::default().with_image_minify(MinifyFilter::Smooth),
    );
    assert_ne!(
        pixels(&point),
        pixels(&smooth),
        "IM-A1 never left `RenderOptions` — smoothing a 0/255 stripe \
         pattern down 4:1 cannot reproduce the point-sampled row"
    );
}

#[test]
fn minification_smoothing_does_not_touch_a_magnified_image() {
    // Blast-radius containment. `IM-A1` is about the direction the clause
    // never legislated; the direction it DID legislate (`/Interpolate`
    // governs magnification) must be untouched, or the setting is wider
    // than its documentation claims and a document's own smoothing
    // request has been overridden.
    let magnified = || page_with_image(8, 1, vec![(5, image_object("", 2, 1, &[0x00, 0xFF]))]);
    let point = render(
        magnified(),
        &RenderOptions::default().with_image_minify(MinifyFilter::PointSample),
    );
    let smooth = render(
        magnified(),
        &RenderOptions::default().with_image_minify(MinifyFilter::Smooth),
    );
    assert_eq!(
        pixels(&point),
        pixels(&smooth),
        "the minification setting changed a MAGNIFIED image"
    );
}

#[test]
fn the_render_default_tracks_the_settings_default() {
    // ★ THIS TEST WAS `the_default_minification_filter_renders_exactly_as_
    // _before` and asserted the default was `PointSample` by name. That
    // pinned the wrong thing: the durable claim is not WHICH variant is
    // default, it is that `RenderOptions::default()` and
    // `MinifyFilter::default()` cannot DRIFT APART. Two defaults for one
    // value is how a shell ends up rendering differently from the engine it
    // is a shell for, and it is silent — the pixels simply differ from the
    // ones the settings file describes.
    //
    // The operator flipped `MinifyFilter::default()` to `Smooth` on
    // 2026-08-25 (see its own docs for the evidence). Written this way, that
    // flip needed no edit here, which is the point: a test that has to be
    // rewritten every time an evidence-based default moves is a test that
    // will eventually be rewritten WITHOUT the evidence.
    let implicit = render(minified_image_page(), &RenderOptions::default());
    let explicit = render(
        minified_image_page(),
        &RenderOptions::default().with_image_minify(MinifyFilter::default()),
    );
    assert_eq!(
        pixels(&implicit),
        pixels(&explicit),
        "RenderOptions::default() no longer carries MinifyFilter::default()"
    );

    // …and the pairing that stops the above being vacuous. If the two
    // variants rendered identically on this fixture, the assertion would
    // hold for any wiring at all, including none.
    let other = match MinifyFilter::default() {
        MinifyFilter::Smooth => MinifyFilter::PointSample,
        _ => MinifyFilter::Smooth,
    };
    let differing = render(
        minified_image_page(),
        &RenderOptions::default().with_image_minify(other),
    );
    assert_ne!(
        pixels(&implicit),
        pixels(&differing),
        "the two filters render this fixture identically, so the assertion above could not have failed however the default were wired"
    );
}

// ---------------------------------------------------------------------------
// DCT-A1 — CMYK-JPEG polarity
// ---------------------------------------------------------------------------
//
// Reuses the decision-006 regression fixtures, which were built for
// exactly this question: `v0.pdf` carries an Adobe APP14 marker with
// transform byte 0 and no `/Decode` — the one shape nothing in the file
// can disambiguate — while `vn.pdf` is the same image with the marker
// rewritten to a COM, i.e. no marker at all.

fn cmyk_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/cmyk-variants")
        .join(name);
    std::fs::read(&path).expect("fixture file")
}

/// Perceptually-weighted luminance of one rendered pixel.
fn luminance_at(page: &RenderedPage, x: u32, y: u32) -> f32 {
    let px = page.pixmap.pixel(x, y).expect("pixel in bounds");
    0.299 * f32::from(px.red()) + 0.587 * f32::from(px.green()) + 0.114 * f32::from(px.blue())
}

#[test]
fn the_cmyk_jpeg_polarity_setting_reaches_the_pixels() {
    let never = render(
        cmyk_fixture("v0.pdf"),
        &RenderOptions::default().with_cmyk_jpeg_polarity(CmykJpegPolarity::NeverInvert),
    );
    let inverted = render(
        cmyk_fixture("v0.pdf"),
        &RenderOptions::default().with_cmyk_jpeg_polarity(CmykJpegPolarity::InvertOnApp14),
    );
    assert_ne!(
        pixels(&never),
        pixels(&inverted),
        "DCT-A1 never left `RenderOptions`"
    );
    // The failure mode this setting exists to fix, and to cause, is a
    // photographic negative — so the luminances must move in opposite
    // directions, not merely differ by a rounding step.
    let a = luminance_at(&never, 10, 10);
    let b = luminance_at(&inverted, 10, 10);
    assert!(
        (a - b).abs() > 20.0,
        "expected a negative-scale difference, got {a} vs {b}"
    );
}

#[test]
fn invert_on_app14_does_nothing_without_an_app14_marker() {
    // The setting is named for its trigger and must honour it. `vn.pdf`
    // is `v0.pdf` with the Adobe marker rewritten to a COM: same samples,
    // same effective transform 0, no marker. Inverting it anyway would
    // make the option a blunt "invert every CMYK JPEG", which is a
    // different and much worse thing than what it says on the tin.
    let never = render(
        cmyk_fixture("vn.pdf"),
        &RenderOptions::default().with_cmyk_jpeg_polarity(CmykJpegPolarity::NeverInvert),
    );
    let inverted = render(
        cmyk_fixture("vn.pdf"),
        &RenderOptions::default().with_cmyk_jpeg_polarity(CmykJpegPolarity::InvertOnApp14),
    );
    assert_eq!(
        pixels(&never),
        pixels(&inverted),
        "a markerless CMYK JPEG must be untouched by `invert_on_app14`"
    );
}

#[test]
fn the_default_polarity_renders_exactly_as_before() {
    // R29 is the default and stays the default: `never_invert`.
    let implicit = render(cmyk_fixture("v0.pdf"), &RenderOptions::default());
    let explicit = render(
        cmyk_fixture("v0.pdf"),
        &RenderOptions::default().with_cmyk_jpeg_polarity(CmykJpegPolarity::NeverInvert),
    );
    assert_eq!(pixels(&implicit), pixels(&explicit), "R29 was moved");
}

/// The settings file's own prose about the CMYK ceiling agrees with the
/// renderer's actual constants (`Pass 132.0`).
///
/// # Why this test exists at this crate boundary
///
/// `pdfcer-core` writes the settings file and CANNOT see `pdfcer-render` —
/// the dependency runs the other way — so the paragraph describing the
/// ceiling quotes numbers (20 bytes per pixel; roughly 530 % zoom on a
/// whole A4 page at the default) that live in a crate it cannot check
/// against. That is exactly the arrangement in which a comment goes quietly
/// wrong: the constant moves, every test still passes, and the operator
/// reads a stale sentence in a file pdfcer itself generated.
///
/// `pdfcer-render` can see both. So the check lives here, and it is a check
/// on the DOCUMENTATION, which is otherwise the one thing in this project
/// nothing tests.
#[test]
fn the_settings_file_describes_the_ceiling_the_renderer_actually_enforces() {
    let text = pdfcer_core::settings::Settings::default().write_to_string();

    assert!(
        text.contains("max_cmyk_buffer_bytes = default"),
        "an unset ceiling must be written as `default`, not as a number that \
         would then be frozen into the operator's file"
    );

    // The per-pixel cost, quoted in the file's own comment.
    assert_eq!(
        pdfcer_render::CMYK_BYTES_PER_PIXEL,
        20,
        "the settings file tells the operator 20 bytes per pixel"
    );
    assert!(text.contains("20 bytes per pixel"));

    // The zoom the default ceiling reaches on a whole A4 page (595x842 pt).
    // Recomputed from the constant rather than restated, so moving the
    // constant fails HERE with a number, instead of silently making the
    // sentence wrong.
    #[allow(clippy::cast_precision_loss)]
    let max_px = pdfcer_render::max_cmyk_composite_pixels(None) as f64;
    let a4_pt = 595.0 * 842.0;
    let zoom_percent = (max_px / a4_pt).sqrt() * 100.0;
    // ★ A BAND OF FIVE POINTS, AND IT USED TO BE SIXTY. The wide band was
    // written to be robust and is the reason this assertion passed while the
    // sentence it guards was WRONG: every percentage in that paragraph had
    // been computed on a 596 x 791 pt page -- the size of the file the
    // request was bisected on, which was called A4 and is not (A4 is
    // 595 x 842). The true figure is 518 %, the prose said 530 %, and a
    // +-30 band swallowed the difference. A tolerance chosen for comfort
    // rather than derived from the arithmetic hides the class of error it
    // was pointed at.
    assert!(
        (513.0..523.0).contains(&zoom_percent),
        "the file says the default reaches about 518% on A4; it now reaches {zoom_percent:.0}% -- move the prose or move the constant, but not one without the other"
    );
    assert!(text.contains("about 518% zoom"));

    // The 1 GiB figure in the same paragraph, from the same arithmetic. Two
    // numbers rather than one, because the wrong-page error moved BOTH and a
    // check on either alone would have caught it only by luck.
    #[allow(clippy::cast_precision_loss)]
    let gib_percent = ((pdfcer_render::max_cmyk_composite_pixels(Some(1024 * 1024 * 1024)) as f64)
        / a4_pt)
        .sqrt()
        * 100.0;
    assert!(
        (1030.0..1040.0).contains(&gib_percent),
        "the file says 1gib reaches about 1035%; it now reaches {gib_percent:.0}%"
    );
    assert!(text.contains("about 1035%"));
}
