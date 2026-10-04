//! Projection, near-plane clipping and the depth-tested triangle fill
//! behind [`crate::render`].

use crate::Texture;
use crate::render::{Camera, Projection, RenderError};
use crate::vec3::{length, sub};

/// One clipped view-space corner: x, y, z, shade, u, v.
pub(crate) type ViewCorner = [f64; 6];

/// One screen-space corner: pixel x, pixel y, depth key; shade; u, v.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Corner {
    pub(crate) at: [f64; 3],
    pub(crate) shade: f64,
    pub(crate) uv: [f64; 2],
}

/// What a triangle is filled with: a straight-RGBA colour and, when
/// textured, the texture the corners' `uv` sample.
#[derive(Clone, Copy)]
pub(crate) struct Paint<'a> {
    pub(crate) colour: [u8; 4],
    pub(crate) texture: Option<&'a Texture>,
}

pub(crate) enum View {
    /// `focal` = 1 / tan(fov_y / 2); `near` = the clip distance.
    Perspective {
        focal: f64,
        aspect: f64,
        near: f64,
    },
    Orthographic {
        half_height: f64,
        aspect: f64,
    },
}

impl View {
    /// The projection for `camera` at width/height `aspect`; refuses a
    /// field of view outside (0, 180) degrees or a non-positive
    /// orthographic height.
    pub(crate) fn new(camera: &Camera, aspect: f64) -> Result<View, RenderError> {
        match camera.projection {
            Projection::Perspective { fov_y } => {
                if !(fov_y > 0.0 && fov_y < 180.0) {
                    return Err(RenderError::Camera(
                        "the field of view is not between 0 and 180 degrees",
                    ));
                }
                let distance = length(sub(camera.target, camera.eye));
                Ok(View::Perspective {
                    focal: 1.0 / (fov_y.to_radians() / 2.0).tan(),
                    aspect,
                    near: (distance * 1e-4).max(f64::MIN_POSITIVE),
                })
            }
            Projection::Orthographic { height } => {
                if !(height.is_finite() && height > 0.0) {
                    return Err(RenderError::Camera(
                        "the orthographic height is not positive",
                    ));
                }
                Ok(View::Orthographic {
                    half_height: height / 2.0,
                    aspect,
                })
            }
        }
    }

    /// The part of `tri` in front of the near plane (Sutherland–Hodgman
    /// against one plane): 0, 3 or 4 corners, every attribute interpolated
    /// along each cut edge.
    pub(crate) fn clip(&self, tri: [ViewCorner; 3]) -> Vec<ViewCorner> {
        let View::Perspective { near, .. } = *self else {
            return tri.to_vec();
        };
        let mut out = Vec::with_capacity(4);
        for (k, &p) in tri.iter().enumerate() {
            let q = tri.get((k + 1) % 3).copied().unwrap_or(p);
            let z = |v: ViewCorner| v.get(2).copied().unwrap_or(0.0);
            let (p_in, q_in) = (z(p) >= near, z(q) >= near);
            if p_in {
                out.push(p);
            }
            if p_in != q_in {
                let t = (near - z(p)) / (z(q) - z(p));
                let mut cut = p;
                for (c, d) in cut.iter_mut().zip(q) {
                    *c += t * (d - *c);
                }
                out.push(cut);
            }
        }
        out
    }

    /// The screen corner of view corner `v`: pixel x, pixel y and a depth
    /// key that is linear in screen space and larger for nearer points.
    pub(crate) fn project(&self, v: ViewCorner, width: u32, height: u32) -> Corner {
        let [x, y, z, shade, u, w] = v;
        let (nx, ny, key) = match *self {
            View::Perspective { focal, aspect, .. } => {
                (x * focal / (z * aspect), y * focal / z, 1.0 / z)
            }
            View::Orthographic {
                half_height,
                aspect,
            } => (x / (half_height * aspect), y / half_height, -z),
        };
        Corner {
            at: [
                (nx + 1.0) / 2.0 * f64::from(width),
                (1.0 - ny) / 2.0 * f64::from(height),
                key,
            ],
            shade,
            uv: [u, w],
        }
    }

    /// The weight that makes `uv` perspective-correct when interpolated in
    /// screen space: 1 / z under perspective (the depth key), 1 otherwise.
    fn uv_weight(&self, c: &Corner) -> f64 {
        match self {
            View::Perspective { .. } => at(c.at, 2),
            View::Orthographic { .. } => 1.0,
        }
    }
}

pub(crate) struct Target {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
    pub(crate) depth: Vec<f64>,
}

