//! A mesh shading (types 4–7, ISO 32000-2 §8.7.4.5.5–8) as flat-colour
//! pieces, for a writer that composites it with other paint as vector
//! regions, as [`super::band`] does for axial and radial shadings.
//!
//! Each of the raster's own triangles is split into four at its edge
//! midpoints until the colour across it changes by at most
//! [`BAND_STEP`], or its longest edge is under [`MIN_PIECE_PX`]. A piece
//! takes the colour at its centroid, within two thirds of a step of the
//! smooth shade wherever the split ran to colour, rounded to 8 bits as a
//! device paints it. A parametric mesh
//! is split in `t` and resolved after, as §8.7.4.5.5 interpolates.
//! When no two pieces overlap ([`super::mesh_cover`]), paint order does
//! not matter and the whole mesh is grouped by colour into bands, flagged
//! not overlapping. Otherwise only the pieces of one mesh triangle, which
//! partition it, are grouped, and a band continues across triangles
//! while the colour holds. `max` bounds the bands, the regions a writer
//! emits.

use tiny_skia::Transform;

use super::bands::{BAND_STEP, Bands, FlatBand};
use super::{Decline, mesh};
use crate::mesh::{Mesh, Shade};
use crate::shading::{ColorRamp, SpotTints};

/// A piece is not split below this longest edge, in device pixels.
pub const MIN_PIECE_PX: f32 = 0.5;

/// The deepest one mesh triangle is split (4ⁿ pieces).
const MAX_DEPTH: u32 = 12;

/// `mesh` as flat bands in device space, at most `max` of them. `spots`
/// is the number of spot colorants the source plates; with none, a band
/// carries no authored tints.
pub(super) fn mesh_bands(
    mesh: &Mesh,
    ramp: Option<&ColorRamp>,
    to_device: Transform,
    spots: usize,
    max: usize,
) -> Result<Bands, Decline> {
    if ramp.is_some_and(|r| !r.is_complete()) {
        return Err(Decline::IncompleteRamp);
    }
    let mut cut = Cutter {
        ramp,
        spots,
        tri: 0,
        pieces: Vec::new(),
    };
    mesh::each_triangle(mesh, to_device, |p, s| {
        cut.piece(p, s, 0)?;
        cut.tri += 1;
        Ok(())
    })?;
    let mut pieces = cut.pieces;
    let corners: Vec<_> = pieces.iter().map(|q| q.corners).collect();
    let overlapping = super::mesh_cover::overlaps(&corners);
    if !overlapping {
        pieces.iter_mut().for_each(|q| q.tri = 0);
    }
    // Stable, so pieces that may overlap keep their order within a colour.
    pieces.sort_by_key(|q| (q.tri, q.key));
    let mut bands: Vec<FlatBand> = Vec::new();
    for q in pieces {
        let quad = [q.corners[0], q.corners[1], q.corners[2], q.corners[2]];
        match bands.last_mut() {
            Some(prev) if q.key.is_some() && key_of(prev) == q.key => prev.quads.push(quad),
            _ => bands.push(FlatBand {
                quads: vec![quad],
                ..q.band
            }),
        }
        if bands.len() > max {
            return Err(Decline::TooManyTriangles);
        }
    }
    Ok(Bands { bands, overlapping })
}

/// The most pieces one mesh is cut into.
const MAX_PIECES: usize = 1 << 22;

/// The colour key two pieces must share to merge into one band.
type Key = ([u8; 3], Option<[u8; 4]>);

/// One flat piece: the mesh triangle it came from, its merge key (`None`
/// with authored tints, which are finer than the key) and its colour.
struct Piece {
    tri: usize,
    key: Option<Key>,
    corners: [[f32; 2]; 3],
    band: FlatBand,
}

struct Cutter<'a> {
    ramp: Option<&'a ColorRamp>,
    spots: usize,
    tri: usize,
    pieces: Vec<Piece>,
}

