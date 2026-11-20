//! [`super::glsl::GLSL_ES_300_FRAGMENT`]'s algorithm on the CPU, read from the
//! packed texture the way the shader reads it, so a host can check a
//! graphics card's output and the tests can check the packing.

use super::bins::round;
use super::fill_edges::DRAW_TILE;
use super::image::{self, IMAGE_PAINT};
use super::scene::{GpuScene, IntTexture};

/// Clip nesting the shader follows before treating a clip as empty.
pub(crate) const MAX_CLIP_DEPTH: usize = 64;

struct Tex<'a>(&'a IntTexture);

impl Tex<'_> {
    fn at(&self, i: i32) -> [i32; 4] {
        let at = usize::try_from(i).unwrap_or(usize::MAX).saturating_mul(4);
        match self.0.texels.get(at..at.saturating_add(4)) {
            Some(&[a, b, c, d]) => [a, b, c, d],
            _ => [0; 4],
        }
    }

    /// Value `e` of a run packed four to a texel.
    fn lane(&self, e: i32) -> i32 {
        self.at(e >> 2).get((e & 3) as usize).copied().unwrap_or(0)
    }
}

/// An edge texel: `[x, dx, first_y, last_y << 1 | rising]`.
#[derive(Clone, Copy)]
struct Edge {
    x: i32,
    dx: i32,
    first_y: i32,
    last_y: i32,
    winding: i32,
    index: i32,
}

impl Edge {
    fn read(t: &Tex<'_>, index: i32) -> Self {
        let [x, dx, first_y, w] = t.at(index);
        Self {
            x,
            dx,
            first_y,
            last_y: w >> 1,
            winding: if w & 1 != 0 { 1 } else { -1 },
            index,
        }
    }

    fn x_at(&self, y: i32) -> i32 {
        self.x.wrapping_add(self.dx.wrapping_mul(y - self.first_y))
    }

    fn active(&self, y: i32) -> bool {
        self.first_y <= y && y <= self.last_y
    }

    /// Whether the walk visits `self` before `o` on row `y`.
    fn before(&self, o: &Self, y: i32) -> bool {
        let (xa, xb) = (self.x_at(y), o.x_at(y));
        if xa != xb {
            return xa < xb;
        }
        if self.first_y != y && o.first_y != y && self.dx != o.dx {
            return xa.wrapping_sub(self.dx) < xb.wrapping_sub(o.dx);
        }
        (self.first_y, self.index) < (o.first_y, o.index)
    }
}

/// The fill whose header texel is `h` at (`px`, `py`): its coverage, as
/// [`super::fill_coverage::pixel`] computes from its edges.
fn coverage(t: &Tex<'_>, h: i32, px: i32, py: i32) -> i32 {
    let [ox, oy, shift, _] = t.at(h + 3);
    let (px, py) = (px - ox, py - oy);
    let [x0, y0, x1, y1] = t.at(h);
    if px < x0 || px >= x1 || py < y0 || py >= y1 {
        return 0;
    }
    let [start_y, stop_y, even_odd, table] = t.at(h + 1);
    let [tc0, tr0, tcols, trows] = t.at(h + 2);
    let (c, r) = (px.div_euclid(16) - tc0, py.div_euclid(4) - tr0);
    if c < 0 || r < 0 || c >= tcols || r >= trows {
        return 0;
    }
    let [first, count, back, _] = t.at(h + table + r * tcols + c);
    let (base, back) = (h + first, if back < 0 { back } else { h + back });
    let filled = |w: i32| if even_odd != 0 { w & 1 != 0 } else { w != 0 };
    if shift == 0 {
        if py < start_y || py >= stop_y {
            return 0;
        }
        return aliased(t, (base, count, back), py & 3, (px, py), filled);
    }
    let mut sum = 0;
    for k in 0..4 {
        let y = (py << 2) + k;
        if y < start_y || y >= stop_y {
            continue;
        }
        let s = (py & 3) * 4 + k;
        let b = if back < 0 { 0 } else { t.lane((back << 2) + s) };
        let c0 = px << 2;
        let mut winding = [b; 4];
        for i in base..base + count {
            let e = Edge::read(t, i);
            if !e.active(y) {
                continue;
            }
            let x = round(e.x_at(y));
            for (j, w) in winding.iter_mut().enumerate() {
                if x <= c0 + j as i32 {
                    *w += e.winding;
                }
            }
        }
        let n = winding.iter().filter(|w| filled(**w)).count() as i32;
        sum += if n < 4 {
            16 * n
        } else if k == 3 && !split(t, (base, count), b, c0, y, filled) {
            63
        } else {
            64
        };
    }
    sum.min(255)
}

