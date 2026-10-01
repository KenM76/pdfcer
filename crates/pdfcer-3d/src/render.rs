//! Headless rendering of triangle meshes to an RGBA image from a camera.
//!
//! A z-buffered scanline-free rasterizer: one sample per pixel centre, flat
//! shading from a light at the camera, no antialiasing. Faces are lit on
//! both sides because a producer's winding is not trusted to face outward.
//! No threads and no GUI dependency, so it runs unchanged on wasm32.

use crate::TriangleMesh;
use crate::vec3::{cross, dot, length, normalize, scale, sub};

/// The largest image [`render`] draws, in pixels (width × height).
pub const MAX_RENDER_PIXELS: u64 = 64 * 1024 * 1024;

/// [`Camera::fit`]'s framed extent over the model's: 5% clear on each side.
const FIT_MARGIN: f64 = 1.1;

/// How the camera projects the scene onto the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// A pinhole camera with this vertical field of view, in degrees
    /// (exclusive range 0–180).
    Perspective {
        /// Vertical field of view, degrees.
        fov_y: f64,
    },
    /// A parallel projection showing this much of the scene vertically, in
    /// model units.
    Orthographic {
        /// Visible height, model units.
        height: f64,
    },
}

/// Where the camera is and where it looks, in model coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The camera position.
    pub eye: [f64; 3],
    /// The point at the image centre.
    pub target: [f64; 3],
    /// The direction that appears upward; must not be parallel to the view
    /// direction.
    pub up: [f64; 3],
    /// The projection.
    pub projection: Projection,
}

/// An axis-aligned box around every point a set of meshes draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// The smallest x, y and z.
    pub min: [f64; 3],
    /// The largest x, y and z.
    pub max: [f64; 3],
}

impl Bounds {
    /// The box around every finite vertex the meshes' triangles use, or
    /// `None` when there is none.
    ///
    /// ```
    /// use pdfcer_3d::{Bounds, TriangleMesh};
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 1.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// let b = Bounds::of(&[mesh]).unwrap();
    /// assert_eq!(b.max, [2.0, 3.0, 1.0]);
    /// ```
    pub fn of(meshes: &[TriangleMesh]) -> Option<Bounds> {
        let mut out: Option<Bounds> = None;
        for mesh in meshes {
            for tri in &mesh.triangles {
                for &i in tri {
                    let Some(&p) = mesh.positions.get(i as usize) else {
                        continue;
                    };
                    if !p.iter().all(|c| c.is_finite()) {
                        continue;
                    }
                    let b = out.get_or_insert(Bounds { min: p, max: p });
                    for ((lo, hi), c) in b.min.iter_mut().zip(b.max.iter_mut()).zip(p) {
                        *lo = lo.min(c);
                        *hi = hi.max(c);
                    }
                }
            }
        }
        out
    }

    /// The box's centre.
    pub fn centre(&self) -> [f64; 3] {
        [0, 1, 2].map(|i| (at(self.min, i) + at(self.max, i)) / 2.0)
    }

    /// Half the box's diagonal: the radius of a sphere holding it.
    pub fn radius(&self) -> f64 {
        length(sub(self.max, self.min)) / 2.0
    }
}