impl Cutter<'_> {
    fn piece(&mut self, p: [[f32; 2]; 3], s: [Shade; 3], depth: u32) -> Result<(), Decline> {
        let mid = |(u, a): ([f32; 2], Shade), (v, b): ([f32; 2], Shade)| {
            ([(u[0] + v[0]) / 2.0, (u[1] + v[1]) / 2.0], a.lerp(b, 0.5))
        };
        let [pa, pb, pc] = [(p[0], s[0]), (p[1], s[1]), (p[2], s[2])];
        let (ab, bc, ca) = (mid(pa, pb), mid(pb, pc), mid(pc, pa));
        let probe = [s[0], s[1], s[2], ab.1, bc.1, ca.1];
        let Some(spread) = self.spread(&probe) else {
            // A ramp hole leaves the pixels unpainted, as the raster does.
            return Ok(());
        };
        if spread <= BAND_STEP || depth >= MAX_DEPTH || longest(p) <= MIN_PIECE_PX {
            return self.emit(p, s);
        }
        let d = depth + 1;
        self.piece([p[0], ab.0, ca.0], [s[0], ab.1, ca.1], d)?;
        self.piece([ab.0, p[1], bc.0], [ab.1, s[1], bc.1], d)?;
        self.piece([ca.0, bc.0, p[2]], [ca.1, bc.1, s[2]], d)?;
        self.piece([ab.0, bc.0, ca.0], [ab.1, bc.1, ca.1], d)
    }

    /// The largest change in any sRGB or ink channel across `shades`;
    /// `None` at a ramp hole.
    fn spread(&self, shades: &[Shade]) -> Option<f32> {
        let mut lo = [f32::MAX; 7];
        let mut hi = [f32::MIN; 7];
        for sh in shades {
            let c = sh.resolve(self.ramp)?;
            let k = sh.resolve_cmyk(self.ramp).unwrap_or([0.0; 4]);
            let values = [c.r, c.g, c.b, k[0], k[1], k[2], k[3]];
            for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(values) {
                *l = l.min(v);
                *h = h.max(v);
            }
        }
        Some(lo.iter().zip(hi).map(|(l, h)| h - l).fold(0.0, f32::max))
    }

    fn emit(&mut self, p: [[f32; 2]; 3], s: [Shade; 3]) -> Result<(), Decline> {
        if self.pieces.len() >= MAX_PIECES {
            return Err(Decline::TooManyTriangles);
        }
        let centre = s[0].lerp(s[1], 0.5).lerp(s[2], 1.0 / 3.0);
        let Some(c) = centre.resolve(self.ramp) else {
            return Ok(());
        };
        let band = FlatBand {
            quads: Vec::new(),
            rgb: [c.r, c.g, c.b].map(on_grid),
            cmyk: centre.resolve_cmyk(self.ramp).map(|k| k.map(on_grid)),
            authored: (self.spots > 0)
                .then(|| centre.resolve_authored(self.ramp))
                .flatten()
                .map(|(process, tints)| SpotTints {
                    process,
                    spots: tints.iter().take(self.spots).copied().collect(),
                }),
        };
        self.pieces.push(Piece {
            tri: self.tri,
            key: key_of(&band),
            corners: p,
            band,
        });
        Ok(())
    }
}

/// The merge key of `band`; `None` when it carries authored tints.
fn key_of(band: &FlatBand) -> Option<Key> {
    band.authored
        .is_none()
        .then(|| (band.rgb.map(level), band.cmyk.map(|k| k.map(level))))
}

/// `v` rounded to the 8-bit grid.
fn on_grid(v: f32) -> f32 {
    f32::from(level(v)) / 255.0
}

