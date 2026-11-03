//! What casts a shadow and how much of the light it stops: opaque meshes
//! fully, see-through surfaces partly, cut-out textures per texel
//! ([`ShadowAlpha`]). Shared by the directional maps ([`crate::shadow`]) and
//! the lamp cubes ([`crate::lamp_shadow`]); the GPU passes test the same
//! alpha against [`ShadowAlpha::solid_range`] and
//! [`ShadowAlpha::veil_range`].

use crate::TriangleMesh;
use crate::raster::Paint;
use crate::render::{corners, triangle_uvs};

/// [`ShadowAlpha::Cutout`]'s usual cutoff: glTF's default `alphaCutoff`,
/// 0.5, on the 0–255 scale (a fragment casts when alpha ≥ 127.5).
pub const DEFAULT_SHADOW_CUTOFF: u8 = 128;

/// How a surface that is not wholly opaque casts
/// ([`crate::RenderOptions::shadow_alpha`]). Alpha is the drawn alpha: the
/// paint's times the texture's, as the surface is drawn. A glTF `MASK`
/// material's texels arrive already cut to 0 or 255 at the file's
/// `alphaCutoff`, so they cast per texel under every choice but
/// [`ShadowAlpha::OpaqueOnly`]. pdfcer's: neither standard defines shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ShadowAlpha {
    /// A fragment of alpha `a` lets `1 − a` of the light through: alpha
    /// 255 casts fully, alpha 0 not at all. The light passing several
    /// see-through layers is the product of theirs, untinted. The default.
    #[default]
    Transmit,
    /// A fragment casts fully where its alpha is at least `cutoff`
    /// (0–255; [`DEFAULT_SHADOW_CUTOFF`] is glTF's 0.5) and not at all
    /// below it.
    Cutout {
        /// The least alpha that casts.
        cutoff: u8,
    },
    /// Only meshes drawn wholly opaque cast, fully; a see-through or
    /// cut-out mesh casts nothing.
    OpaqueOnly,
}

impl ShadowAlpha {
    /// The alpha range, `lo ≤ a < hi` on 0–1, of a see-through draw's
    /// fragments that go into a shadow map's depth as fully casting;
    /// `None` when no see-through draw casts.
    #[must_use]
    pub fn solid_range(self) -> Option<[f32; 2]> {
        match self {
            ShadowAlpha::Transmit => Some([254.5 / 255.0, 2.0]),
            ShadowAlpha::Cutout { cutoff } => {
                Some([((f32::from(cutoff) - 0.5) / 255.0).max(0.5 / 255.0), 2.0])
            }
            ShadowAlpha::OpaqueOnly => None,
        }
    }

    /// The alpha range of the fragments that cast partly, into the veil
    /// layers; `None` unless [`ShadowAlpha::Transmit`].
    #[must_use]
    pub fn veil_range(self) -> Option<[f32; 2]> {
        matches!(self, ShadowAlpha::Transmit).then_some([0.5 / 255.0, 254.5 / 255.0])
    }

    /// What a fragment of `alpha` does to the light.
    pub(crate) fn cast(self, alpha: u8) -> Cast {
        match (self, alpha) {
            (_, 0) | (ShadowAlpha::OpaqueOnly, _) => Cast::Clear,
            (_, 255) => Cast::Solid,
            (ShadowAlpha::Cutout { cutoff }, a) if a >= cutoff => Cast::Solid,
            (ShadowAlpha::Cutout { .. }, _) => Cast::Clear,
            (ShadowAlpha::Transmit, a) => Cast::Veil(1.0 - f32::from(a) / 255.0),
        }
    }
}

/// Which directional lights cast ([`crate::RenderOptions::shadow_casters`]).
/// pdfcer's: the standard defines no shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ShadowCasters {
    /// Every directional light lighting the frame, up to
    /// [`crate::MAX_SHADOW_CASTERS`]; each shadow removes only its own light. The
    /// default.
    #[default]
    EveryLight,
    /// One light, the key light [`crate::RenderOptions::shadows`] names.
    KeyLight,
}

