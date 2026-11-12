//! Type 1 (function-based) shadings, painted (ISO 32000-2 §8.7.4.5.2,
//! Table 78; ISO 32000-1 Table 79).
//!
//! The 2-in colour function is sampled once on a [`GRID`] × [`GRID`]
//! lattice over `/Domain`, each node converted to sRGB through the page's
//! bridges, and interpolated bilinearly per pixel. §10.7.3's smoothness
//! tolerance is the latitude for that (spec RAG `SH12`, `SH44`); a
//! discontinuous function is softened across one lattice cell.
//!
//! A device pixel maps to target space by the route's `to_target`, then to
//! the domain by `/Matrix`⁻¹ (`SH20`). Outside the domain rectangle it is
//! left unpainted (`SH21`): `/Background` is not painted, as for types 2
//! and 3. A node where the function or the colour conversion fails leaves
//! its cells unpainted (`AMB-6`, the recommended reading).

use std::sync::Arc;

use pdfcer_core::settings::CmykIntent;

use super::{Geometry, Shading, ShadingDiagnostics, ShadingFunction, source_over, target_at};
use crate::color::{ColorDiagnostics, ColorSpace};
use crate::icc::ColorBridges;

/// Lattice nodes per domain axis.
const GRID: usize = 128;

/// The grid for a type 1 `geometry`, counted and noted in `diag` like a
/// ramp; `None` for any other type, or when there is no colour to sample.
pub(super) fn grid_for(
    geometry: &Geometry,
    function: Option<&ShadingFunction>,
    (space, bridges, intent): (&ColorSpace, &ColorBridges, CmykIntent),
    diag: &mut ShadingDiagnostics,
    color_diag: &mut ColorDiagnostics,
) -> Option<Arc<FunctionGrid>> {
    let Geometry::FunctionBased { domain, matrix } = *geometry else {
        return None;
    };
    let function = function?;
    diag.ramps_sampled += 1;
    let Some(grid) = FunctionGrid::build(
        function,
        (domain, matrix),
        space,
        bridges,
        intent,
        color_diag,
    ) else {
        diag.note("type 1 shading has an empty /Domain or a non-invertible /Matrix, so no point has a colour");
        return None;
    };
    if !grid.is_complete() {
        diag.ramps_incomplete += 1;
        diag.note(
            "type 1 shading /Function failed at one or more samples; those cells paint nothing",
        );
    }
    Some(Arc::new(grid))
}

/// A type 1 shading's colour, pre-sampled over its domain.
#[derive(Debug, Clone)]
pub(super) struct FunctionGrid {
    /// `/Domain` as `[xmin xmax ymin ymax]` (Table 78's order).
    domain: [f32; 4],
    /// `/Matrix`⁻¹: target space to domain space.
    from_target: tiny_skia::Transform,
    /// sRGB per node, row-major in `y`; `None` where the function failed.
    nodes: Vec<Option<[f32; 3]>>,
}

impl FunctionGrid {
    /// Sample `function` over `domain`. `None` for an empty domain or a
    /// non-invertible `/Matrix`, under which no point has a colour.
    fn build(
        function: &ShadingFunction,
        (domain, matrix): ([f32; 4], [f32; 6]),
        space: &ColorSpace,
        bridges: &ColorBridges,
        intent: CmykIntent,
        diag: &mut ColorDiagnostics,
    ) -> Option<Self> {
        let [a, b, c, d, e, f] = matrix;
        let from_target = tiny_skia::Transform::from_row(a, b, c, d, e, f).invert()?;
        let [x0, x1, y0, y1] = domain;
        if !(x1 > x0 && y1 > y0) {
            return None;
        }
        let mut nodes = Vec::with_capacity(GRID * GRID);
        let (mut raw, mut comps) = (Vec::new(), Vec::<f32>::new());
        #[allow(clippy::cast_precision_loss)] // GRID is 128.
        let step = |lo: f32, hi: f32, i: usize| {
            f64::from(lo) + f64::from(hi - lo) * i as f64 / (GRID - 1) as f64
        };
        for j in 0..GRID {
            for i in 0..GRID {
                if !function.eval(&[step(x0, x1, i), step(y0, y1, j)], &mut raw) {
                    nodes.push(None);
                    continue;
                }
                comps.clear();
                #[allow(clippy::cast_possible_truncation)]
                comps.extend(raw.iter().map(|v| *v as f32));
                nodes.push(
                    bridges
                        .to_rgb(space, &comps, intent, diag)
                        .map(|c| [c.r, c.g, c.b]),
                );
            }
        }
        Some(Self {
            domain,
            from_target,
            nodes,
        })
    }

    /// Whether every node has a colour.
    fn is_complete(&self) -> bool {
        self.nodes.iter().all(Option::is_some)
    }

    /// The colour at target-space point `pt`, or `None` outside the domain
    /// or beside a failed node.
    fn at(&self, mut pt: tiny_skia::Point) -> Option<[f32; 3]> {
        self.from_target.map_point(&mut pt);
        let [x0, x1, y0, y1] = self.domain;
        if !(x0..=x1).contains(&pt.x) || !(y0..=y1).contains(&pt.y) {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let last = (GRID - 1) as f32;
        let (u, v) = (
            (pt.x - x0) / (x1 - x0) * last,
            (pt.y - y0) / (y1 - y0) * last,
        );
        // In 0..=GRID-2 after the clamp, so the cast is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cell = |t: f32| t.floor().clamp(0.0, last - 1.0) as usize;
        let (i, j) = (cell(u), cell(v));
        #[allow(clippy::cast_precision_loss)]
        let (fu, fv) = (u - i as f32, v - j as f32);
        let node =
            |di: usize, dj: usize| self.nodes.get((j + dj) * GRID + i + di).copied().flatten();
        let (c00, c10, c01, c11) = (node(0, 0)?, node(1, 0)?, node(0, 1)?, node(1, 1)?);
        let mut out = [0.0; 3];
        for (k, o) in out.iter_mut().enumerate() {
            let ch = |c: [f32; 3]| c.get(k).copied().unwrap_or(0.0);
            let top = fu.mul_add(ch(c10) - ch(c00), ch(c00));
            let bottom = fu.mul_add(ch(c11) - ch(c01), ch(c01));
            *o = fv.mul_add(bottom - top, top);
        }
        Some(out)
    }
}

/// Paint `shading`'s `grid` over `region`, as [`Shading::paint`] does for
/// types 2 and 3. Returns the pixels written.
pub(super) fn paint(
    shading: &Shading,
    grid: &FunctionGrid,
    to_target: tiny_skia::Transform,
    region: (i32, i32, i32, i32),
    clip: Option<&tiny_skia::Mask>,
    alpha: f32,
    pixmap: &mut tiny_skia::Pixmap,
) -> usize {
    let (x_lo, y_lo, x_hi, y_hi) = region;
    // Pixmap sides are far below `i32::MAX`.
    #[allow(clippy::cast_possible_wrap)]
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let alpha = alpha.clamp(0.0, 1.0);
    let mut painted = 0;
    for py in y_lo.max(0)..y_hi.min(height) {
        for px in x_lo.max(0)..x_hi.min(width) {
            let Some((pt, coverage)) = target_at(shading, to_target, px, py, width, clip) else {
                continue;
            };
            let Some(rgb) = grid.at(pt) else {
                continue;
            };
            // Both are non-negative and inside the pixmap.
            #[allow(clippy::cast_sign_loss)]
            let idx = (py as usize) * (width as usize) + (px as usize);
            painted += usize::from(source_over(pixmap, idx, rgb, alpha * coverage));
        }
    }
    painted
}