impl Camera {
    /// A camera looking along `direction` that fits `bounds` in an image of
    /// the given aspect ratio (width / height), with a 30° perspective or an
    /// orthographic projection.
    ///
    /// The box's eight corners are framed as they project from this
    /// direction, aimed at the box's centre, with about 5% clear on each
    /// side of the tighter axis (perspective: of the corner that limits it).
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] when `direction` is zero or parallel to
    /// `up`, or `aspect` is not positive.
    pub fn fit(
        bounds: &Bounds,
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        let corners = (0..8).map(|k| {
            [0, 1, 2].map(|i| {
                if k >> i & 1 == 0 {
                    at(bounds.min, i)
                } else {
                    at(bounds.max, i)
                }
            })
        });
        Camera::frame(bounds, corners, direction, up, perspective, aspect)
    }

    /// A camera looking along `direction` that fits every vertex the meshes'
    /// triangles use: like [`Camera::fit`], but framing the model itself
    /// rather than its box, so an oblique view is not padded by empty box
    /// corners. Aimed at the middle of the projected model.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] as for [`Camera::fit`], and when the meshes
    /// have no finite vertex.
    ///
    /// ```
    /// use pdfcer_3d::{Camera, TriangleMesh};
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// // Seen with x + y upward, the triangle sits low in its box.
    /// let camera = Camera::fit_meshes(&[mesh], [0.0, 0.0, -1.0], [1.0, 1.0, 0.0], false, 1.0)?;
    /// assert!(camera.target.iter().zip([0.5, 0.5, 0.0]).all(|(a, b)| (a - b).abs() < 1e-9));
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    pub fn fit_meshes(
        meshes: &[TriangleMesh],
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        let bounds = Bounds::of(meshes).ok_or(RenderError::Camera("the model has no vertex"))?;
        let vertices = meshes.iter().flat_map(|m| {
            m.triangles
                .iter()
                .flatten()
                .filter_map(|&i| m.positions.get(i as usize).copied())
                .filter(|p| p.iter().all(|c| c.is_finite()))
        });
        Camera::frame(&bounds, vertices, direction, up, perspective, aspect)
    }

    /// Frames `points` (which `bounds` holds) from `direction`, aimed at the
    /// middle of their projection.
    fn frame(
        bounds: &Bounds,
        points: impl Iterator<Item = [f64; 3]> + Clone,
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        if !(aspect.is_finite() && aspect > 0.0) {
            return Err(RenderError::Camera("the aspect ratio is not positive"));
        }
        let dir = normalize(direction).ok_or(RenderError::Camera("the view direction is zero"))?;
        let centre = bounds.centre();
        let radius = match bounds.radius() {
            r if r.is_finite() && r > 0.0 => r,
            _ => 1.0,
        };
        let [right, upward, forward] = Camera {
            eye: sub(centre, dir),
            target: centre,
            up,
            projection: Projection::Orthographic { height: 1.0 },
        }
        .basis()?;
        let view = |p: [f64; 3]| {
            let r = sub(p, centre);
            [dot(r, right), dot(r, upward), dot(r, forward)]
        };
        let ((x0, x1), (y0, y1)) = points.clone().map(view).fold(
            (
                (f64::INFINITY, f64::NEG_INFINITY),
                (f64::INFINITY, f64::NEG_INFINITY),
            ),
            |((a0, a1), (b0, b1)), [x, y, _]| ((a0.min(x), a1.max(x)), (b0.min(y), b1.max(y))),
        );
        if !(x0 <= x1 && y0 <= y1) {
            return Err(RenderError::Camera("the model has no vertex"));
        }
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let target = [0, 1, 2].map(|i| at(centre, i) + mx * at(right, i) + my * at(upward, i));
        let floor = radius * 1e-3;
        let (projection, distance) = if perspective {
            let fov_y: f64 = 30.0;
            let ty = (fov_y.to_radians() / 2.0).tan() / FIT_MARGIN;
            let tx = ty * aspect;
            // A point at lateral (x, y) from the target and depth f past it
            // is in view when |x| <= (d + f)·tx and |y| <= (d + f)·ty.
            let d = points.map(view).fold(floor, |d, [x, y, f]| {
                let need = ((x - mx).abs() / tx).max((y - my).abs() / ty);
                d.max(need - f).max(floor - f)
            });
            (Projection::Perspective { fov_y }, d)
        } else {
            let height = ((y1 - y0).max((x1 - x0) / aspect) * FIT_MARGIN).max(floor);
            (Projection::Orthographic { height }, 2.0 * radius)
        };
        let camera = Camera {
            eye: sub(target, scale(dir, distance)),
            target,
            up,
            projection,
        };
        camera.basis()?;
        Ok(camera)
    }

    /// Right, up and forward unit vectors.
    fn basis(&self) -> Result<[[f64; 3]; 3], RenderError> {
        let forward = normalize(sub(self.target, self.eye))
            .ok_or(RenderError::Camera("the eye and the target coincide"))?;
        let right = normalize(cross(forward, self.up)).ok_or(RenderError::Camera(
            "the up direction is zero or parallel to the view direction",
        ))?;
        Ok([right, cross(right, forward), forward])
    }
}

/// What [`render`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Image width, pixels.
    pub width: u32,
    /// Image height, pixels.
    pub height: u32,
    /// Background colour, straight RGBA.
    pub background: [u8; 4],
    /// The surface colour, lit by a light at the camera.
    pub colour: [u8; 3],
}

impl Default for RenderOptions {
    /// 1024 × 768, opaque white background, light grey surface.
    fn default() -> Self {
        RenderOptions {
            width: 1024,
            height: 768,
            background: [255, 255, 255, 255],
            colour: [190, 192, 200],
        }
    }
}

