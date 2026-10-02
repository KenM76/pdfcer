//! `PatternType 1` tiling patterns (ISO 32000-1 §8.7.3, Table 75; ISO
//! 32000-2 §8.7.3): the dictionary, the cell raster plan and the tiled
//! paint. Running the pattern's content stream into the cell is the
//! interpreter's job (`interpret::tiling`); this module owns geometry.
//!
//! The cell is rendered ONCE, axis-aligned in pattern space, at roughly
//! device resolution. Copies of the `/BBox` raster that overlap the
//! `XStep × YStep` period window are folded into one periodic cell, which a
//! `SpreadMode::Repeat` shader then replicates through
//! `base CTM × /Matrix`. Cost is therefore independent of how many tiles
//! the fill covers; it is bounded by [`MAX_CELL_PIXELS`] (raster size) and
//! [`MAX_CELL_COPIES`] (overlap folding).
//!
//! `TilingType` 1, 2 and 3 are painted alike: rounding the period to whole
//! cell pixels distorts spacing by under one device pixel, which Table 75
//! permits for types 1 and 3 and, as spacing variation, for type 2.

use std::cell::Cell;

use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::view::DocumentView;
use tiny_skia::{
    FilterQuality, Mask, Paint, Pattern, Pixmap, PixmapPaint, Rect, SpreadMode, Transform,
};

/// Ceiling on the pixels of either raster (the `/BBox` raster and the
/// period cell). Above it the cell is rendered at reduced resolution.
pub(crate) const MAX_CELL_PIXELS: u64 = 1 << 22;

/// Ceiling on `/BBox` copies folded into one period cell. A `/BBox` many
/// times larger than `XStep × YStep` (or a tiny step under a normal box)
/// exceeds it and the pattern is refused rather than folded.
pub(crate) const MAX_CELL_COPIES: u64 = 4096;

/// Ceiling on tiling patterns nested inside tiling patterns. Each level
/// holds up to two [`MAX_CELL_PIXELS`] rasters while its content runs, so
/// this bounds memory where the shared XObject depth limit alone would not.
pub(crate) const MAX_PATTERN_NESTING: usize = 6;

/// Table 75 `PaintType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintType {
    /// 1 — the content stream specifies its own colours.
    Coloured,
    /// 2 — shape only; the colour comes from the `scn` operands in the
    /// underlying space (§8.7.3.3).
    Uncoloured,
}

/// A parsed Table 75 dictionary.
#[derive(Debug, Clone)]
pub(crate) struct TilingPattern {
    pub paint_type: PaintType,
    /// `/BBox`, normalised so `x0 < x1`, `y0 < y1`.
    pub bbox: [f32; 4],
    /// `|XStep|`, `|YStep|`. The sign is irrelevant to the lattice
    /// (`{k · XStep}` over all integers `k` is the same set either way).
    pub step: (f32, f32),
    /// `/Matrix`, pattern space → the parent stream's default space.
    pub matrix: Transform,
}

impl TilingPattern {
    /// Read the Table 75 entries pdfcer needs. `Err` names the defect.
    pub(crate) fn parse(doc: &DocumentView<'_>, dict: &Dict) -> Result<Self, &'static str> {
        let num = |key: &[u8]| {
            dict.get(key)
                .map(|o| doc.resolve(o))
                .and_then(Object::as_number)
        };
        let paint_type = match num(b"PaintType").map(|v| v as i64) {
            Some(1) => PaintType::Coloured,
            Some(2) => PaintType::Uncoloured,
            _ => return Err("missing or invalid /PaintType"),
        };
        let bbox = read_rect(doc, dict).ok_or("missing or degenerate /BBox")?;
        let (Some(xs), Some(ys)) = (num(b"XStep"), num(b"YStep")) else {
            return Err("missing /XStep or /YStep");
        };
        #[allow(clippy::cast_possible_truncation)]
        let step = ((xs as f32).abs(), (ys as f32).abs());
        // Table 75: "shall not be zero".
        if !(step.0.is_finite() && step.1.is_finite() && step.0 > 0.0 && step.1 > 0.0) {
            return Err("zero or non-finite /XStep or /YStep");
        }
        Ok(Self {
            paint_type,
            bbox,
            step,
            matrix: crate::interpret::pattern_matrix(doc, dict),
        })
    }
}

fn read_rect(doc: &DocumentView<'_>, dict: &Dict) -> Option<[f32; 4]> {
    let arr = dict
        .get(b"BBox")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)?;
    #[allow(clippy::cast_possible_truncation)]
    let n: Vec<f32> = arr
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .map(|v| v as f32)
        .collect();
    let &[a, b, c, d] = n.as_slice() else {
        return None;
    };
    let r = [a.min(c), b.min(d), a.max(c), b.max(d)];
    (r.iter().all(|v| v.is_finite()) && r[2] > r[0] && r[3] > r[1]).then_some(r)
}

