//! Pixel patches in place of the export recording's approximations
//! ([`super::EmfOptions::patch_approximations`]).
//!
//! The recording draws some content approximately (overprint as knockout,
//! a non-isolated group as isolated, a nested blend as `Normal`, a
//! subtractive page group's compositing in RGB) and logs where. Each such
//! region becomes a rectangle; the page's own raster supplies the true
//! pixels there. The vector content is written with every rectangle
//! excluded from the clip, then each rectangle is written as one bitmap of
//! the raster, so the bitmap holds the final composite of everything that
//! paints there, in any order. Everything outside the rectangles stays as
//! it was.

use std::sync::Arc;

use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;
use tiny_skia::{BlendMode, Pixmap};

use super::{Writer, rectl};
use pdfcer_render::font::RenderOptions;
use pdfcer_render::writer_seam::{Brush, ShadingSource};
use pdfcer_render::writer_seam::{DeviceBounds, ExportRecording, Op};
use pdfcer_render::writer_seam::{Site, SiteCause, op_box};

/// What [`super::EmfOptions::with_patch_approximations`] replaced with
/// pixels. All zero with the switch off.
///
/// Each cause counts the distinct regions it was found in (a region inside
/// another of the same cause is not counted again). Regions of different
/// causes that overlap share one bitmap; [`Self::rasters`] counts bitmaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EmfPatches {
    /// Overprinted paint over other paint, which the recording would have
    /// drawn as a knockout. An overprint over bare paper stays vector: it
    /// looks the same as a knockout there.
    pub overprint: usize,
    /// Non-isolated groups, which the recording would have composited
    /// against a transparent backdrop (ISO 32000-2 §11.4.7).
    pub non_isolated_groups: usize,
    /// Blends inside groups, or a group's own blend, which the writer would
    /// have drawn `Normal`.
    pub blends: usize,
    /// Compositing in a subtractive page group or colorant space, which the
    /// recording would have done in RGB (§11.7.2).
    pub blending_space: usize,
    /// Ops [`crate::EmfOptions::vector_transparency`] could not write as
    /// regions; why is in [`crate::EmfVectorised`].
    pub transparency: usize,
    /// `EMR_ALPHABLEND` records the patches were written as.
    pub rasters: usize,
}

impl EmfPatches {
    /// Regions patched, every cause summed.
    #[must_use]
    pub fn cases(&self) -> usize {
        self.overprint
            + self.non_isolated_groups
            + self.blends
            + self.blending_space
            + self.transparency
    }
}

/// A pixel rectangle, `x0..x1` × `y0..y1`.
pub(super) type PxRect = (u32, u32, u32, u32);

/// The rectangles to patch and the raster that fills them.
pub(super) struct Plan {
    rects: Vec<PxRect>,
    truth: Pixmap,
    /// Per rectangle: excluded from the vector content but written as no
    /// bitmap, because its true pixels are bare paper.
    blank: Vec<bool>,
    pub patches: EmfPatches,
}

impl Plan {
    /// A plan over already merged `rects`, each filled from `truth` unless
    /// it is `blank`.
    pub(super) fn new(rects: Vec<PxRect>, truth: Pixmap, blank: Vec<bool>) -> Self {
        Self {
            rects,
            truth,
            blank,
            patches: EmfPatches::default(),
        }
    }
}

/// Plan the patches for `rec` (after [`pdfcer_render::writer_seam::flatten_blends`]) and drop the
/// top-level ops lying wholly inside one. `extra` are boxes patched besides
/// the recording's own sites. With `tight`, an op lies inside by what it
/// paints ([`crate::EmfOptions::tight_mask_patches`]). `None` when there is
/// nothing to patch or the raster could not be made; then nothing is
/// changed.
#[allow(clippy::too_many_arguments)] // each is a distinct input of one plan
pub(super) fn plan(
    rec: &mut ExportRecording,
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    scale: f32,
    extra: &[DeviceBounds],
    tight: bool,
    native: bool,
) -> Option<Plan> {
    let (w, h) = rec.page_size;
    let sites = sites(rec, tight, native);
    let rects = merge(
        sites
            .iter()
            .map(|s| s.bounds)
            .chain(extra.iter().copied())
            .filter_map(|b| px_rect(b, w, h))
            .collect(),
    );
    if !rects.is_empty() {
        let options = render
            .clone()
            .with_backdrop(pdfcer_render::PageBackdrop::Transparent);
        let truth = pdfcer_render::render_page_with_view(view, page, scale, &options)
            .ok()?
            .pixmap;
        if (truth.width(), truth.height()) != (w, h) {
            return None;
        }
        let covered = |op: &Op| {
            reach(op, (w, h), tight)
                .and_then(|b| px_rect(b, w, h))
                .is_some_and(|r| rects.iter().any(|c| contains(*c, r)))
        };
        // A shading inside a patch is the raster's own pixels, not a
        // rasterised shading the print route must fall back for.
        let patched_shadings: u32 = rec
            .ops
            .iter()
            .filter(|op| covered(op))
            .map(|op| harvested_shadings(op, &rec.shadings))
            .sum();
        let t = &mut rec.tally;
        t.shadings_rasterised = t.shadings_rasterised.saturating_sub(patched_shadings);
        rec.ops.retain(|op| !covered(op));
        let mut patches = count(&sites, rects.len());
        patches.transparency = extra.len();
        zero_tallies(rec);
        return Some(Plan {
            rects,
            truth,
            blank: Vec::new(),
            patches,
        });
    }
    zero_tallies(rec);
    None
}