/// An aliased fill at (`px`, `py`), tile pixels, its tile's edges at `base`
/// and backdrop at `back`, `s` its row in the tile: 255 where the edges at
/// or left of the pixel satisfy the rule.
fn aliased(
    t: &Tex<'_>,
    (base, count, back): (i32, i32, i32),
    s: i32,
    (px, py): (i32, i32),
    filled: impl Fn(i32) -> bool,
) -> i32 {
    let mut w = if back < 0 { 0 } else { t.lane((back << 2) + s) };
    for i in base..base + count {
        let e = Edge::read(t, i);
        if e.active(py) && round(e.x_at(py)) <= px {
            w += e.winding;
        }
    }
    if filled(w) { 255 } else { 0 }
}

/// Whether the walk ends a span inside the pixel whose first sample column
/// is `c0` on fully covered row `y`: after some edge followed, in walk
/// order, by another at the same rounded `x` inside the pixel, the winding
/// is unfilled. Only listed edges can lie inside a pixel; `back` is the
/// winding of every edge left of the tile.
fn split(
    t: &Tex<'_>,
    (base, count): (i32, i32),
    back: i32,
    c0: i32,
    y: i32,
    filled: impl Fn(i32) -> bool,
) -> bool {
    for i in base..base + count {
        let e = Edge::read(t, i);
        let x = round(e.x_at(y));
        if !e.active(y) || x <= c0 || x >= c0 + 4 {
            continue;
        }
        let mut follower = false;
        let mut w = back;
        for j in base..base + count {
            let f = Edge::read(t, j);
            if !f.active(y) {
                continue;
            }
            if j == i || f.before(&e, y) {
                w += f.winding;
            } else if round(f.x_at(y)) == x {
                follower = true;
            }
        }
        if follower && !filled(w) {
            return true;
        }
    }
    false
}

/// Clip `clip`'s mask value at (`px`, `py`): its path's coverage times its
/// parent's, divided by 255, from the outermost clip in.
fn mask(t: &Tex<'_>, clip_base: i32, (clip, k, stride): (i32, i32, i32), px: i32, py: i32) -> i32 {
    let mut chain = [0i32; MAX_CLIP_DEPTH];
    let mut n = 0;
    let mut at = clip;
    while at >= 0 {
        let [h, parent, _, _] = t.at(clip_base + at * stride + k);
        let Some(slot) = chain.get_mut(n).filter(|_| h >= 0) else {
            return 0;
        };
        *slot = h;
        n += 1;
        at = parent;
    }
    let mut m = 255;
    for (k, h) in chain.get(..n).unwrap_or_default().iter().rev().enumerate() {
        let c = coverage(t, *h, px, py);
        m = if k == 0 { c } else { c * m / 255 };
    }
    m
}

fn div255(v: i32) -> i32 {
    (v + 255) >> 8
}

fn channels(v: i32) -> [i32; 4] {
    v.to_le_bytes().map(i32::from)
}