/// Why a plan could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanRefusal {
    /// The pattern-to-device transform collapses an axis.
    Degenerate,
    /// More than [`MAX_CELL_COPIES`] `/BBox` copies overlap one period.
    TooManyCopies(u64),
}

/// The raster plan for one tiled paint.
#[derive(Debug, Clone)]
pub(crate) struct CellPlan {
    /// Period cell size in pixels.
    pub cell: (u32, u32),
    /// `/BBox` raster size in pixels.
    pub bbox_px: (u32, u32),
    /// `/BBox` copies per axis folded into the cell.
    pub copies: (u32, u32),
    /// Pattern space → `/BBox` raster pixels; the content's initial CTM.
    pub bbox_ctm: Transform,
    /// Cell pixels → device pixels; the shader's transform.
    pub shader: Transform,
    /// Whether the resolution was reduced to fit [`MAX_CELL_PIXELS`].
    pub reduced: bool,
}

/// Plan the cell for `pattern` painted through `to_device`
/// (`base CTM × /Matrix`, §8.7.2).
pub(crate) fn plan(pattern: &TilingPattern, to_device: Transform) -> Result<CellPlan, PlanRefusal> {
    let [x0, y0, x1, y1] = pattern.bbox;
    let (xs, ys) = pattern.step;
    // Device length of one pattern unit along each pattern axis.
    let sx = to_device.sx.hypot(to_device.ky);
    let sy = to_device.kx.hypot(to_device.sy);
    let det = to_device.sx * to_device.sy - to_device.kx * to_device.ky;
    if !(sx.is_finite() && sy.is_finite() && det.is_finite()) || sx <= 0.0 || sy <= 0.0 {
        return Err(PlanRefusal::Degenerate);
    }
    if det == 0.0 {
        return Err(PlanRefusal::Degenerate);
    }
    let (bw_f, bh_f) = ((x1 - x0) * sx, (y1 - y0) * sy);
    let (cw_f, ch_f) = (xs * sx, ys * sy);
    let copies = (
        (f64::from(x1 - x0) / f64::from(xs)).ceil().max(1.0),
        (f64::from(y1 - y0) / f64::from(ys)).ceil().max(1.0),
    );
    let total = copies.0 * copies.1;
    if !total.is_finite() || total > MAX_CELL_COPIES as f64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Err(PlanRefusal::TooManyCopies(total.min(u64::MAX as f64) as u64));
    }
    let area = f64::from(cw_f.max(1.0) * ch_f.max(1.0)).max(f64::from(bw_f * bh_f));
    let fit = (MAX_CELL_PIXELS as f64 / area).sqrt();
    let reduced = fit < 1.0;
    #[allow(clippy::cast_possible_truncation)]
    let k = fit.min(1.0) as f32;
    let cell = (px(cw_f * k), px(ch_f * k));
    #[allow(clippy::cast_precision_loss)]
    let (rx, ry) = (cell.0 as f32 / xs, cell.1 as f32 / ys);
    let bbox_px = (px((x1 - x0) * rx), px((y1 - y0) * ry));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let copies = (
        bbox_px.0.div_ceil(cell.0).max(copies.0 as u32),
        bbox_px.1.div_ceil(cell.1).max(copies.1 as u32),
    );
    Ok(CellPlan {
        cell,
        bbox_px,
        copies,
        bbox_ctm: Transform::from_row(rx, 0.0, 0.0, ry, -x0 * rx, -y0 * ry),
        shader: Transform::from_row(1.0 / rx, 0.0, 0.0, 1.0 / ry, x0, y0).post_concat(to_device),
        reduced,
    })
}

/// Whole pixels for a raster edge: at least 1, and capped so a pathological
/// value cannot overflow the allocation arithmetic.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn px(v: f32) -> u32 {
    if v.is_finite() {
        v.ceil().clamp(1.0, 65_535.0) as u32
    } else {
        1
    }
}