impl Target {
    /// Fill the screen triangle of `corners` with `paint`, depth-tested at
    /// pixel centres, the shade (and any texture coordinate,
    /// perspective-correct) interpolated across it. An opaque pixel
    /// replaces the pixel and its depth; a translucent one is blended over
    /// it, leaving the depth.
    pub(crate) fn fill(&mut self, view: &View, corners: [Corner; 3], paint: Paint<'_>) {
        let [a, b, c] = corners;
        let area = edge(a.at, b.at, c.at);
        if !area.is_finite() || area.abs() < 1e-12 {
            return;
        }
        let Some((x0, x1, y0, y1)) = self.bounds(&corners) else {
            return;
        };
        let q = corners.map(|k| view.uv_weight(&k));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = [f64::from(x) + 0.5, f64::from(y) + 0.5, 0.0];
                let w = [
                    edge(b.at, c.at, p),
                    edge(c.at, a.at, p),
                    edge(a.at, b.at, p),
                ]
                .map(|e| e / area);
                if w.iter().any(|&k| k < 0.0) {
                    continue;
                }
                let mix = |f: &dyn Fn(&Corner) -> f64| {
                    w.iter().zip(&corners).map(|(k, c)| k * f(c)).sum::<f64>()
                };
                let key = mix(&|c| at(c.at, 2));
                let i = y as usize * self.width as usize + x as usize;
                if self.depth.get(i).is_none_or(|&d| key <= d) {
                    continue;
                }
                let shade = if a.shade == b.shade && b.shade == c.shade {
                    a.shade
                } else {
                    mix(&|c| c.shade)
                };
                let colour = match paint.texture {
                    Some(t) => {
                        let qw: f64 = w.iter().zip(q).map(|(k, q)| k * q).sum();
                        let uv = [0, 1].map(|j| {
                            let s: f64 = w
                                .iter()
                                .zip(&corners)
                                .zip(q)
                                .map(|((k, c), q)| k * q * c.uv.get(j).copied().unwrap_or(0.0))
                                .sum();
                            s / qw
                        });
                        t.apply(paint.colour, t.sample(uv))
                    }
                    None => paint.colour,
                };
                self.blend(i, key, shaded(colour, shade));
            }
        }
    }

    /// The pixel rows and columns `corners` may cover, clipped to the
    /// target; `None` when off it.
    fn bounds(&self, corners: &[Corner; 3]) -> Option<(u32, u32, u32, u32)> {
        let lo = |i: usize| {
            corners
                .iter()
                .map(|c| at(c.at, i))
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0)
        };
        let hi = |i: usize, limit: u32| {
            corners
                .iter()
                .map(|c| at(c.at, i))
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .min(f64::from(limit) - 1.0)
        };
        let (x0, x1, y0, y1) = (lo(0), hi(0, self.width), lo(1), hi(1, self.height));
        (x0 <= x1 && y0 <= y1).then_some((x0 as u32, x1 as u32, y0 as u32, y1 as u32))
    }

    /// Write straight-RGBA `rgba` at pixel `i`: replacing it and its depth
    /// when opaque, blended over it otherwise.
    fn blend(&mut self, i: usize, key: f64, rgba: [u8; 4]) {
        let opaque = rgba[3] == 255;
        if opaque && let Some(d) = self.depth.get_mut(i) {
            *d = key;
        }
        let Some(px) = self.rgba.get_mut(i * 4..i * 4 + 4) else {
            return;
        };
        if opaque {
            px.copy_from_slice(&rgba);
        } else if let [pr, pg, pb, pa] = px {
            let [sr, sg, sb, sa] = rgba;
            let src = f64::from(sa) / 255.0;
            let dst = f64::from(*pa) / 255.0 * (1.0 - src);
            let out = src + dst;
            if out <= 0.0 {
                return;
            }
            for (p, s) in [pr, pg, pb].into_iter().zip([sr, sg, sb]) {
                *p = ((f64::from(s) * src + f64::from(*p) * dst) / out).round() as u8;
            }
            *pa = (out * 255.0).round() as u8;
        }
    }
}

/// `colour`'s RGB scaled by `shade`, alpha kept.
fn shaded(colour: [u8; 4], shade: f64) -> [u8; 4] {
    let [r, g, b, a] = colour;
    let [r, g, b] = [r, g, b].map(|v| (f64::from(v) * shade).round() as u8);
    [r, g, b, a]
}

/// Twice the signed area of (a, b, p) in the xy plane.
fn edge(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    (at(b, 0) - at(a, 0)) * (at(p, 1) - at(a, 1)) - (at(b, 1) - at(a, 1)) * (at(p, 0) - at(a, 0))
}

/// Component `i` of `v`, 0.0 out of range; the crate denies indexing.
pub(crate) fn at(v: [f64; 3], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{TextureFunction, TextureOrigin, TextureWrap};

    #[test]
    fn texture_coordinates_are_perspective_correct() {
        // Left half red, right half green; u runs 0 to 1 along the top
        // edge, whose right end is four times as far away.
        let texture = Texture {
            width: 2,
            height: 1,
            rgba: [[255, 0, 0, 255], [0, 255, 0, 255]].concat(),
            wrap: [TextureWrap::Clamp; 2],
            function: TextureFunction::Replace,
            uv_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uv_set: 0,
            origin: TextureOrigin::TopLeft,
        };
        // Screen corners; the top edge's right end has depth key 0.25,
        // four times as far as the others.
        let corner = |at, u| Corner {
            at,
            shade: 1.0,
            uv: [u, 0.0],
        };
        let corners = [
            corner([0.0, 0.0, 1.0], 0.0),
            corner([8.0, 0.0, 0.25], 1.0),
            corner([0.0, 8.0, 1.0], 0.0),
        ];
        let paint = Paint {
            colour: [0, 0, 0, 255],
            texture: Some(&texture),
        };
        let fill = |view: &View| {
            let mut target = Target {
                width: 8,
                height: 8,
                rgba: vec![0; 8 * 8 * 4],
                depth: vec![f64::NEG_INFINITY; 64],
            };
            target.fill(view, corners, paint);
            target.rgba[4 * 4..4 * 4 + 4].to_vec()
        };
        let perspective = View::Perspective {
            focal: 1.0,
            aspect: 1.0,
            near: 0.1,
        };
        // Past halfway across the screen is still under a quarter of u.
        assert_eq!(fill(&perspective), [255, 0, 0, 255]);
        let ortho = View::Orthographic {
            half_height: 1.0,
            aspect: 1.0,
        };
        assert_ne!(fill(&ortho), [255, 0, 0, 255], "affine would reach green");
    }
}
