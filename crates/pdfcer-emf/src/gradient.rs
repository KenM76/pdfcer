//! Smooth shadings written as `EMR_GRADIENTFILL` triangles
//! ([`super::EmfOptions::vector_shadings`]).
//!
//! An axial, radial or mesh shading — a native gradient fill, or a shading
//! the recorder harvested as a raster and paired with its source — is
//! tessellated by `pdfcer_render::writer_seam::tessellate*` into Gouraud
//! triangles in device pixels and written as `EMR_GRADIENTFILL` (MS-EMF
//! §2.3.5.12, `ulMode` = `GRADIENT_FILL_TRIANGLE`) under the op's clip, the
//! pattern's fill area and `/BBox`. A shading the tessellator declines keeps
//! its raster and is counted in [`EmfShadings`].

use std::sync::Arc;

use pdfcer_render::writer_seam::{
    Brush, Decline, Gouraud, Op, ShadingSource, tessellate, tessellate_gradient,
};
use tiny_skia::{BlendMode, FillRule, Path, PathBuilder, Pixmap};

use super::{Writer, i32le, rectl, u32le};

/// What [`super::EmfOptions::vector_shadings`] wrote as triangles and what
/// it left as bitmaps. All zero with the switch off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EmfShadings {
    /// Shadings and gradient fills written as `EMR_GRADIENTFILL`.
    pub vector: usize,
    /// Triangles written for them.
    pub triangles: usize,
    /// Left as bitmaps: a constant alpha below 1.
    pub declined_translucent: usize,
    /// Left as bitmaps: more than 200 000 triangles.
    pub declined_too_many: usize,
    /// Left as bitmaps: a ramp with holes, or degenerate geometry.
    pub declined_other: usize,
}

impl EmfShadings {
    /// Shadings left as bitmaps, every reason summed.
    #[must_use]
    pub fn declined(&self) -> usize {
        self.declined_translucent + self.declined_too_many + self.declined_other
    }
}

/// The most triangles one shading may become before it stays a bitmap.
const MAX_TRIANGLES: usize = 200_000;
/// Triangles per `EMR_GRADIENTFILL` record, keeping each record under
/// ~200 KB.
const PER_RECORD: usize = 4096;
/// The least height, in device pixels, over a triangle's longest edge for
/// GDI to fill it. Measured: a diagonal triangle under about 0.3 px fails;
/// one 0.1 px thick along an axis does not, so this errs thick.
const WIDE_PX: f32 = 0.5;
/// `ulMode`: `GRADIENT_FILL_TRIANGLE`.
const GRADIENT_FILL_TRIANGLE: u32 = 2;

/// An extra clip a shading is drawn under, in device pixels.
enum Inner {
    Path(Path, FillRule),
}

