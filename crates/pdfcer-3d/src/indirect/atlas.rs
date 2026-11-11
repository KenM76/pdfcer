//! A built [`Volume`] packed as one RGBA32F texture for a GPU host, so its
//! lookup reads the same probes [`Volume::irradiance`] reads.

use super::Volume;
use super::octa::{SIDE, TEXELS};

/// Probe tiles per atlas row.
pub(crate) const TILES_PER_ROW: usize = 64;
/// A tile's width in texels: the irradiance map, then the depth map.
pub(crate) const TILE_WIDTH: usize = 2 * SIDE;

/// The packed probes and the lattice they sit on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Atlas {
    /// RGBA32F texels, rows bottom first as uploaded, `width` × `height`.
    pub(crate) texels: Vec<f32>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) origin: [f64; 3],
    pub(crate) spacing: [f64; 3],
    pub(crate) dims: [usize; 3],
}

impl Volume {
    /// The last complete bounce as an [`Atlas`]. Probe `i`'s tile starts at
    /// texel (`16 (i mod 64)`, `8 (i div 64)`); its texel (`u`, `v`) is the
    /// irradiance of octahedral texel `v * 8 + u` (RGB, alpha 1 when the
    /// probe is used, else 0) and (`8 + u`, `v`) that texel's mean distance
    /// and the variance of the distance, as the lookup weighs them.
    pub(crate) fn atlas(&self) -> Atlas {
        let len = self.grid.len();
        let width = TILES_PER_ROW * TILE_WIDTH;
        let height = len.div_ceil(TILES_PER_ROW).max(1) * SIDE;
        let mut texels = vec![0.0f32; width * height * 4];
        for (i, (probe, &active)) in self.current.iter().zip(&self.active).enumerate() {
            let (x0, y0) = ((i % TILES_PER_ROW) * TILE_WIDTH, (i / TILES_PER_ROW) * SIDE);
            for t in 0..TEXELS {
                let (u, v) = (t % SIDE, t / SIDE);
                let row = (y0 + v) * width;
                let e = probe.irradiance.get(t).copied().unwrap_or_default();
                let [mean, square] = probe.depth.get(t).copied().unwrap_or_default();
                let alpha = if active { 1.0 } else { 0.0 };
                put(&mut texels, row + x0 + u, [e[0], e[1], e[2], alpha]);
                let variance = super::grid::variance(f64::from(mean), f64::from(square));
                put(
                    &mut texels,
                    row + x0 + SIDE + u,
                    [mean, variance as f32, 0.0, 0.0],
                );
            }
        }
        Atlas {
            texels,
            width,
            height,
            origin: self.grid.origin,
            spacing: self.grid.spacing,
            dims: self.grid.dims,
        }
    }
}

fn put(texels: &mut [f32], at: usize, rgba: [f32; 4]) {
    if let Some(slot) = texels.get_mut(4 * at..4 * at + 4) {
        slot.copy_from_slice(&rgba);
    }
}