/// What one occluder fragment does to the light reaching past it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Cast {
    /// Stops it.
    Solid,
    /// Lets this share through.
    Veil(f32),
    /// Nothing.
    Clear,
}

/// A see-through or cut-out occluder's paint, corner texture coordinates
/// and rule.
#[derive(Clone, Copy)]
pub(crate) struct Surface<'p> {
    paint: Paint<'p>,
    uv: [[f64; 2]; 3],
    mode: ShadowAlpha,
}

impl Surface<'_> {
    /// What the fragment at barycentric weights `w` (perspective-correct)
    /// casts.
    pub(crate) fn cast(&self, w: [f64; 3]) -> Cast {
        let alpha = match self.paint.texture {
            Some(t) => {
                let uv = [0, 1].map(|j| {
                    w.iter()
                        .zip(&self.uv)
                        .map(|(k, c)| k * c.get(j).copied().unwrap_or(0.0))
                        .sum::<f64>()
                });
                t.apply(self.paint.colour, t.sample(uv))[3]
            }
            None => self.paint.colour[3],
        };
        self.mode.cast(alpha)
    }
}

/// One model triangle drawn into shadow maps.
pub(crate) struct Occluder<'p> {
    /// Model-space corners.
    pub(crate) world: [[f64; 3]; 3],
    /// `None` for an opaque mesh's, which casts fully everywhere.
    pub(crate) surface: Option<Surface<'p>>,
}

/// Every triangle that may cast under `mode`: each opaque mesh's, then,
/// unless [`ShadowAlpha::OpaqueOnly`], each drawn see-through one's. A mesh
/// of alpha 0 is not drawn and casts nothing.
pub(crate) fn occluders<'p>(
    meshes: &[TriangleMesh],
    paints: &[Paint<'p>],
    opaque: &[bool],
    mode: ShadowAlpha,
) -> Vec<Occluder<'p>> {
    let mut out = Vec::new();
    for (i, mesh) in meshes.iter().enumerate() {
        let Some(&paint) = paints.get(i).filter(|p| p.colour[3] != 0) else {
            continue;
        };
        let solid = opaque.get(i).copied().unwrap_or(false);
        if !solid && mode == ShadowAlpha::OpaqueOnly {
            continue;
        }
        for (t, tri) in mesh.triangles.iter().enumerate() {
            let Some(world) = corners(mesh, tri) else {
                continue;
            };
            let surface = (!solid).then(|| Surface {
                paint,
                uv: paint
                    .texture
                    .and_then(|tex| triangle_uvs(mesh, tex.uv_set, t))
                    .unwrap_or([[0.0; 2]; 3]),
                mode,
            });
            out.push(Occluder { world, surface });
        }
    }
    out
}

/// Whether any of `occluders` casts partly, so a map needs veil layers.
pub(crate) fn veiled(occluders: &[Occluder<'_>]) -> bool {
    occluders
        .iter()
        .any(|o| o.surface.is_some_and(|s| s.mode == ShadowAlpha::Transmit))
}

/// A map's see-through layers: per texel, the depth of the nearest partly
/// casting fragment and the product of every such fragment's share let
/// through.
pub(crate) struct Veil {
    pub(crate) depth: Vec<f32>,
    pub(crate) transmit: Vec<f32>,
}

impl Veil {
    /// `len` texels with nothing in them.
    pub(crate) fn new(len: usize) -> Veil {
        Veil {
            depth: vec![1.0; len],
            transmit: vec![1.0; len],
        }
    }

    /// The share of light texel `i` lets through to a point of (biased)
    /// map depth `depth`: its product when the point is deeper than the
    /// nearest see-through fragment, else all of it.
    pub(crate) fn through(&self, i: usize, depth: f64) -> f64 {
        match (self.depth.get(i), self.transmit.get(i)) {
            (Some(&d), Some(&t)) if depth > f64::from(d) => f64::from(t),
            _ => 1.0,
        }
    }
}

/// A run of a map's texels being drawn: solid depth and, when the map has
/// them, the veil layers over the same texels.
pub(crate) struct Texels<'a> {
    pub(crate) depth: &'a mut [f32],
    pub(crate) veil: Option<(&'a mut [f32], &'a mut [f32])>,
}

