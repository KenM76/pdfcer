//! # Pixel patches instead of approximations (`Pass 700.12`)
//!
//! With `EmfOptions::with_patch_approximations(true)` every region the
//! export recording can only approximate is written as the raster's own
//! pixels, over the smallest box that holds it, and everything else stays
//! vector. The oracle is GDI's own `PlayEnhMetaFile` (Windows only), played
//! onto white and compared per pixel against the white-backed raster.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build, count, page, record_offsets, u32_at};

/// Cyan, then magenta overprinting it (OPM 1), then a small K-only mark
/// beside both, over paper.
pub(crate) const OVERPRINT_RES: &str = "/ExtGState << /OP << /OP true /op true /OPM 1 >> >>";
pub(crate) const OVERPRINT: &str =
    "1 0 0 0 k 10 10 50 40 re f /OP gs 0 1 0 0 k 40 20 40 40 re f 0 0 0 1 k 85 10 8 3 re f";

/// A non-isolated group, painted at 70 % opacity (so it needs a buffer: at
/// full opacity it is drawn straight onto the page, exactly), whose
/// Multiply fill blends with a blue backdrop.
pub(crate) fn non_isolated_group() -> Vec<u8> {
    let form = "/M gs 1 1 0 rg 30 20 40 40 re f\n";
    let content = "0 0.6 1 rg 10 10 50 40 re f /H gs /G Do\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /XObject << /G 5 0 R >> /ExtGState << /H << /ca 0.7 >> >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency /I false >> /Resources << /ExtGState << /M << /BM /Multiply >> >> >> /Length {} >>\nstream\n{form}endstream",
                form.len()
            ),
        ),
    ])
}

/// A page group blending in DeviceCMYK, with a Difference fill.
fn cmyk_page_group() -> Vec<u8> {
    let content = "0 0 1 0 k 10 10 50 40 re f /D gs 0.5 0 0 0.2 k 30 20 40 40 re f\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /ExtGState << /D << /BM /Difference >> >> >> >>"
                .into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Group << /S /Transparency /CS /DeviceCMYK >> >>"
                .into(),
        ),
        (
            4,
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        ),
    ])
}

/// The white-backed raster and the export with the switch set as given.
fn export(bytes: &[u8], patch: bool) -> (Pixmap, EmfExport) {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let reference = render_page_with(
        &doc,
        &p,
        DPI / 72.0,
        &RenderOptions::default().with_backdrop(PageBackdrop::White),
    )
    .expect("render");
    let options = EmfOptions::default()
        .with_raster_dpi(DPI)
        .with_patch_approximations(patch);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    (reference.pixmap, emf)
}

/// The pixels of the `EMR_ALPHABLEND` records, `(cxSrc, cySrc)` each.
pub(crate) fn bitmap_sizes(emf: &[u8]) -> Vec<(u32, u32)> {
    let records = walk_records(emf).unwrap();
    records
        .iter()
        .zip(record_offsets(&records))
        .filter(|((t, _), _)| *t == 0x72)
        .map(|(_, off)| (u32_at(emf, off + 8 + 92), u32_at(emf, off + 8 + 96)))
        .collect()
}

/// GDI's playback against the raster: `(pixels whose colour matches no
/// raster pixel within one pixel of it, by more than `tolerance` on any
/// channel; worst such difference)`. GDI fills a rectangle one device pixel
/// right and down of tiny-skia (unpatched vector edges too), so the
/// comparison forgives edge POSITION by a pixel but not colour.
#[cfg(windows)]
pub(crate) fn playback_against_raster(
    name: &str,
    reference: &Pixmap,
    emf: &[u8],
    tolerance: u8,
) -> (usize, u8) {
    let dir = crate::export_emf::scratch_dir(&format!("patch-{name}"));
    let (w, h) = (reference.width() as usize, reference.height() as usize);
    let raw = crate::export_emf::play_with_gdi(&dir, name, emf, w as u32, h as u32);
    let _ = std::fs::remove_dir_all(&dir);
    let px = reference.pixels();
    let diff = |i: usize, g: &[u8]| {
        let r = px[i];
        [
            r.red().abs_diff(g[2]),
            r.green().abs_diff(g[1]),
            r.blue().abs_diff(g[0]),
        ]
        .into_iter()
        .max()
        .unwrap()
    };
    let mut bad = 0usize;
    let mut worst = 0u8;
    for y in 0..h {
        for x in 0..w {
            let g = &raw[(y * w + x) * 4..(y * w + x) * 4 + 4]; // B G R A
            let mut best = u8::MAX;
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    best = best.min(diff(ny * w + nx, g));
                }
            }
            if best > tolerance {
                bad += 1;
                worst = worst.max(best);
            }
        }
    }
    (bad, worst)
}

/// Largest per-channel difference counted as rounding.
#[cfg(windows)]
pub(crate) const ROUNDING: u8 = 2;

#[test]
fn only_the_overprint_overlap_is_a_patch() {
    let (_, off) = export(&page(OVERPRINT_RES, OVERPRINT), false);
    assert!(off.outcome.tally.overprint_approximated > 0);
    let (_, on) = export(&page(OVERPRINT_RES, OVERPRINT), true);
    let o = &on.outcome;
    assert_eq!(o.tally.overprint_approximated, 0);
    assert_eq!(o.patches.overprint, 1, "{:?}", o.patches);
    assert_eq!(o.patches.rasters, 1);
    assert_eq!(o.print_fallback(), None);
    // The overlap is 20 × 30 pt = 40 × 60 px, grown a pixel each side.
    assert_eq!(bitmap_sizes(&on.emf), vec![(42, 62)]);
    // All three rectangles are still vector paths.
    let records = walk_records(&on.emf).unwrap();
    assert_eq!(count(&records, 0x3E), 3, "FILLPATH per rectangle");
    assert_eq!(
        count(&records, 0x1D),
        1,
        "the overlap is excluded from them"
    );
}

