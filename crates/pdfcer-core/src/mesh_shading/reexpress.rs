//! Re-express a decoded mesh in another colour space as a triangle list,
//! subdividing where the conversion is not linear enough.
//!
//! A mesh interpolates colour linearly in its own space (`MSH13`); a
//! conversion to another space generally does not commute with that. Each
//! triangle is therefore checked at its three edge midpoints and its
//! centroid: where the converted source colour differs from the linear
//! blend of the converted corners by more than the tolerance, the triangle
//! is split into four at its edge midpoints and each piece is checked
//! again. A parametric mesh's function can change between those probes (a
//! type 3 stitch, a finely sampled type 0), so its conversion is also
//! sampled on a [`GRID`]-step grid over the mesh's `t` range, and a
//! triangle whose `t` span holds a grid value off the line through its
//! extreme corners is split too. A feature narrower than one grid step can
//! still be missed. [`reexpress_mesh_with_breaks`] replaces the grid with
//! the function's own break list (`PdfFunction::affine_breaks`), which
//! misses nothing for a conversion affine in the function's outputs.

use super::tessellate::{Tessellation, tessellate_patches};
use super::{DecodedMesh, MAX_TRIANGLES, MeshError, MeshGeometry, MeshTriangle, MeshVertex};

/// Splits per source triangle at most; `4⁸` pieces.
const DEFAULT_MAX_DEPTH: u32 = 8;

/// Steps of a parametric mesh's function grid over its `t` range.
const GRID: usize = 256;

/// Tolerance and limits for [`reexpress_mesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reexpress {
    tolerance: f64,
    max_depth: u32,
    tessellation: Tessellation,
    cap: usize,
}

impl Default for Reexpress {
    /// One 8-bit level (`1/255` of a unit component range), eight levels of
    /// splitting, the default [`Tessellation`] and the module triangle cap.
    fn default() -> Self {
        Self {
            tolerance: 1.0 / 255.0,
            max_depth: DEFAULT_MAX_DEPTH,
            tessellation: Tessellation::default(),
            cap: MAX_TRIANGLES,
        }
    }
}

impl Reexpress {
    /// Largest allowed deviation per destination component, in its own
    /// units. A non-finite or negative value reads as the default.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        if tolerance.is_finite() && tolerance >= 0.0 {
            self.tolerance = tolerance;
        }
        self
    }

    /// Most times one source triangle is split (each split makes four);
    /// clamped to 12.
    #[must_use]
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth.min(12);
        self
    }

    /// How type 6/7 patches are cut before conversion.
    #[must_use]
    pub const fn with_tessellation(mut self, tessellation: Tessellation) -> Self {
        self.tessellation = tessellation;
        self
    }

    /// Decline past `cap` output triangles (clamped to the module cap).
    #[must_use]
    pub fn with_triangle_cap(mut self, cap: usize) -> Self {
        self.cap = cap.min(MAX_TRIANGLES);
        self
    }
}

/// What [`reexpress_mesh`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ReexpressReport {
    /// Triangles before conversion (after patch tessellation).
    pub source_triangles: usize,
    /// Patches tessellated (0 for types 4/5).
    pub patches_tessellated: usize,
    /// Splits made: each turned one triangle into four.
    pub subdivisions: usize,
    /// Triangles output.
    pub triangles: usize,
    /// Pieces still past the tolerance at the depth limit, kept as they
    /// are. Zero means every piece is within the tolerance.
    pub over_tolerance: usize,
}

/// Convert every vertex of `mesh` through `convert`, subdividing until
/// each triangle is within the tolerance, and return the triangles in the
/// destination space (paint order kept) for [`write_free_form_mesh`].
///
/// `convert` receives a vertex's colour fields as authored — for a
/// parametric mesh that is `[t]`, so it must apply the shading's
/// `/Function` itself — and returns `out_components` values, or `None` to
/// decline.
///
/// [`write_free_form_mesh`]: super::write_free_form_mesh
///
/// # Errors
///
/// [`MeshError::ConversionDeclined`] (a `None` or non-finite value),
/// [`MeshError::ConversionShape`] (wrong length),
/// [`MeshError::BadComponentCount`] (`out_components` outside 1–32) and
/// [`MeshError::TooManyTriangles`] past the cap, with nothing returned.
pub fn reexpress_mesh<F>(
    mesh: &DecodedMesh,
    out_components: usize,
    convert: F,
    params: &Reexpress,
) -> Result<(Vec<MeshTriangle>, ReexpressReport), MeshError>
where
    F: FnMut(&[f64]) -> Option<Vec<f64>>,
{
    run(mesh, out_components, convert, None, params)
}