impl Texels<'_> {
    /// Texel `i` takes a fragment at depth `z` casting `cast`. Each layer
    /// keeps its nearest depth; transmission multiplies, so the result is
    /// the same in any order.
    pub(crate) fn put(&mut self, i: usize, z: f32, cast: Cast) {
        match cast {
            Cast::Solid => {
                if let Some(d) = self.depth.get_mut(i)
                    && z < *d
                {
                    *d = z;
                }
            }
            Cast::Veil(share) => {
                if let Some((depth, transmit)) = &mut self.veil {
                    if let Some(d) = depth.get_mut(i)
                        && z < *d
                    {
                        *d = z;
                    }
                    if let Some(t) = transmit.get_mut(i) {
                        *t *= share;
                    }
                }
            }
            Cast::Clear => {}
        }
    }
}

/// Cuts `depth` and an optional veil into texel runs of `run` texels each.
pub(crate) fn runs<'a>(
    depth: &'a mut [f32],
    veil: Option<&'a mut Veil>,
    run: usize,
) -> Vec<Texels<'a>> {
    let run = run.max(1);
    let mut veils: Vec<Option<(&mut [f32], &mut [f32])>> = match veil {
        Some(v) => v
            .depth
            .chunks_mut(run)
            .zip(v.transmit.chunks_mut(run))
            .map(Some)
            .collect(),
        None => Vec::new(),
    };
    veils.reverse();
    depth
        .chunks_mut(run)
        .map(|depth| Texels {
            depth,
            veil: veils.pop().flatten(),
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;

    #[test]
    fn each_rule_casts_as_documented() {
        let t = ShadowAlpha::Transmit;
        assert_eq!(t.cast(255), Cast::Solid);
        assert_eq!(t.cast(0), Cast::Clear);
        assert_eq!(t.cast(51), Cast::Veil(0.8));
        let c = ShadowAlpha::Cutout {
            cutoff: DEFAULT_SHADOW_CUTOFF,
        };
        assert_eq!((c.cast(128), c.cast(127)), (Cast::Solid, Cast::Clear));
        assert_eq!(ShadowAlpha::OpaqueOnly.cast(255), Cast::Clear);
    }

    #[test]
    fn the_gpu_ranges_split_alpha_where_the_cpu_does() {
        let [lo, hi] = ShadowAlpha::Transmit.veil_range().unwrap();
        assert!(lo < 1.0 / 255.0 && hi > 254.0 / 255.0 && hi < 1.0);
        assert_eq!(ShadowAlpha::Transmit.solid_range().unwrap()[0], hi);
        let [cut, _] = ShadowAlpha::Cutout { cutoff: 128 }.solid_range().unwrap();
        assert!(cut > 127.0 / 255.0 && cut < 128.0 / 255.0);
        assert_eq!(ShadowAlpha::OpaqueOnly.solid_range(), None);
        assert_eq!(ShadowAlpha::Cutout { cutoff: 9 }.veil_range(), None);
    }

    #[test]
    fn a_veil_multiplies_in_any_order_and_spares_what_is_nearer() {
        let mut veil = Veil::new(1);
        let mut depth = vec![1.0f32];
        for (z, share) in [(0.6, 0.5), (0.4, 0.8)] {
            let mut run = runs(&mut depth, Some(&mut veil), 1);
            run[0].put(0, z, Cast::Veil(share));
        }
        assert_eq!(veil.depth[0], 0.4);
        assert!((veil.transmit[0] - 0.4).abs() < 1e-6);
        assert!((veil.through(0, 0.5) - 0.4).abs() < 1e-6);
        assert_eq!(veil.through(0, 0.3), 1.0);
        assert_eq!(depth[0], 1.0);
    }
}
