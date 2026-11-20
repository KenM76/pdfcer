//! Per-tile lamp lists for [`crate::GLSL_ES_300_FRAGMENT`]: the viewport cut
//! into square tiles, each listing the point and spot lights whose reach
//! may meet it, so a pixel visits only those.
//!
//! A tile lists every lamp whose reach sphere is not wholly outside one of
//! the planes through the eye bounding its column or row of the view,
//! widened by [`MARGIN`] pixels. A pixel's surface point projects into its
//! pixel, so every lamp adding light there is listed; the lists keep the
//! frame's lamp order, so the shader adds the same lamps in the same order
//! and the picture is the one every lamp draws, byte for byte.

use crate::FloatTexture;
use crate::camera::Camera;
use crate::gpu::bounce_build::{FLAT_WIDTH, MAX_SIDE};
use crate::lighting::shade::Local;
use crate::render::raster::View;
use crate::vec3::{dot, length, sub};

/// The side of a lamp tile, pixels.
pub const LAMP_TILE_SIZE: u32 = 16;

/// Pixels each tile is widened by on every side before lamps are tested
/// against it, covering the GPU's single-precision projection.
const MARGIN: f64 = 1.0;

/// A frame's lamp lists per tile ([`crate::FrameLights::tiles`]), for the
/// fragment shader's `u_tile_size`, `u_tile_columns`, `u_tile_rows`,
/// `u_tile_count` and `u_lamp_tiles` ([`LampTiles::texture`]).
///
/// Empty, `tile_size` 0, when the frame has no lamps, under a fisheye
/// (each face is drawn with the fisheye camera's lights, at the face's
/// size), when the lists would not fit one texture, or when they would not
/// halve the lamps a pixel visits on average (reading a tile's list costs
/// more than the reach test it saves): the shader then visits every lamp,
/// the same picture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct LampTiles {
    /// The tile side, [`LAMP_TILE_SIZE`]; 0 for no tiles.
    pub tile_size: u32,
    /// Tiles across the viewport.
    pub columns: u32,
    /// Tiles up the viewport.
    pub rows: u32,
    /// Each tile's lamps, as rows of the point and spot lights in
    /// [`crate::FrameLights::light_table`] less its `count`, ascending.
    /// Tile column `c` and row `r` is entry `r * columns + c`; row 0 is at
    /// the viewport's bottom, as `gl_FragCoord` counts.
    pub lists: Vec<Vec<u16>>,
}

impl LampTiles {
    /// `u_lamp_tiles`: an RGBA32F texture, 2,048 wide, read with
    /// `texelFetch` at flat index `i` as `(i % 2048, i / 2048)`. Texel `t`
    /// for `t` below `lists.len()` (`u_tile_count`) holds tile `t`'s first
    /// entry in the index run in `x` and its lamp count in `y`; index-run
    /// entry `k` is channel `k % 4` of texel `u_tile_count + k / 4`.
    #[must_use]
    pub fn texture(&self) -> FloatTexture {
        let mut texels = Vec::with_capacity(self.lists.len() + self.entries().div_ceil(4));
        let mut first = 0usize;
        for list in &self.lists {
            texels.push([first as f32, list.len() as f32, 0.0, 0.0]);
            first += list.len();
        }
        let run: Vec<f32> = self.lists.iter().flatten().map(|&i| f32::from(i)).collect();
        for chunk in run.chunks(4) {
            let mut t = [0.0; 4];
            for (slot, v) in t.iter_mut().zip(chunk) {
                *slot = *v;
            }
            texels.push(t);
        }
        FloatTexture::from_rows(FLAT_WIDTH, texels)
    }

    /// Entries across every list.
    fn entries(&self) -> usize {
        self.lists.iter().map(Vec::len).sum()
    }
}

/// The tiles of a `width` × `height` viewport seen from `camera`, listing
/// `local`, the frame's lamps in light-table order.
pub(crate) fn build(local: &[Local], camera: &Camera, width: u32, height: u32) -> LampTiles {
    let none = LampTiles::default();
    if local.is_empty() || width == 0 || height == 0 || local.len() > usize::from(u16::MAX) {
        return none;
    }
    let aspect = f64::from(width) / f64::from(height);
    let (Ok(view), Ok(basis)) = (View::new(camera, aspect), camera.basis()) else {
        return none;
    };
    let size = LAMP_TILE_SIZE;
    let (columns, rows) = (width.div_ceil(size), height.div_ceil(size));
    let mut lists = vec![Vec::new(); (columns * rows) as usize];
    for (i, l) in local.iter().enumerate() {
        let [right, up, forward] = basis;
        let d = sub(l.position, camera.eye);
        let (x, y, z) = (dot(d, right), dot(d, up), dot(d, forward));
        let r = l.reach + 1e-5 * (l.reach + length(d));
        let cols = span(&view, Axis::X, (x, z, r), width, columns);
        let rws = span(&view, Axis::Y, (y, z, r), height, rows);
        let (Some((c0, c1)), Some((r0, r1))) = (cols, rws) else {
            continue;
        };
        for row in r0..=r1 {
            for c in c0..=c1 {
                if let Some(list) = lists.get_mut((row * columns + c) as usize) {
                    list.push(i as u16);
                }
            }
        }
    }
    let out = LampTiles {
        tile_size: size,
        columns,
        rows,
        lists,
    };
    let entries = out.entries();
    let texels = out.lists.len() + entries.div_ceil(4);
    if texels > FLAT_WIDTH * MAX_SIDE || entries * 2 > out.lists.len() * local.len() {
        return none;
    }
    out
}

/// The screen axis a span runs along.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

/// The first and last tile along `axis` that a sphere at view-space
/// offset `a` along it, depth `z`, radius `r` may meet, over `pixels`
/// cut into `tiles`; `None` when it meets none. Infinite reach meets all.
fn span(
    view: &View,
    axis: Axis,
    (a, z, r): (f64, f64, f64),
    pixels: u32,
    tiles: u32,
) -> Option<(u32, u32)> {
    if !r.is_finite() {
        return Some((0, tiles - 1));
    }
    let px = f64::from(pixels);
    // Signed distance of the sphere's centre past the plane through the
    // eye where the screen coordinate along `axis` is pixel `p`.
    let past = |p: f64| {
        let s = 2.0 * p / px - 1.0;
        match *view {
            View::Perspective { focal, aspect, .. } => {
                let t = match axis {
                    Axis::X => s * aspect / focal,
                    Axis::Y => s / focal,
                };
                (a - t * z) / t.mul_add(t, 1.0).sqrt()
            }
            View::Orthographic {
                half_height,
                aspect,
            } => {
                let half = match axis {
                    Axis::X => half_height * aspect,
                    Axis::Y => half_height,
                };
                a - s * half
            }
        }
    };
    let size = f64::from(LAMP_TILE_SIZE);
    let meets = |t: u32| {
        let lo = f64::from(t) * size - MARGIN;
        let hi = (f64::from(t + 1) * size).min(px) + MARGIN;
        !(past(lo) < -r || past(hi) > r)
    };
    let first = (0..tiles).find(|&t| meets(t))?;
    let last = (first..tiles).rev().find(|&t| meets(t))?;
    Some((first, last))
}

#[cfg(test)]
#[path = "lamp_tiles_tests.rs"]
mod tests;