/// [`reexpress_mesh`] for a parametric mesh whose function's breaks are
/// known: `breaks` lists the `t` between which every output of the
/// shading's `/Function` is affine, and, where `convert` clamps to the
/// colour space's component range, its clamped outputs too
/// (`PdfFunction::affine_breaks_clamped`).
///
/// Instead of the grid, each triangle is checked at every break inside its
/// closed corner `t` range, at the floats just either side of each, and
/// midway between neighbouring breaks, against the line through its
/// lowest and highest corners' converted values, to half the tolerance.
/// For a conversion affine in the function's outputs that bounds the error
/// everywhere in the triangle, so a band of any width is found; a
/// conversion that is itself curved is covered by the midpoint probes as in
/// [`reexpress_mesh`]. A jump larger than the tolerance never passes and
/// ends in [`ReexpressReport::over_tolerance`]. `breaks` is ignored for a
/// mesh that is not parametric.
///
/// # Errors
///
/// As [`reexpress_mesh`].
pub fn reexpress_mesh_with_breaks<F>(
    mesh: &DecodedMesh,
    out_components: usize,
    convert: F,
    breaks: &[f64],
    params: &Reexpress,
) -> Result<(Vec<MeshTriangle>, ReexpressReport), MeshError>
where
    F: FnMut(&[f64]) -> Option<Vec<f64>>,
{
    let mut sorted = breaks.to_vec();
    sorted.retain(|t| t.is_finite());
    sorted.sort_by(f64::total_cmp);
    run(mesh, out_components, convert, Some(&sorted), params)
}

fn run<F>(
    mesh: &DecodedMesh,
    out_components: usize,
    mut convert: F,
    breaks: Option<&[f64]>,
    params: &Reexpress,
) -> Result<(Vec<MeshTriangle>, ReexpressReport), MeshError>
where
    F: FnMut(&[f64]) -> Option<Vec<f64>>,
{
    if out_components == 0 || out_components > super::decode::MAX_COMPONENTS {
        return Err(MeshError::BadComponentCount(out_components));
    }
    let mut report = ReexpressReport::default();
    let tessellated;
    let source: &[MeshTriangle] = match &mesh.geometry {
        MeshGeometry::Triangles(t) => t,
        MeshGeometry::Patches(p) => {
            report.patches_tessellated = p.len();
            tessellated = tessellate_patches(p, params.tessellation, params.cap)?;
            &tessellated
        }
    };
    report.source_triangles = source.len();
    let mut cx = Splitter {
        convert: &mut convert,
        out_components,
        params,
        out: Vec::new(),
        report,
        check: Check::Probes,
    };
    if mesh.parametric {
        cx.check = match breaks {
            Some(b) => Check::Breaks(b),
            None => Check::Grid(cx.grid(source)?),
        };
    }
    for tri in source {
        let converted = [
            cx.vertex(&tri[0])?,
            cx.vertex(&tri[1])?,
            cx.vertex(&tri[2])?,
        ];
        cx.fit(tri.clone(), converted)?;
    }
    cx.report.triangles = cx.out.len();
    Ok((cx.out, cx.report))
}

struct Splitter<'a, F> {
    convert: &'a mut F,
    out_components: usize,
    params: &'a Reexpress,
    out: Vec<MeshTriangle>,
    report: ReexpressReport,
    check: Check<'a>,
}

/// What a parametric mesh is checked with beyond the midpoint probes.
enum Check<'a> {
    /// Nothing more: not parametric.
    Probes,
    /// [`GRID`] samples of the function.
    Grid(Grid),
    /// The function's sorted breaks.
    Breaks(&'a [f64]),
}

/// `convert` on [`GRID`] steps over a parametric mesh's corner `t` range,
/// `t` increasing; empty when the range is a single value.
struct Grid {
    samples: Vec<(f64, Vec<f64>)>,
}

/// A parametric vertex's `t`.
fn t_of(v: &MeshVertex) -> f64 {
    v.color.first().copied().unwrap_or(0.0)
}