/// Every approximated region: the recorder's, plus what only the finished
/// op list shows.
fn sites(rec: &ExportRecording, tight: bool, native: bool) -> Vec<Site> {
    let mut out = rec.sites.clone();
    for op in &rec.ops {
        let Some(bounds) = reach(op, rec.page_size, tight) else {
            continue;
        };
        if rec.subtractive_page && composites_on(op, rec, native) {
            out.push(Site {
                cause: SiteCause::BlendingSpace,
                bounds,
            });
        }
        if drops_blend(op) {
            out.push(Site {
                cause: SiteCause::Blend,
                bounds,
            });
        }
    }
    out
}

/// Where `op` can change the page: its box, or with `tight` what it paints
/// ([`crate::EmfOptions::tight_mask_patches`]), so a soft-masked layer is
/// patched, and dropped inside a patch, by its mask's nonzero pixels.
fn reach(op: &Op, page: (u32, u32), tight: bool) -> Option<DeviceBounds> {
    tight
        .then(|| crate::painted::painted(op, page))
        .flatten()
        .or_else(|| op_box(op))
}

/// Shadings in `op` the recorder harvested as image ops (each counted in
/// `ExportTally::shadings_rasterised`).
pub(crate) fn harvested_shadings(op: &Op, shadings: &[(Arc<Pixmap>, ShadingSource)]) -> u32 {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => match &brush.brush {
            Brush::Image { texels, .. } => {
                u32::from(shadings.iter().any(|(t, _)| Arc::ptr_eq(t, texels)))
            }
            _ => 0,
        },
        Op::Layer { ops, .. } | Op::Text { ops, .. } => {
            ops.iter().map(|o| harvested_shadings(o, shadings)).sum()
        }
    }
}

/// The recording's approximations are all patched: none is left to count.
fn zero_tallies(rec: &mut ExportRecording) {
    let t = &mut rec.tally;
    t.overprint_approximated = 0;
    t.nonseparable_approximated = 0;
    t.non_isolated_groups_isolated = 0;
    t.colorant_buffer_on_screen = 0;
}

/// Whether `op`'s result depends on what lies beneath it.
pub(super) fn composites(op: &Op) -> bool {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => match brush.brush {
            Brush::Solid { rgba } => {
                rgba[3] < 255
                    || brush.blend != BlendMode::SourceOver
                    || brush.spots.iter().any(|s| s.own_appearance)
            }
            _ => true,
        },
        Op::Layer { .. } => true,
        Op::Text { ops, .. } => ops.iter().any(composites),
    }
}

/// [`composites`], except that with `native` shadings
/// ([`crate::EmfOptions::vector_shadings`]) a harvested shading the
/// triangle route draws on its own does not: opaque, `Normal`, with no
/// overprint and no spot inks, it paints the same over any backdrop.
pub(super) fn composites_on(op: &Op, rec: &ExportRecording, native: bool) -> bool {
    composites(op) && !(native && opaque_shading(op, &rec.shadings))
}

fn opaque_shading(op: &Op, shadings: &[(Arc<Pixmap>, ShadingSource)]) -> bool {
    let Op::Fill { brush, .. } = op else {
        return false;
    };
    let Brush::Image { texels, .. } = &brush.brush else {
        return false;
    };
    brush.blend == BlendMode::SourceOver
        && brush.spots.is_empty()
        && shadings.iter().any(|(t, s)| {
            Arc::ptr_eq(t, texels)
                && s.alpha >= 1.0 - 1e-6
                && s.blend == BlendMode::SourceOver
                && s.overprint.is_none()
                && s.spots.is_empty()
        })
}

/// Whether the writer would draw a blend in `op` as `Normal`.
fn drops_blend(op: &Op) -> bool {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => brush.blend != BlendMode::SourceOver,
        Op::Layer { paint, ops, .. } => {
            paint.blend != BlendMode::SourceOver
                || paint.nonseparable.is_some()
                || ops.iter().any(drops_blend)
        }
        Op::Text { ops, .. } => ops.iter().any(drops_blend),
    }
}

/// `b` grown by a pixel of antialiasing each side, clamped to the page.
pub(super) fn px_rect(b: DeviceBounds, w: u32, h: u32) -> Option<PxRect> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let clamp = |v: f32, max: u32| {
        if v.is_nan() {
            0
        } else {
            v.clamp(0.0, max as f32) as u32
        }
    };
    let r = (
        clamp(b.left.floor() - 1.0, w),
        clamp(b.top.floor() - 1.0, h),
        clamp(b.right.ceil() + 1.0, w),
        clamp(b.bottom.ceil() + 1.0, h),
    );
    (r.0 < r.2 && r.1 < r.3).then_some(r)
}

