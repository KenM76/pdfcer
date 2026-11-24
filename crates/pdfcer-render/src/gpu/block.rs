//! One fill laid out as a position-independent run of texels, so fills are
//! laid out in parallel and the scene is their concatenation.
//!
//! Texel 0 is the header `h`; every offset inside is relative to it:
//! `[x0, y0, x1, y1]`, `[start_y, stop_y, even_odd, table]`,
//! `[tc0, tr0, tcols, trows]`, `[ox, oy, shift, 0]` (the `DrawTiler` tile's
//! origin: the edges, area and tiles are in that tile's pixels, raster
//! pixel `p` is tile pixel `p - o`; `shift` 0 is an aliased fill), then
//! the tile table (per tile `[first_edge, count, backdrop or -1, split or
//! 0]`, `[0, 0, -1, 0]` where empty), then the edges
//! (`[x, dx, first_y, last_y << 1 | rising]`), backdrops (4 texels) and
//! split masks (1 texel, [`super::bins::TileEntry::split`] low word first).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use super::bins::{FillTiles, SUB_ROWS, TILE_H, TILE_W};
use super::fill_edges::FillEdges;

/// FNV-1a over the hashed bytes; backdrops are small and not adversarial.
#[derive(Default)]
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn write_i32(&mut self, v: i32) {
        self.0 = (self.0 ^ u64::from(v as u32)).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

type BuildFnv = BuildHasherDefault<Fnv>;

/// Which raster tiles a fill covers.
pub(crate) struct Footprint {
    col0: i32,
    row0: i32,
    cols: i32,
    rows: i32,
    covered: Vec<bool>,
}

impl Footprint {
    /// Whether raster tile (`col`, `row`) is one the fill touches.
    pub(crate) fn covers(&self, col: i32, row: i32) -> bool {
        let (c, r) = (col - self.col0, row - self.row0);
        c >= 0
            && r >= 0
            && c < self.cols
            && r < self.rows
            && self.covered.get((r * self.cols + c) as usize) == Some(&true)
    }

    /// Covered tiles, row-major.
    pub(crate) fn tiles(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.covered
            .iter()
            .enumerate()
            .filter(|(_, c)| **c)
            .map(|(k, _)| {
                let k = k as i32;
                (self.col0 + k % self.cols, self.row0 + k / self.cols)
            })
    }
}

/// A fill's texels and footprint.
pub(crate) struct FillBlock {
    pub texels: Vec<[i32; 4]>,
    pub footprint: Footprint,
}

/// Texels before the tile table.
const HEAD: usize = 4;

/// The raster tiles the fill's tiles, `origin` pixels from the raster's,
/// overlap; the identity when `origin` is zero.
fn footprint(tiles: &FillTiles, local: &[bool], (ox, oy): (i32, i32)) -> Footprint {
    let span = |first: i32, n: i32, size: i32, o: i32| {
        let lo = (first * size + o).div_euclid(size);
        (lo, ((first + n) * size - 1 + o).div_euclid(size) - lo + 1)
    };
    let (col0, cols) = span(tiles.col0, tiles.cols, TILE_W, ox);
    let (row0, rows) = span(tiles.row0, tiles.rows, TILE_H, oy);
    if (ox, oy) == (0, 0) {
        return Footprint {
            col0,
            row0,
            cols,
            rows,
            covered: local.to_vec(),
        };
    }
    let mut covered = vec![false; (cols * rows) as usize];
    for (k, _) in local.iter().enumerate().filter(|(_, c)| **c) {
        let (c, r) = (
            tiles.col0 + k as i32 % tiles.cols,
            tiles.row0 + k as i32 / tiles.cols,
        );
        let (x, y) = (c * TILE_W + ox, r * TILE_H + oy);
        for gr in y.div_euclid(TILE_H)..=(y + TILE_H - 1).div_euclid(TILE_H) {
            for gc in x.div_euclid(TILE_W)..=(x + TILE_W - 1).div_euclid(TILE_W) {
                if let Some(v) = covered.get_mut(((gr - row0) * cols + gc - col0) as usize) {
                    *v = true;
                }
            }
        }
    }
    Footprint {
        col0,
        row0,
        cols,
        rows,
        covered,
    }
}

/// `edges`, scanned into a `DrawTiler` tile `origin` pixels from the
/// raster's, cut into `tiles` and laid out.
pub(crate) fn build(edges: &FillEdges, tiles: FillTiles, origin: (i32, i32)) -> FillBlock {
    let a = &edges.area;
    let mut t: Vec<[i32; 4]> = Vec::with_capacity(HEAD + tiles.entries.len());
    t.push([a.x(), a.y(), a.right(), a.bottom()]);
    t.push([
        edges.start_y,
        edges.stop_y,
        i32::from(edges.even_odd),
        HEAD as i32,
    ]);
    t.push([tiles.col0, tiles.row0, tiles.cols, tiles.rows]);
    t.push([origin.0, origin.1, edges.shift, 0]);
    t.resize(HEAD + tiles.entries.len(), [0, 0, -1, 0]);
    let mut backdrops: HashMap<[i32; SUB_ROWS], i32, BuildFnv> = HashMap::default();
    // Neighbouring tiles mostly share a backdrop; this skips the hash.
    let mut last: Option<([i32; SUB_ROWS], i32)> = None;
    let mut covered = Vec::with_capacity(tiles.entries.len());
    for (k, entry) in tiles.entries.iter().enumerate() {
        covered.push(entry.is_some());
        let Some(entry) = entry else { continue };
        let first = t.len() as i32;
        for &i in &entry.edges {
            if let Some(e) = edges.edges.get(i as usize) {
                t.push([
                    e.x,
                    e.dx,
                    e.first_y,
                    (e.last_y << 1) | i32::from(e.winding > 0),
                ]);
            }
        }
        let back = if entry.backdrop.iter().all(|w| *w == 0) {
            -1
        } else if let Some((_, at)) = last.filter(|(b, _)| *b == entry.backdrop) {
            at
        } else if let Some(at) = backdrops.get(&entry.backdrop) {
            *at
        } else {
            let at = t.len() as i32;
            t.extend(
                entry
                    .backdrop
                    .chunks(4)
                    .map(|c| std::array::from_fn(|k| c.get(k).copied().unwrap_or(0))),
            );
            backdrops.insert(entry.backdrop, at);
            at
        };
        if back >= 0 {
            last = Some((entry.backdrop, back));
        }
        let split = if entry.split == 0 {
            0
        } else {
            let at = t.len() as i32;
            #[allow(clippy::cast_possible_truncation)] // the mask's two words
            t.push([entry.split as i32, (entry.split >> 32) as i32, 0, 0]);
            at
        };
        if let Some(slot) = t.get_mut(HEAD + k) {
            *slot = [first, entry.edges.len() as i32, back, split];
        }
    }
    FillBlock {
        footprint: footprint(&tiles, &covered, origin),
        texels: t,
    }
}