/// A corner's `t` and converted vertex.
type Corner<'a> = (f64, &'a MeshVertex);

/// `src`'s lowest- and highest-`t` corners: each `t` with the matching
/// corner of `dst`.
fn extremes<'a>(src: &MeshTriangle, dst: &'a MeshTriangle) -> Option<(Corner<'a>, Corner<'a>)> {
    let ends = |pick: fn(f64, f64) -> bool| {
        src.iter()
            .map(t_of)
            .zip(dst)
            .reduce(|best, v| if pick(v.0, best.0) { v } else { best })
    };
    Some((ends(|a, b| a < b)?, ends(|a, b| a > b)?))
}

impl Grid {
    /// Most a grid value strictly between `src`'s lowest and highest corner
    /// `t` is from the line through those corners' converted values in
    /// `dst`.
    fn off(&self, src: &MeshTriangle, dst: &MeshTriangle) -> f64 {
        let Some(((t_lo, f_lo), (t_hi, f_hi))) = extremes(src, dst) else {
            return 0.0;
        };
        if t_hi <= t_lo {
            return 0.0;
        }
        let a = self.samples.partition_point(|(s, _)| *s <= t_lo);
        let b = self.samples.partition_point(|(s, _)| *s < t_hi);
        let mut off = 0.0_f64;
        for (s, values) in self.samples.get(a..b).unwrap_or_default() {
            let w = (s - t_lo) / (t_hi - t_lo);
            for ((x, y), v) in f_lo.color.iter().zip(&f_hi.color).zip(values) {
                off = off.max((x + (y - x) * w - v).abs());
            }
        }
        off
    }
}

