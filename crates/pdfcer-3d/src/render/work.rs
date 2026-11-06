//! The raster work ceiling. Before any pass scans a triangle, the frame is
//! charged every centre each pass's triangles' clipped boxes span (the
//! image, then every shadow map and lamp cube), so a model whose triangles
//! would take too long to draw is refused at the cost of projecting it,
//! not of drawing it.

use crate::TriangleMesh;
use crate::camera::Camera;
use crate::render::raster::View;
use crate::render::raster::{at, edge, view_point};
use crate::render::{RenderError, at3, corners};
use crate::vec3::{dot, span, sub, unit_cross};

/// Ceiling on the pixel- and texel-centre tests drawing one frame may make:
/// every triangle's box, clipped to its target, summed over the image and
/// every shadow map. Past it the render fails with
/// [`RenderError::TooMuchWork`] before anything is scanned.
pub const MAX_RENDER_PIXEL_TESTS: u64 = 1 << 37;

/// The tests charged so far in one frame.
#[derive(Debug, Default)]
pub(crate) struct Work {
    spent: u64,
}

impl Work {
    /// Adds `tests`; [`RenderError::TooMuchWork`] once the total passes
    /// [`MAX_RENDER_PIXEL_TESTS`].
    pub(crate) fn charge(&mut self, tests: u64) -> Result<(), RenderError> {
        self.spent = self.spent.saturating_add(tests);
        if self.spent > MAX_RENDER_PIXEL_TESTS {
            return Err(RenderError::TooMuchWork {
                limit: MAX_RENDER_PIXEL_TESTS,
            });
        }
        Ok(())
    }
}

/// The centres a fill of screen triangle `t` (x, y, anything) tests in a
/// `width` × `height` target: its box, floor to ceil, clipped to the
/// target, as the fills scan it; 0 for one they skip (an area that is not
/// finite or below 1e-12).
pub(crate) fn box_tests(t: [[f64; 3]; 3], width: u32, height: u32) -> u64 {
    let [a, b, c] = t;
    let area = edge(a, b, c);
    if !area.is_finite() || area.abs() < 1e-12 {
        return 0;
    }
    let span = |i: usize, limit: u32| {
        let lo = t.iter().map(|p| at(*p, i)).fold(f64::INFINITY, f64::min);
        let hi = t
            .iter()
            .map(|p| at(*p, i))
            .fold(f64::NEG_INFINITY, f64::max);
        let (lo, hi) = (lo.floor().max(0.0), hi.ceil().min(f64::from(limit) - 1.0));
        if lo <= hi { (hi - lo) as u64 + 1 } else { 0 }
    };
    span(0, width).saturating_mul(span(1, height))
}

/// The triangles a fan from the first of `points` makes, as the fills cut
/// a near-clipped polygon.
pub(crate) fn fan<T: Copy>(points: &[T]) -> impl Iterator<Item = [T; 3]> + '_ {
    let (first, rest) = points
        .split_first()
        .map_or((None, &[][..]), |(a, r)| (Some(*a), r));
    rest.windows(2).filter_map(move |pair| match (first, pair) {
        (Some(a), [b, c]) => Some([a, *b, *c]),
        _ => None,
    })
}

/// Charges `work` the centres the image fills test for `drawn` (mesh,
/// triangle), seen from `camera` (`basis` right, up, forward; projected by
/// `view`) in a `width` × `height` image: each triangle near-clipped,
/// projected and fanned as the fills cut it, every fan triangle's
/// [`box_tests`].
pub(crate) fn charge_frame<'m>(
    work: &mut Work,
    (camera, basis, view): (&Camera, &[[f64; 3]; 3], &View),
    drawn: impl Iterator<Item = (&'m TriangleMesh, usize)>,
    (width, height): (u32, u32),
) -> Result<(), RenderError> {
    let [right, up, forward] = *basis;
    for (mesh, t) in drawn {
        let Some(world) = mesh.triangles.get(t).and_then(|tri| corners(mesh, tri)) else {
            continue;
        };
        let edges = [1, 2].map(|k| span(at3(&world, 0), at3(&world, k)));
        if unit_cross(edges[0], edges[1]).is_none() {
            continue;
        }
        let points = [0, 1, 2].map(|k| {
            let d = sub(at3(&world, k), camera.eye);
            view_point(dot(d, right), dot(d, up), dot(d, forward))
        });
        let screen: Vec<[f64; 3]> = view
            .clip(points)
            .into_iter()
            .map(|v| view.project(v, width, height).at)
            .collect();
        for t in fan(&screen) {
            work.charge(box_tests(t, width, height))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_is_clipped_to_the_target() {
        let t = [[-5.0, -5.0, 0.0], [20.0, 0.5, 0.0], [0.5, 30.0, 0.0]];
        assert_eq!(box_tests(t, 10, 8), 80);
        assert_eq!(box_tests([[1.0, 1.0, 0.0]; 3], 10, 8), 0);
        let off = [[20.0, 20.0, 0.0], [30.0, 20.0, 0.0], [20.0, 30.0, 0.0]];
        assert_eq!(box_tests(off, 10, 8), 0);
    }

    #[test]
    fn the_ceiling_is_charged_cumulatively() {
        let mut work = Work::default();
        assert!(work.charge(MAX_RENDER_PIXEL_TESTS).is_ok());
        assert_eq!(
            work.charge(1),
            Err(RenderError::TooMuchWork {
                limit: MAX_RENDER_PIXEL_TESTS
            })
        );
    }
}
