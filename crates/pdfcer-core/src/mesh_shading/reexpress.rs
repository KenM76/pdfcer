//! Re-express a decoded mesh in another colour space as a triangle list,
//! subdividing where the conversion is not linear enough.
//!
//! A mesh interpolates colour linearly in its own space (`MSH13`); a
//! conversion to another space generally does not commute with that. Each
//! triangle is therefore checked at its three edge midpoints and its
//! centroid: where the converted source colour differs from the linear
//! blend of the converted corners by more than the tolerance, the triangle
//! is split into four at its edge midpoints and each piece is checked
//! again.

use super::tessellate::{Tessellation, tessellate_patches};
use super::{DecodedMesh, MAX_TRIANGLES, MeshError, MeshGeometry, MeshTriangle, MeshVertex};

/// Splits per source triangle at most; `4⁸` pieces.
const DEFAULT_MAX_DEPTH: u32 = 8;

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
    mut convert: F,
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
    };
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
            let within = deviation(&dst, &conv) <= self.params.tolerance;
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