impl<F> Splitter<'_, F>
where
    F: FnMut(&[f64]) -> Option<Vec<f64>>,
{
    fn color(&mut self, source: &[f64]) -> Result<Vec<f64>, MeshError> {
        let c = (self.convert)(source).ok_or(MeshError::ConversionDeclined)?;
        if c.len() != self.out_components {
            return Err(MeshError::ConversionShape {
                expected: self.out_components,
                found: c.len(),
            });
        }
        if !c.iter().all(|v| v.is_finite()) {
            return Err(MeshError::ConversionDeclined);
        }
        Ok(c)
    }

    /// The function grid over `source`'s corner `t` range.
    fn grid(&mut self, source: &[MeshTriangle]) -> Result<Grid, MeshError> {
        let (lo, hi) = source
            .iter()
            .flatten()
            .map(t_of)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| {
                (lo.min(t), hi.max(t))
            });
        let mut samples = Vec::new();
        if lo < hi {
            for k in 0..=GRID {
                #[allow(clippy::cast_precision_loss)] // k <= GRID = 256
                let t = lo + (hi - lo) * (k as f64 / GRID as f64);
                samples.push((t, self.color(&[t])?));
            }
        }
        Ok(Grid { samples })
    }

    /// Most `convert` at the `breaks` inside `src`'s closed corner `t`
    /// range, just either side of each, and midway between neighbours, is
    /// from the line through the lowest and highest corners' values in
    /// `dst`.
    fn break_off(
        &mut self,
        breaks: &[f64],
        src: &MeshTriangle,
        dst: &MeshTriangle,
    ) -> Result<f64, MeshError> {
        let Some(((t_lo, f_lo), (t_hi, f_hi))) = extremes(src, dst) else {
            return Ok(0.0);
        };
        if t_hi <= t_lo {
            return Ok(0.0);
        }
        let a = breaks.partition_point(|b| *b < t_lo);
        let b = breaks.partition_point(|b| *b <= t_hi);
        let inside = breaks.get(a..b).unwrap_or_default();
        let mut probes = Vec::with_capacity(inside.len() * 4);
        for (k, &t) in inside.iter().enumerate() {
            probes.extend([t, t.next_down().max(t_lo), t.next_up().min(t_hi)]);
            if let Some(&next) = inside.get(k + 1) {
                probes.push(t + (next - t) / 2.0);
            }
        }
        let mut off = 0.0_f64;
        for t in probes {
            let v = self.color(&[t])?;
            let w = (t - t_lo) / (t_hi - t_lo);
            for ((x, y), v) in f_lo.color.iter().zip(&f_hi.color).zip(&v) {
                off = off.max((x + (y - x) * w - v).abs());
            }
        }
        Ok(off)
    }

    fn vertex(&mut self, v: &MeshVertex) -> Result<MeshVertex, MeshError> {
        Ok(MeshVertex {
            xy: v.xy,
            color: self.color(&v.color)?,
        })
    }

    /// Emit `src` (source colours) / `dst` (its converted corners), split
    /// until within tolerance or at the depth limit. A work stack, not
    /// recursion; pieces leave in a fixed order, which is enough because
    /// the pieces of one triangle do not overlap.
    fn fit(&mut self, src: MeshTriangle, dst: MeshTriangle) -> Result<(), MeshError> {
        let mut work = vec![(src, dst, 0u32)];
        while let Some((src, dst, depth)) = work.pop() {
            let mids = midpoints(&src);
            let mut conv = Vec::with_capacity(4);
            for m in &mids {
                conv.push(self.color(&m.color)?);
            }
            let mut dev = deviation(&dst, &conv);
            match &self.check {
                Check::Probes => {}
                Check::Grid(grid) => dev = dev.max(grid.off(&src, &dst)),
                Check::Breaks(b) => {
                    let b: &[f64] = b;
                    dev = dev.max(2.0 * self.break_off(b, &src, &dst)?);
                }
            }
            let within = dev <= self.params.tolerance;
            if within || depth >= self.params.max_depth {
                if !within {
                    self.report.over_tolerance += 1;
                }
                if self.out.len() >= self.params.cap {
                    return Err(MeshError::TooManyTriangles {
                        cap: self.params.cap,
                    });
                }
                self.out.push(dst);
                continue;
            }
            if self.out.len() + work.len() + 4 > self.params.cap {
                return Err(MeshError::TooManyTriangles {
                    cap: self.params.cap,
                });
            }
            self.report.subdivisions += 1;
            let [ab, bc, ca, _] = mids;
            let [cab, cbc, cca, _] = <[Vec<f64>; 4]>::try_from(conv).unwrap_or_default();
            let (dab, dbc, dca) = (
                MeshVertex {
                    xy: ab.xy,
                    color: cab,
                },
                MeshVertex {
                    xy: bc.xy,
                    color: cbc,
                },
                MeshVertex {
                    xy: ca.xy,
                    color: cca,
                },
            );
            let [a, b, c] = src;
            let [da, db, dc] = dst;
            let next = depth + 1;
            work.push((
                [ab.clone(), bc.clone(), ca.clone()],
                [dab.clone(), dbc.clone(), dca.clone()],
                next,
            ));
            work.push((
                [ca.clone(), bc.clone(), c],
                [dca.clone(), dbc.clone(), dc],
                next,
            ));
            work.push(([ab.clone(), b, bc], [dab.clone(), db, dbc], next));
            work.push(([a, ab, ca], [da, dab, dca], next));
        }
        Ok(())
    }
}

/// Edge midpoints `ab`, `bc`, `ca` and the centroid, in source colours.
fn midpoints(t: &MeshTriangle) -> [MeshVertex; 4] {
    let [a, b, c] = t;
    let mix = |vs: &[&MeshVertex]| {
        #[allow(clippy::cast_precision_loss)] // 2 or 3
        let n = vs.len() as f64;
        let xy = [
            vs.iter().map(|v| v.xy[0]).sum::<f64>() / n,
            vs.iter().map(|v| v.xy[1]).sum::<f64>() / n,
        ];
        let color = (0..a.color.len())
            .map(|k| {
                vs.iter()
                    .map(|v| v.color.get(k).copied().unwrap_or(0.0))
                    .sum::<f64>()
                    / n
            })
            .collect();
        MeshVertex { xy, color }
    };
    [mix(&[a, b]), mix(&[b, c]), mix(&[c, a]), mix(&[a, b, c])]
}

/// Largest gap between a converted sample and the linear blend of the
/// converted corners at the same point.
fn deviation(dst: &MeshTriangle, conv: &[Vec<f64>]) -> f64 {
    let [a, b, c] = dst;
    let linear = midpoints(&[a.clone(), b.clone(), c.clone()]);
    linear
        .iter()
        .zip(conv)
        .flat_map(|(l, s)| l.color.iter().zip(s).map(|(x, y)| (x - y).abs()))
        .fold(0.0, f64::max)
}