fn overlaps(a: PxRect, b: PxRect) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// Whether `inner` lies wholly within `outer` (edges inclusive).
pub(super) fn contains(outer: PxRect, inner: PxRect) -> bool {
    outer.0 <= inner.0 && outer.1 <= inner.1 && inner.2 <= outer.2 && inner.3 <= outer.3
}

/// Union overlapping rectangles until no two overlap: each pixel is then
/// written by at most one bitmap.
pub(super) fn merge(mut rects: Vec<PxRect>) -> Vec<PxRect> {
    let mut out: Vec<PxRect> = Vec::new();
    while let Some(mut r) = rects.pop() {
        while let Some(i) = out.iter().position(|o| overlaps(*o, r)) {
            let o = out.swap_remove(i);
            r = (r.0.min(o.0), r.1.min(o.1), r.2.max(o.2), r.3.max(o.3));
        }
        out.push(r);
    }
    out.sort_unstable();
    out
}

/// Distinct regions per cause.
fn count(sites: &[Site], rasters: usize) -> EmfPatches {
    let distinct = |cause: SiteCause| {
        let of: Vec<DeviceBounds> = sites
            .iter()
            .filter(|s| s.cause == cause)
            .map(|s| s.bounds)
            .collect();
        of.iter()
            .enumerate()
            .filter(|(i, b)| {
                !of.iter()
                    .enumerate()
                    .any(|(j, o)| j != *i && within(**b, *o) && (!within(*o, **b) || j < *i))
            })
            .count()
    };
    EmfPatches {
        overprint: distinct(SiteCause::Overprint),
        non_isolated_groups: distinct(SiteCause::NonIsolatedGroup),
        blends: distinct(SiteCause::Blend),
        blending_space: distinct(SiteCause::BlendingSpace),
        transparency: 0,
        rasters,
    }
}

/// Whether `inner` lies entirely inside `outer`.
pub(super) fn within(inner: DeviceBounds, outer: DeviceBounds) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

impl Writer<'_> {
    /// Open the exclusion: everything written until [`Self::patch_close`]
    /// leaves the rectangles untouched. The raster prologue goes first so
    /// the closing `RESTOREDC` cannot undo it.
    pub(super) fn patch_open(&mut self, plan: &Plan) {
        self.raster_prologue();
        self.record(0x21, &[]); // SAVEDC
        for &(x0, y0, x1, y1) in &plan.rects {
            let r = self.lu_rect(x0, y0, x1, y1);
            self.record(0x1D, &rectl(r.0, r.1, r.2, r.3)); // EXCLUDECLIPRECT
        }
    }

    /// Close the exclusion and write each rectangle's true pixels.
    pub(super) fn patch_close(&mut self, plan: &Plan) {
        self.end_clip();
        self.record(0x22, &super::i32le(-1)); // RESTOREDC -1
        let before = self.outcome.rasters_embedded;
        for (i, &(x0, y0, x1, y1)) in plan.rects.iter().enumerate() {
            if plan.blank.get(i) == Some(&true) {
                continue;
            }
            let r = self.lu_rect(x0, y0, x1, y1);
            let dest = (r.0, r.1, (r.2 - r.0).max(1), (r.3 - r.1).max(1));
            self.bitmap(&plan.truth, (x0, y0, x1 - x0, y1 - y0), dest);
        }
        self.outcome.patches = EmfPatches {
            rasters: self.outcome.rasters_embedded - before,
            ..plan.patches
        };
    }

    /// A pixel rectangle's edges in logical units.
    fn lu_rect(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> (i32, i32, i32, i32) {
        #[allow(clippy::cast_precision_loss)]
        let f = |v: u32| self.lu(v as f32);
        (f(x0), f(y0), f(x1), f(y1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_rectangles_merge_and_disjoint_ones_stay() {
        let out = merge(vec![(0, 0, 4, 4), (10, 10, 12, 12), (3, 3, 6, 6)]);
        assert_eq!(out, vec![(0, 0, 6, 6), (10, 10, 12, 12)]);
    }

    #[test]
    fn a_chain_of_overlaps_merges_into_one() {
        let out = merge(vec![(0, 0, 2, 2), (4, 0, 6, 2), (1, 0, 5, 2)]);
        assert_eq!(out, vec![(0, 0, 6, 2)]);
    }

    #[test]
    fn a_rectangle_is_grown_a_pixel_and_clamped_to_the_page() {
        let b = DeviceBounds {
            left: 0.5,
            top: 2.0,
            right: 9.2,
            bottom: f32::INFINITY,
        };
        assert_eq!(px_rect(b, 10, 20), Some((0, 1, 10, 20)));
    }
}