impl Writer<'_> {
    /// Write a gradient or harvested-shading fill as triangles. `false`
    /// (nothing written) leaves the op to the raster route.
    pub(super) fn vector_shading(&mut self, op: &Op) -> bool {
        let Op::Fill {
            path,
            brush,
            rule,
            ctm,
            clip,
            bounds,
        } = op
        else {
            return false;
        };
        if brush.blend != BlendMode::SourceOver {
            return false;
        }
        let (got, inner, harvested) = match &brush.brush {
            Brush::Gradient(spec) => {
                let Some(b) = bounds else {
                    return false;
                };
                let region = self.clamp_region([b.left, b.top, b.right, b.bottom]);
                let to_device = ctm.pre_concat(spec.transform);
                let area = path.as_ref().clone().transform(*ctm);
                let inner: Vec<Inner> = area.map(|p| Inner::Path(p, *rule)).into_iter().collect();
                (
                    tessellate_gradient(spec, to_device, region, MAX_TRIANGLES),
                    inner,
                    false,
                )
            }
            Brush::Image { texels, .. } => {
                let Some(src) = self.source_of(texels) else {
                    return false;
                };
                (tessellate(&src, MAX_TRIANGLES), inner_clips(&src), true)
            }
            Brush::Solid { .. } => return false,
        };
        let g = match got {
            Ok(g) => g,
            Err(why) => {
                let s = &mut self.outcome.shadings;
                match why {
                    Decline::Translucent => s.declined_translucent += 1,
                    Decline::TooManyTriangles => s.declined_too_many += 1,
                    _ => s.declined_other += 1,
                }
                return false;
            }
        };
        self.ensure_clip(*clip);
        self.gradient_under(&g, &inner);
        self.outcome.shadings.vector += 1;
        self.outcome.shadings.triangles += g.len();
        if harvested {
            let t = &mut self.outcome.tally;
            t.shadings_rasterised = t.shadings_rasterised.saturating_sub(1);
        }
        true
    }

    fn source_of(&self, texels: &Arc<Pixmap>) -> Option<ShadingSource> {
        self.shadings?
            .iter()
            .find(|(t, _)| Arc::ptr_eq(t, texels))
            .map(|(_, s)| s.clone())
    }

    /// The region a tessellation must cover: `b` cut to the page.
    fn clamp_region(&self, [x0, y0, x1, y1]: [f32; 4]) -> [f32; 4] {
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (self.page_w as f32, self.page_h as f32);
        [x0.max(0.0), y0.max(0.0), x1.min(w), y1.min(h)]
    }

    /// `g` under the clip in force and each of `inner`.
    fn gradient_under(&mut self, g: &Gouraud, inner: &[Inner]) {
        if !inner.is_empty() {
            self.record(0x21, &[]); // SAVEDC
            for Inner::Path(p, rule) in inner {
                self.set_fill_mode(*rule);
                self.path_bracket(p);
                self.record(0x43, &u32le(1)); // SELECTCLIPPATH RGN_AND
            }
        }
        for run in record_runs(g) {
            let body = self.gradient_fill_body(g, &run);
            self.record(0x76, &body); // EMR_GRADIENTFILL
        }
        if !inner.is_empty() {
            self.record(0x22, &i32le(-1)); // RESTOREDC -1
            // RESTOREDC put back the fill mode in force at SAVEDC.
            self.fill_mode = 0;
        }
        self.outcome.ops += 1;
    }

    /// One `EMR_GRADIENTFILL` body after the 8-byte head: `Bounds`,
    /// `nVer`, `nTri`, `ulMode`, the `TriVertex` objects (x, y as i32 logical
    /// units; red, green, blue, alpha as u16 with the 8-bit value in the high
    /// byte), then one `GradientTriangle` (three u32 indices) per triangle.
    /// Triangle mode carries no padding.
    fn gradient_fill_body(&self, g: &Gouraud, tris: &[[u32; 3]]) -> Vec<u8> {
        let mut remap = std::collections::HashMap::<u32, u32>::new();
        let mut verts: Vec<u8> = Vec::new();
        let mut idx: Vec<u8> = Vec::with_capacity(tris.len() * 12);
        let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for tri in tris {
            for &v in tri {
                #[allow(clippy::cast_possible_truncation)]
                let next = remap.len() as u32;
                let i = *remap.entry(v).or_insert_with(|| {
                    // Indices are in range by construction (`Gouraud`).
                    let p = g.points.get(v as usize).copied().unwrap_or_default();
                    let c = g.colours.get(v as usize).copied().unwrap_or_default();
                    let (x, y) = (self.lu(p[0]), self.lu(p[1]));
                    b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                    verts.extend_from_slice(&i32le(x));
                    verts.extend_from_slice(&i32le(y));
                    for ch in c {
                        verts.extend_from_slice(&channel(ch).to_le_bytes());
                    }
                    verts.extend_from_slice(&0u16.to_le_bytes());
                    next
                });
                idx.extend_from_slice(&u32le(i));
            }
        }
        let mut body = Vec::with_capacity(28 + verts.len() + idx.len());
        body.extend_from_slice(&rectl(b[0], b[1], b[2], b[3]));
        #[allow(clippy::cast_possible_truncation)]
        body.extend_from_slice(&u32le(remap.len() as u32));
        #[allow(clippy::cast_possible_truncation)]
        body.extend_from_slice(&u32le((idx.len() / 12) as u32));
        body.extend_from_slice(&u32le(GRADIENT_FILL_TRIANGLE));
        body.extend_from_slice(&verts);
        body.extend_from_slice(&idx);
        body
    }
}

/// `g`'s triangles split into `EMR_GRADIENTFILL` runs, paint order kept.
///
/// GDI's `GradientFill` returns the result of a record's last triangle, and
/// fails on a diagonal sliver; `PlayEnhMetaFile` then returns `FALSE`
/// though every triangle before it painted, and a printer route treats that
/// as a failed page. So each run ends on a [`wide`] triangle: a cut is moved
/// back to one, and the final run, whose end is fixed, has its last such
/// triangle moved to the end when nothing after it overlaps it.
fn record_runs(g: &Gouraud) -> Vec<Vec<[u32; 3]>> {
    let t = &g.triangles;
    let last_wide = t.iter().rposition(|&tri| wide(g, tri));
    let mut out = Vec::new();
    let mut start = 0;
    while start < t.len() {
        let cap = (start + PER_RECORD).min(t.len());
        let end = if cap == t.len() {
            cap
        } else {
            // Leave the final run a wide triangle to end on.
            (start + PER_RECORD / 2 + 1..=cap)
                .rev()
                .find(|&e| {
                    t.get(e - 1).is_some_and(|&tri| wide(g, tri))
                        && last_wide.is_none_or(|w| w >= e)
                })
                .unwrap_or(cap)
        };
        let mut run = t.get(start..end).unwrap_or(&[]).to_vec();
        end_on_a_wide_triangle(g, &mut run);
        out.push(run);
        start = end;
    }
    out
}

