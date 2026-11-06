//! Projection, near-plane clipping and the depth-tested triangle fill
//! behind [`crate::render`].

use crate::Texture;
use crate::camera::Camera;
use crate::lighting::gloss::{Gloss, Resolved};
use crate::lighting::shade::{Local, add_local};
use crate::render::{Projection, RenderError};
use crate::shadow::ShadowMap;
use crate::shadow::lamp::LampShadows;
use crate::shadow::set::ExtraShadows;
use crate::vec3::{length, sub};

/// One clipped view-space corner: x, y, z, red/green/blue shade, u, v,
/// red/green/blue shadowed share of the shade, shadow-map lookup point,
/// model point, model normal.
pub(crate) type ViewCorner = [f64; 20];

/// A view corner at `x, y, z` carrying no shade, texture or shadow.
pub(crate) fn view_point(x: f64, y: f64, z: f64) -> ViewCorner {
    let mut v = [0.0; 20];
    for (slot, c) in v.iter_mut().zip([x, y, z]) {
        *slot = c;
    }
    v
}

/// One screen-space corner: pixel x, pixel y, depth key; shade; u, v; the
/// share of the shade a shadow removes ([`crate::shadow`]); its shadow-map
/// lookup point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Corner {
    pub(crate) at: [f64; 3],
    pub(crate) shade: [f64; 3],
    pub(crate) uv: [f64; 2],
    pub(crate) key: [f64; 3],
    pub(crate) light: [f64; 3],
    /// The model point and normal, for point and spot lights.
    pub(crate) model: [[f64; 3]; 2],
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
    /// field of view outside (0, 180) degrees or too small for a finite
    /// focal length, or a non-positive orthographic height, and every
    /// fisheye: a lens is drawn as planar faces ([`crate::camera::lens`]). The near
    /// plane is 1e-4 of the eye-to-target distance.
    pub(crate) fn new(camera: &Camera, aspect: f64) -> Result<View, RenderError> {
        match camera.projection {
            Projection::Perspective { fov_y } => {
                if !(fov_y > 0.0 && fov_y < 180.0) {
                    return Err(RenderError::Camera(
                        "the field of view is not between 0 and 180 degrees",
                    ));
                }
                let focal = 1.0 / (fov_y.to_radians() / 2.0).tan();
                if !focal.is_finite() {
                    return Err(RenderError::Camera(
                        "the field of view is too small to give a finite focal length",
                    ));
                }
                let distance = length(sub(camera.target, camera.eye));
                Ok(View::Perspective {
                    focal,
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
            Projection::Fisheye { .. } => Err(RenderError::Camera(
                "a fisheye lens has no single planar view",
            )),
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
                // Cut from the same end whichever way round the edge is met,
                // so two triangles sharing it get the same cut point.
                let (s, e) = if first(&p, &q) { (p, q) } else { (q, p) };
                let t = (near - z(s)) / (z(e) - z(s));
                let mut cut = s;
                for (c, d) in cut.iter_mut().zip(e) {
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
        let [
            x,
            y,
            z,
            r,
            g,
            b,
            u,
            w,
            kr,
            kg,
            kb,
            lx,
            ly,
            lz,
            px,
            py,
            pz,
            mx,
            my,
            mz,
        ] = v;
        let (nx, ny, key) = match *self {
            // Divided before multiplied, so an on-screen point's
            // intermediate values stay finite.
            View::Perspective { focal, aspect, .. } => {
                (x / z * (focal / aspect), y / z * focal, 1.0 / z)
            }
            View::Orthographic {
                half_height,
                aspect,
            } => (x / half_height / aspect, y / half_height, -z),
        };
        Corner {
            at: [
                (nx + 1.0) / 2.0 * f64::from(width),
                (1.0 - ny) / 2.0 * f64::from(height),
                key,
            ],
            shade: [r, g, b],
            uv: [u, w],
            key: [kr, kg, kb],
            light: [lx, ly, lz],
            model: [[px, py, pz], [mx, my, mz]],
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

/// Whether view corner `a`'s position comes before `b`'s, ordering x, then
/// y, then z.
fn first(a: &ViewCorner, b: &ViewCorner) -> bool {
    a.iter()
        .zip(b)
        .take(3)
        .map(|(p, q)| p.total_cmp(q))
        .find(|o| o.is_ne())
        .is_none_or(std::cmp::Ordering::is_lt)
}

/// A band of whole image rows from row `top`: `rgba` and `depth` hold the
/// band's pixels; `width` and `height` are the whole image's.
pub(crate) struct Target<'b> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) top: u32,
    pub(crate) rgba: &'b mut [u8],
    pub(crate) depth: &'b mut [f64],
}

/// What lights a triangle's pixels beyond its corners' shade.
#[derive(Clone, Copy, Default)]
pub(crate) struct PixelLights<'a> {
    /// The casting light's map.
    pub(crate) shadow: Option<&'a ShadowMap>,
    /// Whether the casting light is behind the seen face, so none of it
    /// reaches the seen side whatever the map holds.
    pub(crate) key_far: bool,
    /// Point and spot lights, added per pixel.
    pub(crate) local: &'a [Local],
    /// Their shadows.
    pub(crate) lamps: Option<&'a LampShadows>,
    /// The further directional lights' shadows.
    pub(crate) extra: Option<&'a ExtraShadows>,
    /// The triangle's unit normal, on the viewer's side of it.
    pub(crate) face: [f64; 3],
    /// The frame's finishes and the triangle's own, when it has one.
    pub(crate) gloss: Option<(&'a Gloss, &'a Resolved)>,
}

impl PixelLights<'_> {
    /// The casting light's lit share at map point `at`: 1 without a map, 0
    /// from behind the seen face.
    pub(crate) fn key_lit(&self, at: [f64; 3]) -> f64 {
        match self.shadow {
            Some(_) if self.key_far => 0.0,
            Some(map) => map.visibility(at),
            None => 1.0,
        }
    }
}

impl Target<'_> {
    /// Fill the screen triangle of `corners` with `paint`, depth-tested at
    /// pixel centres, the shade (and any texture coordinate,
    /// perspective-correct) interpolated across it. An opaque pixel
    /// replaces the pixel and its depth; a translucent one is blended over
    /// it, leaving the depth. With `shadow`, each pixel's `key` share of the
    /// shade is scaled by its visibility from the casting light. Each of
    /// `local` is added per pixel at the perspective-correct model point
    /// and normal, scaled by its visibility in `lamps` for a triangle with
    /// unit normal `face`.
    pub(crate) fn fill(
        &mut self,
        view: &View,
        corners: [Corner; 3],
        paint: Paint<'_>,
        lights: PixelLights<'_>,
    ) {
        let PixelLights {
            shadow,
            local,
            lamps,
            extra,
            face,
            ..
        } = lights;
        let [a, b, c] = corners;
        let area = edge(a.at, b.at, c.at);
        if !area.is_finite() || area.abs() < 1e-12 {
            return;
        }
        let Some((x0, x1, y0, y1)) = self.bounds(&corners) else {
            return;
        };
        let q = corners.map(|k| view.uv_weight(&k));
        let owned = owned_edges(&corners, area);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = [f64::from(x) + 0.5, f64::from(y) + 0.5, 0.0];
                let w = [
                    edge(b.at, c.at, p),
                    edge(c.at, a.at, p),
                    edge(a.at, b.at, p),
                ]
                .map(|e| e / area);
                if !covers(w, owned) {
                    continue;
                }
                let key: f64 = w.iter().zip(&corners).map(|(k, c)| k * at(c.at, 2)).sum();
                let i = (y - self.top) as usize * self.width as usize + x as usize;
                if self.depth.get(i).is_none_or(|&d| key <= d) {
                    continue;
                }
                let qw: f64 = w.iter().zip(q).map(|(k, q)| k * q).sum();
                // Every attribute but the depth key is perspective-correct.
                let correct = |f: &dyn Fn(&Corner) -> f64| {
                    let s: f64 = w
                        .iter()
                        .zip(&corners)
                        .zip(q)
                        .map(|((k, c), q)| k * q * f(c))
                        .sum();
                    s / qw
                };
                let mut shade = if a.shade == b.shade && b.shade == c.shade {
                    a.shade
                } else {
                    [0, 1, 2].map(|j| correct(&|c| at(c.shade, j)))
                };
                if shadow.is_some() && corners.iter().any(|c| c.key != [0.0; 3]) {
                    let lit = lights.key_lit([0, 1, 2].map(|j| correct(&|c| at(c.light, j))));
                    let key = [0, 1, 2].map(|j| correct(&|c| at(c.key, j)));
                    for (s, k) in shade.iter_mut().zip(key) {
                        *s += lit * k;
                    }
                }
                if !local.is_empty() || extra.is_some() {
                    let model =
                        |f: fn(&Corner) -> [f64; 3]| [0, 1, 2].map(|j| correct(&|c| at(f(c), j)));
                    let (p, n) = (model(|c| c.model[0]), model(|c| c.model[1]));
                    if let Some(extra) = extra {
                        extra.darken(&mut shade, p, n, face);
                    }
                    let lit = |i: usize| lamps.map_or(1.0, |m| m.visibility(i, p, face));
                    add_local(&mut shade, local, (p, n), &lit);
                }
                let colour = match paint.texture {
                    Some(t) => {
                        let uv = [0, 1].map(|j| correct(&|c| c.uv.get(j).copied().unwrap_or(0.0)));
                        t.apply(paint.colour, t.sample(uv))
                    }
                    None => paint.colour,
                };
                let out = match lights.gloss {
                    Some((g, f)) => {
                        g.pixel(f, (colour, shade), paint.texture.is_some(), &lights, &|f| {
                            [0, 1, 2].map(|j| correct(&|c| at(f(c), j)))
                        })
                    }
                    None => shaded(colour, shade),
                };
                self.blend(i, key, out);
            }
        }
    }

    /// The pixel rows and columns `corners` may cover, clipped to the
    /// band; `None` when off it.
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
        let rows = (self.depth.len() / self.width.max(1) as usize) as u32;
        let bottom = self.height.min(self.top.saturating_add(rows));
        let (x0, x1) = (lo(0), hi(0, self.width));
        let (y0, y1) = (lo(1).max(f64::from(self.top)), hi(1, bottom));
        (x0 <= x1 && y0 <= y1).then_some((x0 as u32, x1 as u32, y0 as u32, y1 as u32))
    }

    /// Write straight-RGBA `rgba` at pixel `i`: replacing it and its depth
    /// when opaque, blended over it otherwise.
    pub(crate) fn blend(&mut self, i: usize, key: f64, rgba: [u8; 4]) {
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

/// `colour`'s RGB scaled channel by channel by `shade`, saturating at 255;
/// alpha kept.
fn shaded(colour: [u8; 4], shade: [f64; 3]) -> [u8; 4] {
    let [r, g, b, a] = colour;
    let [sr, sg, sb] = shade;
    let [r, g, b] = [(r, sr), (g, sg), (b, sb)].map(|(v, s)| (f64::from(v) * s).round() as u8);
    [r, g, b, a]
}

/// Twice the signed area of (a, b, p) in the xy plane.
pub(crate) fn edge(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    // Two triangles sharing an edge meet it in opposite orders; evaluating
    // it in one canonical order makes their values exact negatives, so a
    // pixel centre on the edge cannot round out of both and leave a crack.
    if (at(a, 0), at(a, 1)) > (at(b, 0), at(b, 1)) {
        return -edge(b, a, p);
    }
    (at(b, 0) - at(a, 0)) * (at(p, 1) - at(a, 1)) - (at(b, 1) - at(a, 1)) * (at(p, 0) - at(a, 0))
}

/// Whether a pixel centre at barycentric weights `w` belongs to the
/// triangle: every weight positive, or zero on an edge it `owns`.
fn covers(w: [f64; 3], owns: [bool; 3]) -> bool {
    w.iter()
        .zip(owns)
        .all(|(&k, own)| k > 0.0 || (k == 0.0 && own) || k.is_nan())
}

/// The top-left fill rule: for each edge opposite corner 0, 1 and 2,
/// whether a pixel centre exactly on it is the triangle's. An edge is owned
/// when the triangle lies to its right (a left edge) or, horizontal, below
/// it (a top edge), with y pointing down the image. A neighbour sharing
/// the edge sees the exact negation ([`edge`]), so exactly one owns it.
fn owned_edges(corners: &[Corner; 3], area: f64) -> [bool; 3] {
    let [a, b, c] = corners.map(|k| k.at);
    [(b, c), (c, a), (a, b)].map(|(p, q)| {
        let [gx, gy] = edge_gradient(p, q).map(|g| g * area.signum());
        gx > 0.0 || (gx == 0.0 && gy > 0.0)
    })
}

/// The x and y gradient of [`edge`]`(a, b, ·)`, in the same canonical
/// order, so the two orders give exact negations.
fn edge_gradient(a: [f64; 3], b: [f64; 3]) -> [f64; 2] {
    if (at(a, 0), at(a, 1)) > (at(b, 0), at(b, 1)) {
        return edge_gradient(b, a).map(|g| -g);
    }
    [at(a, 1) - at(b, 1), at(b, 0) - at(a, 0)]
}

/// Component `i` of `v`, 0.0 out of range; the crate denies indexing.
pub(crate) fn at(v: [f64; 3], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{TextureFunction, TextureOrigin, TextureWrap};

    #[test]
    fn projection_divides_before_it_multiplies() {
        let corner = |x: f64, z: f64| {
            let mut c = [1.0; 20];
            c[..3].copy_from_slice(&[x, 0.0, z]);
            c
        };
        // x times the focal length overflows; x over z is on screen.
        let narrow = View::Perspective {
            focal: 1.1e300,
            aspect: 1.0,
            near: 1.0,
        };
        let at = narrow.project(corner(2e7, 1e308), 30, 10).at;
        assert!(at[0] > 15.0 && at[0] < 30.0, "{at:?}");
        // Half the height times the aspect ratio overflows.
        let tall = View::Orthographic {
            half_height: 7.5e307,
            aspect: 3.0,
        };
        let at = tall.project(corner(1e308, 0.0), 30, 10).at;
        assert!(
            (at[0] - 30.0 * (1.0 + 1.0 / 2.25) / 2.0).abs() < 1e-9,
            "{at:?}"
        );
        // ... or underflows to zero, and the centre divides 0 by 0.
        let thin = View::Orthographic {
            half_height: 1e-300,
            aspect: 1e-30,
        };
        assert_eq!(thin.project(corner(0.0, 0.0), 30, 10).at[0], 15.0);
    }

    #[test]
    fn a_shared_edge_is_cut_at_the_same_point_from_either_triangle() {
        // Cutting this edge from P towards Q and from Q towards P rounds
        // differently.
        let view = View::Perspective {
            focal: 1.0,
            aspect: 1.0,
            near: 0.5,
        };
        let corner = |x: f64, y: f64, z: f64| {
            let mut c = [1.0; 20];
            c[..3].copy_from_slice(&[x, y, z]);
            c
        };
        let (p, q) = (corner(0.1, 0.1, 0.2), corner(0.7, 0.2, 1.6));
        let one = view.clip([p, q, corner(0.0, 3.0, 2.0)]);
        let other = view.clip([q, p, corner(3.0, 0.0, 2.0)]);
        let shared = one
            .iter()
            .filter(|a| other.iter().any(|b| a[..3] == b[..3]))
            .count();
        assert_eq!(shared, 2, "Q and the cut on PQ: {one:?} {other:?}");
    }

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
            shade: [1.0; 3],
            uv: [u, 0.0],
            key: [0.0; 3],
            light: [0.0; 3],
            model: [[0.0; 3]; 2],
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
            let (mut rgba, mut depth) = (vec![0; 8 * 8 * 4], vec![f64::NEG_INFINITY; 64]);
            let mut target = Target {
                width: 8,
                height: 8,
                top: 0,
                rgba: &mut rgba,
                depth: &mut depth,
            };
            target.fill(view, corners, paint, PixelLights::default());
            rgba[4 * 4..4 * 4 + 4].to_vec()
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

    #[test]
    fn an_edge_seen_from_either_triangle_is_exactly_negated() {
        // So no pixel centre on a shared edge is rejected by both.
        let vals = [0.1, 0.7, 1.3, 2.9, 3.3, 5.5];
        for (a, b, p) in [
            ([0.1, 0.1], [0.7, 0.7], [0.1, 2.9]),
            ([5.5, 0.7], [1.3, 3.3], [2.9, 0.1]),
        ]
        .into_iter()
        .chain(vals.iter().map(|&v| ([v, 0.1], [0.7, v], [v, v])))
        {
            let [a, b, p] = [a, b, p].map(|[x, y]| [x, y, 0.0]);
            assert_eq!(edge(a, b, p), -edge(b, a, p), "{a:?} {b:?} {p:?}");
        }
    }

    fn plain(at: [f64; 3]) -> Corner {
        Corner {
            at,
            shade: [1.0; 3],
            uv: [0.0; 2],
            key: [0.0; 3],
            light: [0.0; 3],
            model: [[0.0; 3]; 2],
        }
    }

    /// `corners` filled into an empty 8 × 8 image under `view`.
    fn fill_all(
        view: &View,
        tris: &[[Corner; 3]],
        colour: [u8; 4],
        lights: PixelLights,
    ) -> Vec<u8> {
        let (mut rgba, mut depth) = (vec![0; 8 * 8 * 4], vec![f64::NEG_INFINITY; 64]);
        let mut target = Target {
            width: 8,
            height: 8,
            top: 0,
            rgba: &mut rgba,
            depth: &mut depth,
        };
        let paint = Paint {
            colour,
            texture: None,
        };
        for &t in tris {
            target.fill(view, t, paint, lights);
        }
        rgba
    }

    const ORTHO: View = View::Orthographic {
        half_height: 1.0,
        aspect: 1.0,
    };

    #[test]
    fn translucent_triangles_sharing_a_diagonal_blend_once() {
        // A 4 × 4 square split along a diagonal through pixel centres, in
        // both windings and both diagonals.
        let p = |x: f64, y: f64| plain([x, y, 1.0]);
        let squares = [
            [
                [p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0)],
                [p(0.0, 0.0), p(4.0, 4.0), p(0.0, 4.0)],
            ],
            [
                [p(0.0, 0.0), p(4.0, 4.0), p(4.0, 0.0)],
                [p(0.0, 4.0), p(4.0, 4.0), p(0.0, 0.0)],
            ],
            [
                [p(4.0, 0.0), p(0.0, 4.0), p(0.0, 0.0)],
                [p(4.0, 0.0), p(4.0, 4.0), p(0.0, 4.0)],
            ],
        ];
        for tris in squares {
            let rgba = fill_all(&ORTHO, &tris, [200, 100, 50, 128], PixelLights::default());
            let first = rgba[..4].to_vec();
            assert_eq!(first[3], 128);
            for y in 0..4 {
                for x in 0..4 {
                    let i = (y * 8 + x) * 4;
                    assert_eq!(rgba[i..i + 4], first[..], "pixel {x},{y}");
                }
            }
        }
    }

    /// A triangle whose corner 1 is three times as far as the others (depth
    /// key 1/3), carrying `lit`; pixel (3, 1)'s centre sits at screen
    /// weights (0.5, 0.375, 0.125), surface weight 1/6 for corner 1.
    fn far_corner(lit: impl Fn(&mut Corner)) -> [Corner; 3] {
        let mut far = plain([8.5, 0.5, 1.0 / 3.0]);
        let near = |x: f64, y: f64| {
            let mut c = plain([x, y, 1.0]);
            c.shade = [0.0; 3];
            c
        };
        let (a, c) = (near(0.5, 0.5), near(0.5, 8.5));
        far.shade = [0.0; 3];
        lit(&mut far);
        [a, far, c]
    }

    const PERSPECTIVE: View = View::Perspective {
        focal: 1.0,
        aspect: 1.0,
        near: 0.1,
    };

    #[test]
    fn shade_is_perspective_correct() {
        let tri = far_corner(|c| c.shade = [1.0; 3]);
        let rgba = fill_all(
            &PERSPECTIVE,
            &[tri],
            [240, 240, 240, 255],
            PixelLights::default(),
        );
        let i = (8 + 3) * 4;
        // 240 / 6, not 240 × 0.375 = 90.
        assert_eq!(rgba[i..i + 4], [40, 40, 40, 255]);
    }

    #[test]
    fn the_shadow_key_is_perspective_correct() {
        use crate::shadow::{LightView, ShadowMap};
        let bounds = crate::Bounds {
            min: [-1.0; 3],
            max: [1.0; 3],
        };
        let view = LightView::new([0.0, 0.0, 1.0], &bounds, 16).unwrap();
        let map = ShadowMap::build(view, &[], false, 1);
        // Every lookup point off the map, so fully lit.
        let mut tri = far_corner(|c| c.key = [1.0; 3]);
        for c in &mut tri {
            c.light = [-100.0, -100.0, 0.0];
        }
        let lights = PixelLights {
            shadow: Some(&map),
            ..PixelLights::default()
        };
        let rgba = fill_all(&PERSPECTIVE, &[tri], [240, 240, 240, 255], lights);
        let i = (8 + 3) * 4;
        assert_eq!(rgba[i..i + 4], [40, 40, 40, 255]);
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod camera_tests {
    use crate::{Bounds, Camera, Projection, RenderError, RenderOptions, TriangleMesh, render};

    fn triangle(scale: f64, z: f64) -> Vec<TriangleMesh> {
        vec![TriangleMesh {
            positions: vec![[-scale, -scale, z], [scale, -scale, z], [0.0, scale, z]],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        }]
    }

    fn camera(eye_z: f64, fov_y: f64) -> Camera {
        Camera {
            eye: [0.0, 0.0, eye_z],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            projection: Projection::Perspective { fov_y },
        }
    }

    const BACKGROUND: [u8; 4] = [1, 2, 3, 255];

    /// The centre pixel of `meshes` drawn 16 × 16 from `camera`.
    fn centre(meshes: &[TriangleMesh], camera: &Camera) -> [u8; 4] {
        let options = RenderOptions {
            width: 16,
            height: 16,
            background: BACKGROUND,
            ..RenderOptions::default()
        };
        let image = render(meshes, camera, &options).unwrap();
        let i = (8 * 16 + 8) * 4;
        image.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn a_subnormal_field_of_view_is_a_camera_error() {
        let meshes = triangle(1.0, 0.0);
        let cam = camera(-3.0, 1e-310);
        let options = RenderOptions {
            width: 8,
            height: 8,
            ..RenderOptions::default()
        };
        assert!(matches!(
            render(&meshes, &cam, &options),
            Err(RenderError::Camera(_))
        ));
        let bounds = Bounds::of(&meshes).unwrap();
        assert!(matches!(
            cam.projection_matrix(1.0, &bounds),
            Err(RenderError::Camera(_))
        ));
    }

    #[test]
    fn a_triangle_with_edges_near_1e160_is_drawn() {
        let meshes = triangle(1e160, 0.0);
        assert_ne!(centre(&meshes, &camera(-3e160, 60.0)), BACKGROUND);
    }

    #[test]
    fn a_triangle_nearer_than_the_near_plane_is_not_drawn() {
        // Eye-to-target distance 1, so the near plane is at 1e-4.
        let cam = camera(-1.0, 60.0);
        let at = |d: f64| triangle(1.0, -1.0 + d);
        assert_eq!(centre(&at(0.5e-4), &cam), BACKGROUND);
        assert_ne!(centre(&at(2e-4), &cam), BACKGROUND, "drawn past it");
    }
}