/// Fold the `/BBox` raster into one period cell: every copy at a lattice
/// offset that overlaps `[0, XStep) × [0, YStep)` is drawn into it, so
/// content spilling past the step (§8.7.3.1 permits overlap) wraps.
pub(crate) fn fold_cell(bbox: &Pixmap, plan: &CellPlan) -> Option<Pixmap> {
    let (cw, ch) = plan.cell;
    let mut cell = Pixmap::new(cw, ch)?;
    for ky in 0..plan.copies.1 {
        for kx in 0..plan.copies.0 {
            let x = i64::from(kx) * -i64::from(cw);
            let y = i64::from(ky) * -i64::from(ch);
            let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                continue;
            };
            cell.draw_pixmap(
                x,
                y,
                bbox.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }
    Some(cell)
}

/// Paint `region` of `dest` with the cell repeated through `plan.shader`,
/// under `mask` (path coverage × clip).
pub(crate) fn paint_tiled(
    dest: &mut Pixmap,
    cell: &Pixmap,
    plan: &CellPlan,
    mask: &Mask,
    region: (i32, i32, i32, i32),
    alpha: f32,
    blend: tiny_skia::BlendMode,
) -> bool {
    #[allow(clippy::cast_precision_loss)]
    let Some(rect) = Rect::from_ltrb(
        region.0 as f32,
        region.1 as f32,
        region.2 as f32,
        region.3 as f32,
    ) else {
        return false;
    };
    // An axis-aligned shader is sampled Nearest: the cell is already at
    // device resolution and bilinear would only soften it.
    let quality = if plan.shader.kx == 0.0 && plan.shader.ky == 0.0 && !plan.reduced {
        FilterQuality::Nearest
    } else {
        FilterQuality::Bilinear
    };
    let paint = Paint {
        shader: Pattern::new(
            cell.as_ref(),
            SpreadMode::Repeat,
            quality,
            alpha.clamp(0.0, 1.0),
            plan.shader,
        ),
        blend_mode: blend,
        anti_alias: false,
        force_hq_pipeline: false,
    };
    dest.fill_rect(rect, &paint, Transform::identity(), Some(mask));
    true
}

thread_local! {
    static NESTING: Cell<usize> = const { Cell::new(0) };
}

/// One level of tiling-pattern nesting on this thread; released on drop.
pub(crate) struct NestGuard(());

impl NestGuard {
    /// `None` when [`MAX_PATTERN_NESTING`] levels are already open.
    pub(crate) fn enter() -> Option<Self> {
        NESTING.with(|n| {
            if n.get() >= MAX_PATTERN_NESTING {
                None
            } else {
                n.set(n.get() + 1);
                Some(Self(()))
            }
        })
    }
}

impl Drop for NestGuard {
    fn drop(&mut self) {
        NESTING.with(|n| n.set(n.get().saturating_sub(1)));
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn pat(bbox: [f32; 4], step: (f32, f32)) -> TilingPattern {
        TilingPattern {
            paint_type: PaintType::Coloured,
            bbox,
            step,
            matrix: Transform::identity(),
        }
    }

    #[test]
    fn the_cell_is_the_step_at_device_scale() {
        let p = plan(
            &pat([0.0, 0.0, 5.0, 5.0], (10.0, 20.0)),
            Transform::from_scale(2.0, 2.0),
        )
        .unwrap();
        assert_eq!(p.cell, (20, 40));
        assert_eq!(p.bbox_px, (10, 10));
        assert_eq!(p.copies, (1, 1));
        assert!(!p.reduced);
    }

    #[test]
    fn a_bbox_wider_than_the_step_folds_several_copies() {
        let p = plan(
            &pat([0.0, 0.0, 25.0, 10.0], (10.0, 10.0)),
            Transform::identity(),
        )
        .unwrap();
        assert_eq!(p.copies, (3, 1));
    }

    #[test]
    fn a_tiny_step_under_a_normal_box_is_refused() {
        let r = plan(
            &pat([0.0, 0.0, 100.0, 100.0], (0.01, 0.01)),
            Transform::identity(),
        );
        assert!(matches!(r, Err(PlanRefusal::TooManyCopies(_))));
    }

    #[test]
    fn a_huge_cell_is_reduced_under_the_ceiling() {
        let p = plan(
            &pat([0.0, 0.0, 10_000.0, 10_000.0], (10_000.0, 10_000.0)),
            Transform::from_scale(4.0, 4.0),
        )
        .unwrap();
        assert!(p.reduced);
        assert!(u64::from(p.cell.0) * u64::from(p.cell.1) <= MAX_CELL_PIXELS);
        assert!(u64::from(p.bbox_px.0) * u64::from(p.bbox_px.1) <= MAX_CELL_PIXELS);
    }

    #[test]
    fn a_collapsed_matrix_is_degenerate() {
        let r = plan(
            &pat([0.0, 0.0, 1.0, 1.0], (1.0, 1.0)),
            Transform::from_row(1.0, 0.0, 2.0, 0.0, 0.0, 0.0),
        );
        assert_eq!(r.unwrap_err(), PlanRefusal::Degenerate);
    }

    #[test]
    fn folding_wraps_overspill_into_the_cell() {
        // A 2-px-wide red bbox raster over a 1-px period: both copies land.
        let p = plan(
            &pat([0.0, 0.0, 2.0, 1.0], (1.0, 1.0)),
            Transform::identity(),
        )
        .unwrap();
        let mut bbox = Pixmap::new(2, 1).unwrap();
        bbox.pixels_mut()[1] = tiny_skia::ColorU8::from_rgba(255, 0, 0, 255).premultiply();
        let cell = fold_cell(&bbox, &p).unwrap();
        assert_eq!(
            cell.pixels()[0].red(),
            255,
            "the spilled pixel wraps to x = 0"
        );
    }

    #[test]
    fn nesting_is_bounded_and_released() {
        let guards: Vec<_> = (0..MAX_PATTERN_NESTING)
            .map(|_| NestGuard::enter().unwrap())
            .collect();
        assert!(NestGuard::enter().is_none());
        drop(guards);
        assert!(NestGuard::enter().is_some());
    }
}