/// Move `run`'s last [`wide`] triangle to its end, if it is not there
/// already and no triangle after it overlaps it (so the move changes no
/// pixel).
fn end_on_a_wide_triangle(g: &Gouraud, run: &mut Vec<[u32; 3]>) {
    let Some(&last) = run.last() else {
        return;
    };
    if wide(g, last) {
        return;
    }
    let Some(k) = run.iter().rposition(|&tri| wide(g, tri)) else {
        return;
    };
    let (Some(&tri), Some(after)) = (run.get(k), run.get(k + 1..)) else {
        return;
    };
    if after.iter().all(|&o| !overlaps(g, tri, o)) {
        run.remove(k);
        run.push(tri);
    }
}

fn corners(g: &Gouraud, tri: [u32; 3]) -> [[f32; 2]; 3] {
    tri.map(|i| g.points.get(i as usize).copied().unwrap_or_default())
}

/// Whether `tri` is wide enough for GDI to fill: its height over its
/// longest edge reaches [`WIDE_PX`].
fn wide(g: &Gouraud, tri: [u32; 3]) -> bool {
    let [a, b, c] = corners(g, tri);
    let area2 = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    let len = |p: [f32; 2], q: [f32; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
    let longest = len(a, b).max(len(b, c)).max(len(c, a));
    longest > 0.0 && area2.abs() / longest >= WIDE_PX
}

/// Whether two triangles' interiors meet: no edge normal of either
/// separates them (a shared edge does not count).
fn overlaps(g: &Gouraud, s: [u32; 3], t: [u32; 3]) -> bool {
    const EPS: f32 = 1e-3;
    let (s, t) = (corners(g, s), corners(g, t));
    let span = |tri: &[[f32; 2]; 3], n: [f32; 2]| {
        let d = tri.map(|p| p[0] * n[0] + p[1] * n[1]);
        (d[0].min(d[1]).min(d[2]), d[0].max(d[1]).max(d[2]))
    };
    for &[p0, p1, p2] in [&s, &t] {
        for (p, q) in [(p0, p1), (p1, p2), (p2, p0)] {
            let n = [q[1] - p[1], p[0] - q[0]];
            let (a, b) = (span(&s, n), span(&t, n));
            let tol = EPS * n[0].hypot(n[1]);
            if a.1 <= b.0 + tol || b.1 <= a.0 + tol {
                return false;
            }
        }
    }
    true
}

/// A colour channel `0..=1` as a `TriVertex` component: the 8-bit value in
/// the high byte, so a vertex lands on exactly the byte the raster paints.
fn channel(v: f32) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (v.clamp(0.0, 1.0) * 255.0).round() as u16;
    byte << 8
}

/// A harvested shading's own clips: the pattern's fill area and `/BBox`.
fn inner_clips(src: &ShadingSource) -> Vec<Inner> {
    let mut out = Vec::new();
    if let Some((area, rule)) = &src.area {
        out.push(Inner::Path(area.as_ref().clone(), *rule));
    }
    if let Some(q) = src.bbox_device() {
        let mut pb = PathBuilder::new();
        pb.move_to(q[0][0], q[0][1]);
        for p in &q[1..] {
            pb.line_to(p[0], p[1]);
        }
        pb.close();
        if let Some(p) = pb.finish() {
            out.push(Inner::Path(p, FillRule::Winding));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A 10 px triangle, then a diagonal sliver 0.1 px wide beside it.
    fn wide_then_sliver() -> Gouraud {
        Gouraud {
            points: vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [0.0, 10.0],
                [20.0, 20.0],
                [30.0, 30.0],
                [30.0, 30.14],
            ],
            colours: vec![[0.0; 3]; 6],
            triangles: vec![[0, 1, 2], [3, 4, 5]],
        }
    }

    #[test]
    fn a_sliver_is_not_wide() {
        let g = wide_then_sliver();
        assert!(wide(&g, g.triangles[0]));
        assert!(!wide(&g, g.triangles[1]));
    }

    #[test]
    fn a_run_ending_on_a_sliver_ends_on_a_wide_triangle_instead() {
        let g = wide_then_sliver();
        assert_eq!(record_runs(&g), vec![vec![[3, 4, 5], [0, 1, 2]]]);
    }

    #[test]
    fn an_overlapped_wide_triangle_keeps_its_place() {
        let mut g = wide_then_sliver();
        g.points[3..].copy_from_slice(&[[1.0, 1.0], [5.0, 3.0], [5.0, 3.1]]);
        assert_eq!(record_runs(&g), vec![g.triangles.clone()]);
    }

    #[test]
    fn a_long_run_is_cut_after_a_wide_triangle() {
        let mut g = wide_then_sliver();
        g.triangles = [[0, 1, 2], [3, 4, 5]].repeat(PER_RECORD);
        let runs = record_runs(&g);
        assert!(runs.iter().all(|r| r.len() <= PER_RECORD));
        assert!(runs.iter().all(|r| wide(&g, *r.last().unwrap())));
        assert_eq!(runs.iter().map(Vec::len).sum::<usize>(), 2 * PER_RECORD);
    }
}