/// A rendered image: `rgba` holds `width * height` straight-alpha pixels,
/// row by row from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width, pixels.
    pub width: u32,
    /// Height, pixels.
    pub height: u32,
    /// The pixels.
    pub rgba: Vec<u8>,
}

/// Why [`render`] or [`Camera::fit`] refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    /// The image is empty or larger than [`MAX_RENDER_PIXELS`].
    #[error("a {width}x{height} image is empty or larger than the render ceiling")]
    Size {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The camera cannot form a view.
    #[error("invalid camera: {0}")]
    Camera(&'static str),
}

/// Draw `meshes` as seen by `camera`.
///
/// Every triangle is filled in [`RenderOptions::colour`], shaded by the
/// angle between its face and the direction to the camera; nearer surfaces
/// hide farther ones. Triangles with a non-finite or out-of-range vertex are
/// skipped. A perspective camera clips what lies behind it.
///
/// # Errors
///
/// [`RenderError::Size`] for an empty or over-ceiling image;
/// [`RenderError::Camera`] for a camera that cannot form a view (eye on the
/// target, `up` parallel to the view, a field of view outside 0–180°, a
/// non-positive orthographic height).
///
/// ```
/// use pdfcer_3d::{Bounds, Camera, RenderOptions, TriangleMesh, render};
/// let mut mesh = TriangleMesh::default();
/// mesh.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
/// mesh.triangles = vec![[0, 1, 2]];
/// let meshes = [mesh];
/// let bounds = Bounds::of(&meshes).unwrap();
/// let camera = Camera::fit(&bounds, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], false, 1.0)?;
/// let options = RenderOptions { width: 64, height: 64, ..RenderOptions::default() };
/// let image = render(&meshes, &camera, &options)?;
/// assert_eq!(image.rgba.len(), 64 * 64 * 4);
/// # Ok::<(), pdfcer_3d::RenderError>(())
/// ```
pub fn render(
    meshes: &[TriangleMesh],
    camera: &Camera,
    options: &RenderOptions,
) -> Result<Image, RenderError> {
    render_coloured(meshes, &[], camera, options)
}

/// Draw `meshes` as [`render`] does, mesh `i` in `colours[i]`: straight
/// RGBA, the alpha its opacity. A mesh with no entry, or `None`, is drawn
/// in [`RenderOptions::colour`], opaque.
///
/// Opaque meshes are drawn first. Translucent ones are then blended over
/// them, hidden by nearer opaque surfaces but not by each other, in mesh
/// order; fully transparent ones are not drawn.
///
/// # Errors
///
/// As [`render`].
///
/// ```
/// use pdfcer_3d::{Bounds, Camera, RenderOptions, TriangleMesh, render_coloured};
/// let mut mesh = TriangleMesh::default();
/// mesh.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
/// mesh.triangles = vec![[0, 1, 2]];
/// let meshes = [mesh];
/// let bounds = Bounds::of(&meshes).unwrap();
/// let camera = Camera::fit(&bounds, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], false, 1.0)?;
/// let options = RenderOptions { width: 64, height: 64, ..RenderOptions::default() };
/// let image = render_coloured(&meshes, &[Some([200, 0, 0, 255])], &camera, &options)?;
/// assert_eq!(image.rgba.len(), 64 * 64 * 4);
/// # Ok::<(), pdfcer_3d::RenderError>(())
/// ```
pub fn render_coloured(
    meshes: &[TriangleMesh],
    colours: &[Option<[u8; 4]>],
    camera: &Camera,
    options: &RenderOptions,
) -> Result<Image, RenderError> {
    let (width, height) = (options.width, options.height);
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_RENDER_PIXELS {
        return Err(RenderError::Size { width, height });
    }
    let [right, up, forward] = camera.basis()?;
    let aspect = f64::from(width) / f64::from(height);
    let view = View::new(camera, aspect)?;
    let mut target = Target {
        width,
        height,
        rgba: options.background.repeat(pixels as usize),
        depth: vec![f64::NEG_INFINITY; pixels as usize],
    };
    let to_view = |p: [f64; 3]| {
        let d = sub(p, camera.eye);
        [dot(d, right), dot(d, up), dot(d, forward)]
    };
    let [r, g, b] = options.colour;
    let colour_of = |i: usize| colours.get(i).copied().flatten().unwrap_or([r, g, b, 255]);
    for opaque_pass in [true, false] {
        for (i, mesh) in meshes.iter().enumerate() {
            let colour = colour_of(i);
            let alpha = colour[3];
            if alpha == 0 || (alpha == 255) != opaque_pass {
                continue;
            }
            for (t, tri) in mesh.triangles.iter().enumerate() {
                let Some(world) = corners(mesh, tri) else {
                    continue;
                };
                let Some(face) = normalize(cross(
                    sub(at3(&world, 1), at3(&world, 0)),
                    sub(at3(&world, 2), at3(&world, 0)),
                )) else {
                    continue;
                };
                let stored = stored_normals(mesh, t);
                let shade = |k: usize| {
                    let p = at3(&world, k);
                    let towards = match view {
                        View::Perspective { .. } => normalize(sub(camera.eye, p)),
                        View::Orthographic { .. } => Some(scale(forward, -1.0)),
                    };
                    let normal = stored
                        .and_then(|n| n.get(k).copied().flatten())
                        .unwrap_or(face);
                    0.3 + 0.7 * towards.map_or(0.0, |l| dot(normal, l).abs())
                };
                // Without stored normals the triangle is flat: one shade,
                // lit along the view ray to its first corner.
                let flat = stored.is_none().then(|| shade(0));
                let shaded = [0, 1, 2].map(|k| {
                    let [x, y, z] = to_view(at3(&world, k));
                    [x, y, z, flat.unwrap_or_else(|| shade(k))]
                });
                let screen: Vec<([f64; 3], f64)> = view
                    .clip(shaded)
                    .iter()
                    .map(|&[x, y, z, s]| (view.project([x, y, z], width, height), s))
                    .collect();
                if let Some((&a, rest)) = screen.split_first() {
                    for pair in rest.windows(2) {
                        if let [b, c] = pair {
                            target.fill([a, *b, *c], colour);
                        }
                    }
                }
            }
        }
    }
    Ok(Image {
        width,
        height,
        rgba: target.rgba,
    })
}

/// Triangle `t`'s stored corner normals, unit length; a corner whose normal
/// is zero or not finite is `None`, and so is the whole when the mesh
/// stores none.
fn stored_normals(mesh: &TriangleMesh, t: usize) -> Option<[Option<[f64; 3]>; 3]> {
    let slots = mesh.triangle_normals.get(t)?;
    Some(slots.map(|i| mesh.normals.get(i as usize).copied().and_then(normalize)))
}

/// The three corners of a triangle, or `None` when one is missing or not
/// finite.
fn corners(mesh: &TriangleMesh, tri: &[u32; 3]) -> Option<[[f64; 3]; 3]> {
    let mut out = [[0.0; 3]; 3];
    for (slot, &i) in out.iter_mut().zip(tri) {
        let p = *mesh.positions.get(i as usize)?;
        if !p.iter().all(|c| c.is_finite()) {
            return None;
        }
        *slot = p;
    }
    Some(out)
}

enum View {
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
    fn new(camera: &Camera, aspect: f64) -> Result<View, RenderError> {
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

    /// The part of a view-space triangle in front of the near plane
    /// (Sutherland–Hodgman against one plane): 0, 3 or 4 points.
    /// The part of `tri` (view x, y, z and a shade) in front of the near
    /// plane, the shade interpolated along each cut edge.
    fn clip(&self, tri: [[f64; 4]; 3]) -> Vec<[f64; 4]> {
        let View::Perspective { near, .. } = *self else {
            return tri.to_vec();
        };
        let mut out = Vec::with_capacity(4);
        for (k, &p) in tri.iter().enumerate() {
            let q = tri.get((k + 1) % 3).copied().unwrap_or(p);
            let z = |v: [f64; 4]| v.get(2).copied().unwrap_or(0.0);
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

    /// Pixel x, pixel y and a depth key that is linear in screen space and
    /// larger for nearer points.
    fn project(&self, p: [f64; 3], width: u32, height: u32) -> [f64; 3] {
        let (nx, ny, key) = match *self {
            View::Perspective { focal, aspect, .. } => {
                let z = at(p, 2);
                (
                    at(p, 0) * focal / (z * aspect),
                    at(p, 1) * focal / z,
                    1.0 / z,
                )
            }
            View::Orthographic {
                half_height,
                aspect,
            } => (
                at(p, 0) / (half_height * aspect),
                at(p, 1) / half_height,
                -at(p, 2),
            ),
        };
        [
            (nx + 1.0) / 2.0 * f64::from(width),
            (1.0 - ny) / 2.0 * f64::from(height),
            key,
        ]
    }
}

struct Target {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    depth: Vec<f64>,
}

impl Target {
    /// Fill one screen-space triangle, depth-tested, sampling pixel centres.
    /// An opaque colour replaces the pixel and its depth; a translucent one
    /// is blended over it, leaving the depth.
    /// Fill the screen triangle of `corners` (pixel x, y, depth key; and
    /// a shade), the shade interpolated across it and applied to `colour`.
    fn fill(&mut self, corners: [([f64; 3], f64); 3], colour: [u8; 4]) {
        let [(a, sa), (b, sb), (c, sc)] = corners;
        let area = edge(a, b, c);
        if !area.is_finite() || area.abs() < 1e-12 {
            return;
        }
        let lo = |i: usize| at(a, i).min(at(b, i)).min(at(c, i)).floor().max(0.0);
        let hi = |i: usize, limit: u32| {
            at(a, i)
                .max(at(b, i))
                .max(at(c, i))
                .ceil()
                .min(f64::from(limit) - 1.0)
        };
        let (x0, x1, y0, y1) = (lo(0), hi(0, self.width), lo(1), hi(1, self.height));
        if x0 > x1 || y0 > y1 {
            return;
        }
        let (x0, x1, y0, y1) = (x0 as u32, x1 as u32, y0 as u32, y1 as u32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = [f64::from(x) + 0.5, f64::from(y) + 0.5, 0.0];
                let (w0, w1, w2) = (
                    edge(b, c, p) / area,
                    edge(c, a, p) / area,
                    edge(a, b, p) / area,
                );
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let key = w0 * at(a, 2) + w1 * at(b, 2) + w2 * at(c, 2);
                let i = y as usize * self.width as usize + x as usize;
                let Some(d) = self.depth.get_mut(i) else {
                    continue;
                };
                if key <= *d {
                    continue;
                }
                let shade = if sa == sb && sb == sc {
                    sa
                } else {
                    w0 * sa + w1 * sb + w2 * sc
                };
                let [cr, cg, cb, alpha] = colour;
                let [r, g, b] = [cr, cg, cb].map(|v| (f64::from(v) * shade).round() as u8);
                let rgba = [r, g, b, alpha];
                let opaque = alpha == 255;
                if opaque {
                    *d = key;
                }
                if let Some(px) = self.rgba.get_mut(i * 4..i * 4 + 4) {
                    if opaque {
                        px.copy_from_slice(&rgba);
                    } else if let [pr, pg, pb, pa] = px {
                        let [sr, sg, sb, sa] = rgba;
                        let src = f64::from(sa) / 255.0;
                        let dst = f64::from(*pa) / 255.0 * (1.0 - src);
                        let out = src + dst;
                        for (p, s) in [pr, pg, pb].into_iter().zip([sr, sg, sb]) {
                            *p = ((f64::from(s) * src + f64::from(*p) * dst) / out).round() as u8;
                        }
                        *pa = (out * 255.0).round() as u8;
                    }
                }
            }
        }
    }
}

/// Twice the signed area of (a, b, p) in the xy plane.
fn edge(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    (at(b, 0) - at(a, 0)) * (at(p, 1) - at(a, 1)) - (at(b, 1) - at(a, 1)) * (at(p, 0) - at(a, 0))
}

fn at(v: [f64; 3], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

fn at3(v: &[[f64; 3]; 3], i: usize) -> [f64; 3] {
    v.get(i).copied().unwrap_or([0.0; 3])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly by design.
#[allow(clippy::single_range_in_vec_init)] // One face spanning every triangle is intended.
mod tests {
    use super::*;

    fn quad(z: f64, half: f64, tilt: f64) -> TriangleMesh {
        TriangleMesh {
            positions: vec![
                [-half, -half, z - tilt],
                [half, -half, z + tilt],
                [half, half, z + tilt],
                [-half, half, z - tilt],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            faces: vec![0..2],
            normals_recalculated: true,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
        }
    }

    fn ortho(height: f64) -> Camera {
        Camera {
            eye: [0.0, 0.0, 10.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            projection: Projection::Orthographic { height },
        }
    }

    fn small() -> RenderOptions {
        RenderOptions {
            width: 40,
            height: 40,
            ..RenderOptions::default()
        }
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = (y * image.width + x) as usize * 4;
        image.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn a_facing_square_fills_the_middle_and_leaves_the_corners() {
        let image = render(&[quad(0.0, 1.0, 0.0)], &ortho(4.0), &small()).unwrap();
        assert_eq!(pixel(&image, 20, 20), [190, 192, 200, 255]);
        assert_eq!(pixel(&image, 1, 1), [255, 255, 255, 255]);
        // Half the view height is covered: pixels 10..30.
        assert_eq!(pixel(&image, 10, 20)[0], 190);
        assert_eq!(pixel(&image, 9, 20)[0], 255);
    }

    #[test]
    fn stored_normals_shade_smoothly_across_a_flat_square() {
        let mut mesh = quad(0.0, 1.0, 0.0);
        let s = 3f64.sqrt() / 2.0;
        // The left corners lean 60° away from the viewer; the right face it.
        mesh.normals = vec![[s, 0.0, 0.5], [0.0, 0.0, 2.0]];
        mesh.triangle_normals = vec![[0, 1, 1], [0, 1, 0]];
        let image = render(&[mesh.clone()], &ortho(4.0), &small()).unwrap();
        let red = |x| pixel(&image, x, 20)[0];
        assert!(
            red(11) < red(20) && red(20) < red(29),
            "{} {} {}",
            red(11),
            red(20),
            red(29)
        );
        // Shade 0.3 + 0.7 cos θ at the edges: 0.65 left, 1.0 right.
        assert!((i32::from(red(10)) - 128).abs() <= 4, "{}", red(10));
        assert!(red(29) >= 186);
        mesh.normals.clear();
        mesh.triangle_normals.clear();
        let flat = render(&[mesh], &ortho(4.0), &small()).unwrap();
        assert_eq!(pixel(&flat, 11, 20), pixel(&flat, 29, 20));
    }

    #[test]
    fn the_nearer_surface_hides_the_farther_whatever_the_order() {
        let far = quad(-1.0, 2.0, 1.0);
        let near = quad(1.0, 0.5, 0.0);
        for meshes in [[far.clone(), near.clone()], [near, far]] {
            let image = render(&meshes, &ortho(8.0), &small()).unwrap();
            assert_eq!(pixel(&image, 20, 20), [190, 192, 200, 255]);
            assert!(
                pixel(&image, 20, 12)[0] < 190,
                "the tilted far square is darker"
            );
        }
    }

    const RED: Option<[u8; 4]> = Some([255, 0, 0, 255]);
    const GLASS: Option<[u8; 4]> = Some([0, 0, 255, 102]);

    /// Glass (40% blue) in front of red blends whichever is listed first,
    /// glass behind red is hidden, a clear mesh draws nothing, and a mesh
    /// with no colour takes the options' colour.
    #[test]
    fn meshes_draw_in_their_own_colours_and_glass_blends() {
        let (near, far) = (quad(1.0, 1.0, 0.0), quad(-1.0, 1.0, 0.0));
        let at_middle = |meshes: &[TriangleMesh], colours: &[Option<[u8; 4]>]| {
            let image = render_coloured(meshes, colours, &ortho(4.0), &small()).unwrap();
            pixel(&image, 20, 20)
        };
        let two = [near.clone(), far.clone()];
        assert_eq!(at_middle(&two, &[GLASS, RED]), [153, 0, 102, 255]);
        let flipped = [far.clone(), near.clone()];
        assert_eq!(at_middle(&flipped, &[RED, GLASS]), [153, 0, 102, 255]);
        assert_eq!(at_middle(&flipped, &[GLASS, RED]), [255, 0, 0, 255]);
        assert_eq!(
            at_middle(&two, &[Some([0, 0, 255, 0]), RED]),
            [255, 0, 0, 255]
        );
        assert_eq!(at_middle(&two, &[None]), [190, 192, 200, 255]);
        let panes = [quad(2.0, 1.0, 0.0), near.clone(), far.clone()];
        assert_eq!(
            at_middle(&panes, &[GLASS, GLASS, RED]),
            [92, 0, 163, 255],
            "glass does not hide glass"
        );
        let clear = RenderOptions {
            background: [0, 0, 0, 0],
            ..small()
        };
        let image = render_coloured(&[near], &[GLASS], &ortho(4.0), &clear).unwrap();
        assert_eq!(
            pixel(&image, 20, 20),
            [0, 0, 255, 102],
            "glass over nothing"
        );
    }

    #[test]
    fn each_edge_bounds_a_triangle() {
        let tri = TriangleMesh {
            positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            faces: vec![0..1],
            normals_recalculated: true,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
        };
        let image = render(&[tri], &ortho(4.0), &small()).unwrap();
        assert_ne!(pixel(&image, 20, 25)[0], 255);
        // Just past each edge: below the base, left and right of the apex.
        for (x, y) in [(20, 31), (13, 15), (27, 15)] {
            assert_eq!(pixel(&image, x, y)[0], 255, "({x},{y})");
        }
    }

    #[test]
    fn a_face_turned_away_is_still_drawn() {
        let mut back = quad(0.0, 1.0, 0.0);
        for t in &mut back.triangles {
            t.swap(1, 2);
        }
        let image = render(&[back], &ortho(4.0), &small()).unwrap();
        assert_eq!(pixel(&image, 20, 20), [190, 192, 200, 255]);
    }

    #[test]
    fn perspective_clips_what_is_behind_the_camera() {
        let camera = Camera {
            projection: Projection::Perspective { fov_y: 60.0 },
            ..ortho(1.0)
        };
        let behind = quad(20.0, 1.0, 0.0);
        let image = render(&[behind], &camera, &small()).unwrap();
        assert!(image.rgba.chunks(4).all(|p| p == [255, 255, 255, 255]));
        // A floor running from behind the camera to beyond the target.
        let floor = TriangleMesh {
            positions: vec![[-5.0, -1.0, 20.0], [5.0, -1.0, 20.0], [0.0, -1.0, -20.0]],
            triangles: vec![[0, 1, 2]],
            faces: vec![0..1],
            normals_recalculated: true,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
        };
        let image = render(&[floor], &camera, &small()).unwrap();
        assert_ne!(pixel(&image, 20, 38), [255, 255, 255, 255]);
        assert_eq!(pixel(&image, 20, 2), [255, 255, 255, 255]);
    }

    #[test]
    fn a_fitted_camera_frames_the_model() {
        // Depth matters in perspective: the nearer square looks larger.
        let meshes = [quad(3.0, 3.0, 0.0), quad(-3.0, 3.0, 0.0)];
        let bounds = Bounds::of(&meshes).unwrap();
        for perspective in [true, false] {
            let camera =
                Camera::fit(&bounds, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], perspective, 2.0).unwrap();
            let options = RenderOptions {
                width: 80,
                height: 40,
                ..RenderOptions::default()
            };
            let image = render(&meshes, &camera, &options).unwrap();
            assert_ne!(pixel(&image, 40, 20)[0], 255);
            // Nothing drawn touches the image border.
            for x in 0..80 {
                assert_eq!(pixel(&image, x, 0)[0], 255);
                assert_eq!(pixel(&image, x, 39)[0], 255);
            }
        }
    }

    #[test]
    fn a_mesh_fit_ignores_empty_box_corners() {
        // A thin strip along the box's diagonal: seen across the diagonal,
        // the box is as tall as it is wide, the strip is not.
        let strip = TriangleMesh {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 10.0, 0.0], [10.0, 10.01, 0.0]],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        };
        let meshes = [strip];
        let (dir, up) = ([0.0, 0.0, -1.0], [1.0, -1.0, 0.0]);
        let height = |c: Camera| match c.projection {
            Projection::Orthographic { height } => height,
            Projection::Perspective { .. } => unreachable!(),
        };
        let boxed =
            height(Camera::fit(&Bounds::of(&meshes).unwrap(), dir, up, false, 2.0).unwrap());
        let tight = height(Camera::fit_meshes(&meshes, dir, up, false, 2.0).unwrap());
        assert!((boxed / tight - 2.0).abs() < 0.01, "{boxed} vs {tight}");
        let near = Camera::fit_meshes(&meshes, dir, up, true, 2.0).unwrap();
        let far = Camera::fit(&Bounds::of(&meshes).unwrap(), dir, up, true, 2.0).unwrap();
        assert!(length(sub(near.eye, near.target)) < 0.6 * length(sub(far.eye, far.target)));

        // Lopsided in its box: the limiting vertex sits exactly on the
        // margin, measured from the re-centred target.
        let tri = TriangleMesh {
            positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 2.0, 1.0]],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        };
        // Narrow (width limits) and wide (height limits).
        for aspect in [0.5, 3.0] {
            let c = Camera::fit_meshes(
                std::slice::from_ref(&tri),
                [0.0, 0.3, -1.0],
                [1.0, 1.0, 0.0],
                true,
                aspect,
            )
            .unwrap();
            let [right, upward, forward] = c.basis().unwrap();
            let ty = 15f64.to_radians().tan() / FIT_MARGIN;
            let fill = tri
                .positions
                .iter()
                .map(|&p| {
                    let r = sub(p, c.eye);
                    let z = dot(r, forward);
                    (dot(r, right).abs() / (ty * aspect)).max(dot(r, upward).abs() / ty) / z
                })
                .fold(0.0, f64::max);
            assert!((fill - 1.0).abs() < 1e-6, "aspect {aspect}: {fill}");
        }
    }

    #[test]
    fn a_fitted_camera_fills_the_tighter_axis() {
        let drawn = |image: &Image, horizontal: bool, line: u32| {
            let n = if horizontal {
                image.width
            } else {
                image.height
            };
            (0..n)
                .filter(|&i| {
                    let (x, y) = if horizontal { (i, line) } else { (line, i) };
                    pixel(image, x, y)[0] != 255
                })
                .count()
        };
        // Height-limited: a 6 x 6 square in an 80 x 40 image.
        let square = [quad(0.0, 3.0, 0.0)];
        // Width-limited and off-origin: a 10 x 1 strip in a 40 x 40 image.
        let strip = [TriangleMesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [10.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            faces: vec![0..2],
            normals_recalculated: true,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
        }];
        for perspective in [true, false] {
            let fit = |meshes: &[TriangleMesh], aspect| {
                let bounds = Bounds::of(meshes).unwrap();
                Camera::fit(
                    &bounds,
                    [0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0],
                    perspective,
                    aspect,
                )
                .unwrap()
            };
            let wide = RenderOptions {
                width: 80,
                height: 40,
                ..RenderOptions::default()
            };
            let image = render(&square, &fit(&square, 2.0), &wide).unwrap();
            let rows = drawn(&image, false, 40);
            assert!(
                (34..=37).contains(&rows),
                "{perspective}: {rows} of 40 rows"
            );

            let image = render(&strip, &fit(&strip, 1.0), &small()).unwrap();
            let cols = drawn(&image, true, 20);
            assert!(
                (34..=37).contains(&cols),
                "{perspective}: {cols} of 40 columns"
            );
            // Centred: the margins differ by at most a pixel.
            let left = (0..40)
                .take_while(|&x| pixel(&image, x, 20)[0] == 255)
                .count();
            assert!(
                left.abs_diff(40 - cols - left) <= 1,
                "{perspective}: left {left}"
            );
        }
    }

    #[test]
    fn bad_sizes_and_cameras_are_refused() {
        let m = [quad(0.0, 1.0, 0.0)];
        let size = |width, height| RenderOptions {
            width,
            height,
            ..RenderOptions::default()
        };
        assert!(matches!(
            render(&m, &ortho(1.0), &size(0, 10)),
            Err(RenderError::Size { .. })
        ));
        assert!(matches!(
            render(&m, &ortho(1.0), &size(10_000, 10_000)),
            Err(RenderError::Size { .. })
        ));
        let bad = [
            Camera {
                eye: [0.0; 3],
                target: [0.0; 3],
                ..ortho(1.0)
            },
            Camera {
                up: [0.0, 0.0, 1.0],
                ..ortho(1.0)
            },
            Camera {
                projection: Projection::Perspective { fov_y: 180.0 },
                ..ortho(1.0)
            },
            ortho(0.0),
        ];
        for camera in bad {
            assert!(matches!(
                render(&m, &camera, &small()),
                Err(RenderError::Camera(_))
            ));
        }
        let b = Bounds::of(&m).unwrap();
        assert!(Camera::fit(&b, [0.0; 3], [0.0, 1.0, 0.0], true, 1.0).is_err());
        assert!(Camera::fit(&b, [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], true, 0.0).is_err());
    }

    #[test]
    fn non_finite_and_missing_vertices_are_skipped() {
        let mesh = TriangleMesh {
            positions: vec![[0.0, 0.0, 0.0], [f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2], [0, 2, 9]],
            faces: vec![0..2],
            normals_recalculated: true,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
        };
        let image = render(std::slice::from_ref(&mesh), &ortho(4.0), &small()).unwrap();
        assert!(image.rgba.chunks(4).all(|p| p == [255, 255, 255, 255]));
        assert_eq!(Bounds::of(&[mesh]).unwrap().max, [0.0, 1.0, 0.0]);
    }
}