/// `v` on the 8-bit grid.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255 first
fn level(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The longest edge of `p`.
fn longest(p: [[f32; 2]; 3]) -> f32 {
    let d = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    d(p[0], p[1]).max(d(p[1], p[2])).max(d(p[2], p[0]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::unreachable)]
mod tests {
    use super::*;
    use crate::gstate::Rgb;
    use crate::mesh::{MeshColorants, MeshData, Triangle};

    fn mesh(shade: [Shade; 3]) -> Mesh {
        Mesh {
            shading_type: 4,
            data: MeshData::Triangles(vec![Triangle {
                xy: [[0.0, 0.0], [255.0, 0.0], [0.0, 255.0]],
                shade,
            }]),
            records: 3,
            truncated: false,
            rows_inferred: None,
            colorants: MeshColorants::None,
        }
    }

    fn rgb(r: f32) -> Shade {
        Shade::Rgb(Rgb { r, g: 0.0, b: 0.0 })
    }

    #[test]
    fn every_piece_is_within_a_rounded_two_thirds_of_a_level_of_the_smooth_shade() {
        // Red is x / 255: one level per pixel across the triangle.
        let m = mesh([rgb(0.0), rgb(1.0), rgb(0.0)]);
        let b = mesh_bands(&m, None, Transform::identity(), 0, 1 << 20).unwrap();
        assert!(!b.overlapping);
        let pieces = b.quads();
        assert!(pieces > 255, "{pieces}");
        for band in &b.bands {
            assert!(band.cmyk.is_none() && band.authored.is_none());
            for q in &band.quads {
                assert_eq!(q[2], q[3]);
                for p in &q[..3] {
                    let err = (band.rgb[0] - p[0] / 255.0).abs() * 255.0;
                    assert!(err <= 0.5 + 0.67, "{p:?}: {err} levels off");
                }
            }
        }
        // Nothing overlaps, so there is one band per 8-bit level.
        assert!(b.bands.len() <= 256, "{} bands", b.bands.len());
    }

    #[test]
    fn a_fold_keeps_its_paint_order() {
        let mut m = mesh([rgb(0.0); 3]);
        let MeshData::Triangles(tris) = &mut m.data else {
            unreachable!()
        };
        tris.push(Triangle {
            xy: [[10.0, 10.0], [100.0, 10.0], [10.0, 100.0]],
            shade: [rgb(1.0); 3],
        });
        tris.push(Triangle {
            xy: [[20.0, 20.0], [60.0, 20.0], [20.0, 60.0]],
            shade: [rgb(0.0); 3],
        });
        let b = mesh_bands(&m, None, Transform::identity(), 0, 16).unwrap();
        assert!(b.overlapping);
        let reds: Vec<f32> = b.bands.iter().map(|b| b.rgb[0]).collect();
        assert_eq!(reds, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_flat_triangle_is_one_piece() {
        let m = mesh([rgb(0.5); 3]);
        let b = mesh_bands(&m, None, Transform::identity(), 0, 16).unwrap();
        assert_eq!(b.bands.len(), 1);
        assert_eq!(b.quads(), 1);
        assert!((b.bands[0].rgb[0] - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn an_ink_mesh_band_carries_its_authored_ink() {
        let ink = |c: f32| Shade::Ink {
            rgb: Rgb {
                r: 1.0 - c,
                g: 1.0,
                b: 1.0,
            },
            cmyk: [c, 0.0, 0.0, 0.0],
        };
        let m = mesh([ink(0.0), ink(1.0), ink(0.0)]);
        let b = mesh_bands(&m, None, Transform::identity(), 0, 1 << 20).unwrap();
        for band in &b.bands {
            let k = band.cmyk.unwrap();
            assert!(
                (k[0] - (1.0 - band.rgb[0])).abs() <= 1.0 / 255.0 + 1e-6,
                "{k:?}"
            );
        }
    }

    #[test]
    fn past_the_ceiling_banding_declines() {
        let m = mesh([rgb(0.0), rgb(1.0), rgb(0.0)]);
        let got = mesh_bands(&m, None, Transform::identity(), 0, 8);
        assert_eq!(got, Err(Decline::TooManyTriangles));
    }
}