/// The image paint whose fill header is `h` and record `rec` drawn over
/// `d` at raster pixel (`px`, `py`) with clip `mask` and coverage `c`.
fn image_pixel(
    t: &Tex<'_>,
    (h, rec): (i32, i32),
    (px, py): (i32, i32),
    d: [i32; 4],
    (mask, c): (Option<f32>, i32),
) -> [i32; 4] {
    let [ox, oy, _, _] = t.at(h + 3);
    let [at, w, ih, bilinear] = t.at(rec);
    let ([a, b, cc, dd], [e, f, _, _]) = (t.at(rec + 1), t.at(rec + 2));
    let inverse = [a, b, cc, dd, e, f].map(|v| f32::from_bits(v as u32));
    let fetch = |i: i32| t.lane(at * 4 + i);
    let s = image::sample(fetch, (w, ih), bilinear != 0, inverse, (px - ox, py - oy));
    image::over(d, s, mask, c)
}

/// One pixel of `scene`, premultiplied, before the flatten over white.
fn paint(t: &Tex<'_>, px: i32, py: i32) -> [i32; 4] {
    let [w, h, cols, _] = t.at(0);
    let [tile_base, op_base, clip_base, div_base] = t.at(1);
    let tile = DRAW_TILE as i32;
    let dcols = (w + tile - 1) / tile;
    let (k, stride) = (
        (py / tile) * dcols + px / tile,
        dcols * ((h + tile - 1) / tile),
    );
    let [first, count, _, _] = t.at(tile_base + py.div_euclid(4) * cols + px.div_euclid(16));
    let mut d = [0i32; 4];
    for e in first..first + count {
        let [h, clip, color, memset] = t.at(op_base + t.lane(e));
        let c = coverage(t, h, px, py);
        if c == 0 {
            continue;
        }
        if color == IMAGE_PAINT {
            let mask = match clip {
                ..0 => None,
                _ => match mask(t, clip_base, (clip, k, stride), px, py) {
                    0 => continue,
                    m => Some(f32::from_bits(t.lane(div_base * 4 + m) as u32)),
                },
            };
            d = image_pixel(t, (h, memset), (px, py), d, (mask, c));
            continue;
        }
        let u = channels(color);
        if clip < 0 && u[3] == 255 {
            if c == 255 {
                d = channels(memset);
            } else {
                for (dv, uv) in d.iter_mut().zip(u) {
                    *dv = div255(*dv * (255 - c) + uv * c);
                }
            }
            continue;
        }
        let mut s = u;
        if clip >= 0 {
            let m = mask(t, clip_base, (clip, k, stride), px, py);
            if m == 0 {
                continue;
            }
            s = s.map(|v| div255(v * m));
        }
        if c < 255 {
            s = s.map(|v| div255(v * c));
        }
        let keep = 255 - s[3];
        for (dv, sv) in d.iter_mut().zip(s) {
            *dv = sv + div255(*dv * keep);
        }
    }
    d
}

impl GpuScene {
    /// The raster the shader draws for this scene, computed on the CPU the
    /// way the shader computes it: RGBA8, top row first, equal to
    /// [`crate::DisplayList::replay_region`]'s bytes. Slow; for checking a
    /// graphics card's output, not for drawing.
    #[must_use]
    pub fn evaluate(&self) -> Vec<u8> {
        let t = Tex(&self.data);
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for py in 0..self.height as i32 {
            for px in 0..self.width as i32 {
                let d = paint(&t, px, py);
                // `flatten_page_group_over_white`.
                let a = d[3];
                for v in &d[..3] {
                    out.push((v + 255 - a).clamp(0, 255) as u8);
                }
                out.push(255);
            }
        }
        out
    }
}

/// `fill`'s coverage at every pixel of `w` x `h`, read from a texture
/// holding only it, row-major.
#[cfg(test)]
pub(crate) fn packed_coverage(fill: super::fill_edges::FillEdges, w: i32, h: i32) -> Vec<u8> {
    let tiles = super::bins::build(&fill);
    let data = IntTexture {
        texels: super::block::build(&fill, tiles, (0, 0))
            .texels
            .into_flattened(),
        width: 0,
        height: 0,
        layers: 0,
    };
    let t = Tex(&data);
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.push(coverage(&t, 0, x, y) as u8);
        }
    }
    v
}