#[cfg(windows)]
#[test]
fn an_overprint_plays_back_like_the_raster() {
    let (reference, emf) = export(&page(OVERPRINT_RES, OVERPRINT), true);
    let (bad, worst) = playback_against_raster("overprint", &reference, &emf.emf, ROUNDING);
    assert_eq!(bad, 0, "worst {worst}");
    // Without the switch the overlap plays back wrong, so the metric bites.
    let (reference, emf) = export(&page(OVERPRINT_RES, OVERPRINT), false);
    let (bad, _) = playback_against_raster("overprint-off", &reference, &emf.emf, ROUNDING);
    assert!(bad > 1000, "{bad}");
}

#[test]
fn a_non_isolated_group_is_patched_not_isolated() {
    let (_, off) = export(&non_isolated_group(), false);
    assert!(
        off.outcome.tally.non_isolated_groups_isolated > 0,
        "{:?}",
        off.outcome.tally
    );
    let (_, on) = export(&non_isolated_group(), true);
    let o = &on.outcome;
    assert_eq!(o.tally.non_isolated_groups_isolated, 0);
    assert_eq!(o.blend_modes_dropped, 0);
    assert_eq!(o.patches.non_isolated_groups, 1, "{:?}", o.patches);
    assert_eq!(o.print_fallback(), None);
    // The group's painted box (40 x 40 pt = 80 x 80 px, grown a pixel each
    // side), not the page.
    assert_eq!(bitmap_sizes(&on.emf), vec![(82, 82)]);
}

#[cfg(windows)]
#[test]
fn a_non_isolated_group_plays_back_like_the_raster() {
    let (reference, emf) = export(&non_isolated_group(), true);
    let (bad, worst) = playback_against_raster("non-isolated", &reference, &emf.emf, ROUNDING);
    assert_eq!(bad, 0, "worst {worst}");
    let (reference, emf) = export(&non_isolated_group(), false);
    let (bad, _) = playback_against_raster("non-isolated-off", &reference, &emf.emf, ROUNDING);
    assert!(bad > 1000, "{bad}");
}

#[test]
fn a_cmyk_page_group_blend_is_patched() {
    let (_, off) = export(&cmyk_page_group(), false);
    assert!(off.outcome.tally.colorant_buffer_on_screen > 0);
    let (_, on) = export(&cmyk_page_group(), true);
    let o = &on.outcome;
    assert_eq!(o.tally.colorant_buffer_on_screen, 0);
    assert!(o.patches.blending_space > 0, "{:?}", o.patches);
    assert_eq!(o.print_fallback(), None);
    // Only the Difference fill composites; its box, not the page.
    assert_eq!(bitmap_sizes(&on.emf), vec![(82, 82)]);
}

#[cfg(windows)]
#[test]
fn a_cmyk_page_group_blend_plays_back_like_the_raster() {
    let (reference, emf) = export(&cmyk_page_group(), true);
    let (bad, worst) = playback_against_raster("cmyk-group", &reference, &emf.emf, ROUNDING);
    assert_eq!(bad, 0, "worst {worst}");
    let (reference, emf) = export(&cmyk_page_group(), false);
    let (bad, _) = playback_against_raster("cmyk-group-off", &reference, &emf.emf, ROUNDING);
    assert!(bad > 1000, "{bad}");
}

/// An unextended two-circle radial shading (no native gradient form, so
/// it is harvested as a raster) drawn wholly under the overprinting
/// magenta of [`OVERPRINT`], so the patch for the overlap covers it.
fn shading_under_a_patch() -> Vec<u8> {
    let shading = "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [50 30 1 50 30 4] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 0] /C1 [0 0 1] /N 1 >> \
         /Extend [false false] >>";
    page(
        &format!("{OVERPRINT_RES} /Shading << /Sh0 {shading} >>"),
        &format!("q 45 25 10 10 re W n /Sh0 sh Q {OVERPRINT}"),
    )
}

/// A shading the patch already carries as pixels is not a rasterised
/// shading the print route has to fall back for.
#[test]
fn a_shading_inside_a_patch_does_not_send_the_page_to_raster() {
    let (_, off) = export(&shading_under_a_patch(), false);
    assert_eq!(off.outcome.tally.shadings_rasterised, 1);
    let (_, on) = export(&shading_under_a_patch(), true);
    let o = &on.outcome;
    assert!(o.patches.overprint > 0, "{:?}", o.patches);
    assert_eq!(o.tally.shadings_rasterised, 0);
    assert_eq!(o.print_fallback(), None);
}

/// FNV-1a, 64-bit.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn with_the_switch_off_the_metafile_is_unchanged() {
    let bytes = page(OVERPRINT_RES, OVERPRINT);
    let doc = Document::from_bytes(bytes).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let base = EmfOptions::default().with_raster_dpi(DPI);
    let default = export_emf(&doc, &p, &RenderOptions::default(), &base).unwrap();
    let off = export_emf(
        &doc,
        &p,
        &RenderOptions::default(),
        &base.clone().with_patch_approximations(false),
    )
    .unwrap();
    assert_eq!(default.emf, off.emf);
    // The bytes the writer produced before the switch existed.
    assert_eq!(off.emf.len(), 692);
    assert_eq!(fnv1a(&off.emf), 0x7438_7fba_aab1_058a);
    assert_eq!(off.outcome.patches.cases(), 0);
}
