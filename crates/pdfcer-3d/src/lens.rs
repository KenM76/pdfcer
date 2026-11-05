//! The fisheye lens ([`Projection::Fisheye`]): the map between a picture
//! pixel and a direction from the eye, and the planar views a lens picture
//! is composed from ([`FrameLens`]).
//!
//! Camera space here is x right, y up, z forward (the camera's basis); a
//! pixel is y down with pixel `(i, j)`'s centre at `(i + 0.5, j + 0.5)`. A
//! direction θ off the axis lands at r = f·g(θ) from the picture centre
//! ([`FisheyeMapping`]), f fixed by the field across the picture's height.

use crate::pick::Ray;
use crate::raster::View;
use crate::render::{Camera, FisheyeMapping, Projection, RenderError};
use crate::vec3::{add, dot, length, normalize, scale, sub};
use std::f64::consts::{FRAC_PI_2, PI};

/// How many straight pieces a line or wire segment is cut into under a
/// fisheye lens: each piece's ends are lens-projected exactly and the
/// piece is drawn straight between them, so a long line bends in up to
/// this many chords.
pub const LENS_LINE_PIECES: u32 = 16;

/// The largest side of a lens's 90° faces, pixels.
const MAX_FACE_SIDE: u32 = 4096;

/// A fisheye lens over a picture of a given size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Lens {
    pub(crate) mapping: FisheyeMapping,
    /// Pixels per unit of g(θ).
    pub(crate) focal: f64,
    width: f64,
    height: f64,
    /// The faces' near-plane distance: 1e-4 of the eye-to-target distance.
    pub(crate) near: f64,
}

/// The views a lens picture is composed from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Layout {
    /// One forward view reaching `tx`, `ty` (tangents) at its edges: the
    /// whole picture lies within 90° of the axis and inside a 90° square.
    Narrow { tx: f64, ty: f64 },
    /// Up to six 90° views of `side` pixels, one per cube face.
    Cube { side: u32 },
}

/// One planar view of a lens picture, drawn as an ordinary perspective
/// frame and resampled into the pixels it owns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Face {
    pub(crate) camera: Camera,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The owner code ([`Layout::owner`]).
    pub(crate) code: u8,
    /// The face's right, up and forward in the lens's camera space.
    pub(crate) axes: [[f64; 3]; 3],
    /// Its view's focal / aspect and focal ([`View::Perspective`]).
    pub(crate) scale: [f64; 2],
}

impl Lens {
    /// The lens of `camera` over a `width` × `height` picture, `None` for a
    /// planar camera.
    ///
    /// # Errors
    ///
    /// [`RenderError::FisheyeField`] for a field the lens's mapping does
    /// not draw, or one too small for a finite scale.
    pub(crate) fn for_camera(
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<Option<Lens>, RenderError> {
        let Projection::Fisheye { fov, mapping } = camera.projection else {
            return Ok(None);
        };
        let refused = RenderError::FisheyeField { mapping };
        if !mapping.accepts(fov) {
            return Err(refused);
        }
        let focal = f64::from(height) / 2.0 / mapping.radius(fov.to_radians() / 2.0);
        if !(focal.is_finite() && focal > 0.0) {
            return Err(refused);
        }
        let distance = length(sub(camera.target, camera.eye));
        Ok(Some(Lens {
            mapping,
            focal,
            width: f64::from(width),
            height: f64::from(height),
            near: (distance * 1e-4).max(f64::MIN_POSITIVE),
        }))
    }

    /// The unit camera-space direction drawn at picture position (`x`,
    /// `y`), `None` outside the lens's picture circle.
    pub(crate) fn direction(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        let (dx, dy) = (x - self.width / 2.0, self.height / 2.0 - y);
        let r = dx.hypot(dy);
        let theta = self
            .mapping
            .angle(r / self.focal)
            .filter(|t| t.is_finite())?;
        if r == 0.0 {
            return Some([0.0, 0.0, 1.0]);
        }
        let s = theta.sin() / r;
        Some([s * dx, s * dy, theta.cos()])
    }

    /// The picture position camera-space point `c` is drawn at; `None`
    /// straight behind the eye, at the eye, or not finite.
    pub(crate) fn pixel(&self, [x, y, z]: [f64; 3]) -> Option<[f64; 2]> {
        let (cx, cy) = (self.width / 2.0, self.height / 2.0);
        let rho = x.hypot(y);
        if rho == 0.0 {
            return (z > 0.0).then_some([cx, cy]);
        }
        let r = self.focal * self.mapping.radius(rho.atan2(z));
        let out = [cx + r * x / rho, cy - r * y / rho];
        out.iter().all(|c| c.is_finite()).then_some(out)
    }

    /// The angle off the axis at the picture's corners; `None` when they
    /// lie outside the picture circle.
    fn corner_angle(&self) -> Option<f64> {
        let rc = (self.width / 2.0).hypot(self.height / 2.0);
        self.mapping.angle(rc / self.focal)
    }

    /// The largest angle off the axis the picture shows.
    pub(crate) fn max_angle(&self) -> f64 {
        self.corner_angle().unwrap_or(PI)
    }

    /// The tangents the picture reaches across and up, when it lies within
    /// 90° of the axis: tan θ / r grows with r under both mappings, so the
    /// corner's ratio bounds every pixel.
    fn reach(&self) -> Option<(f64, f64)> {
        let theta = self.corner_angle().filter(|&t| t < FRAC_PI_2)?;
        let k = theta.tan() / (self.width / 2.0).hypot(self.height / 2.0);
        Some((k * self.width / 2.0, k * self.height / 2.0))
    }

    /// The views the picture is composed from. A cube face's side spends
    /// at its centre the lens's own pixels per radian at the widest angle
    /// the picture shows, so no face is coarser than the picture it fills;
    /// the side is capped at twice the picture's larger side and at 4096,
    /// which undersamples an equidistant picture's rim near 180° off the
    /// axis, where the lens spends ever more.
    pub(crate) fn layout(&self) -> Layout {
        if let Some((tx, ty)) = self.reach().filter(|&(tx, ty)| tx <= 1.0 && ty <= 1.0) {
            return Layout::Narrow { tx, ty };
        }
        let d = self.focal * self.mapping.density(self.max_angle());
        let cap = (2.0 * self.width.max(self.height)).min(f64::from(MAX_FACE_SIDE));
        Layout::Cube {
            side: (2.0 * d).ceil().clamp(1.0, cap) as u32,
        }
    }

    /// Whether the GPU host draws face `code` under `layout`: a superset of
    /// the faces that own a picture pixel, decided from the field alone.
    fn host_draws(&self, layout: Layout, code: u8) -> bool {
        let Layout::Cube { .. } = layout else {
            return true;
        };
        let reach = self.reach();
        match code {
            1 | 2 => reach.is_none_or(|(tx, _)| tx > 1.0),
            3 | 4 => reach.is_none_or(|(_, ty)| ty > 1.0),
            // A direction the back face owns is at least acos(-1/√3) off
            // the axis.
            5 => self.max_angle() >= (-1.0 / 3f64.sqrt()).acos(),
            _ => true,
        }
    }

    /// The ray [`Camera::ray_through`] gives at picture position (`x`,
    /// `y`): from the eye along the drawn direction, starting at the near
    /// plane of the face that owns it.
    pub(crate) fn ray(&self, camera: &Camera, x: f64, y: f64) -> Result<Ray, RenderError> {
        if !(x.is_finite() && y.is_finite()) {
            return Err(RenderError::Camera(
                "the image position is not a finite number",
            ));
        }
        let d = self.direction(x, y).ok_or(RenderError::OutsideLens)?;
        let [right, up, forward] = camera.basis()?;
        let world = add(
            add(scale(right, d[0]), scale(up, d[1])),
            scale(forward, d[2]),
        );
        let direction = normalize(world).ok_or(RenderError::Camera(
            "the image position is not a finite number",
        ))?;
        let along = match self.layout() {
            Layout::Narrow { .. } => d[2],
            Layout::Cube { .. } => d.iter().fold(0.0f64, |m, c| m.max(c.abs())),
        };
        Ok(Ray {
            origin: camera.eye,
            direction,
            min_distance: self.near / along,
        })
    }
}

impl Layout {
    /// The face drawing camera-space direction `d`: 0 forward, 1 right,
    /// 2 left, 3 up, 4 down, 5 back (the cube face its largest component
    /// points through, forward/back winning ties, then right/left).
    pub(crate) fn owner(&self, [x, y, z]: [f64; 3]) -> u8 {
        if let Layout::Narrow { .. } = self {
            return 0;
        }
        let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
        if az >= ax && az >= ay {
            if z > 0.0 { 0 } else { 5 }
        } else if ax >= ay {
            if x > 0.0 { 1 } else { 2 }
        } else if y > 0.0 {
            3
        } else {
            4
        }
    }
}

/// `layout`'s faces for `camera`'s `lens` whose code `wanted` accepts.
pub(crate) fn faces(
    camera: &Camera,
    lens: &Lens,
    layout: Layout,
    wanted: impl Fn(u8) -> bool,
) -> Result<Vec<Face>, RenderError> {
    let basis = camera.basis()?;
    let [r, u, f] = basis;
    let back = |v: [f64; 3]| scale(v, -1.0);
    let specs = match layout {
        Layout::Narrow { .. } => vec![(0, f, u)],
        Layout::Cube { .. } => vec![
            (0, f, u),
            (1, r, u),
            (2, back(r), u),
            (3, u, back(f)),
            (4, back(u), f),
            (5, back(f), u),
        ],
    };
    let (fov_y, width, height) = face_size(lens, layout);
    let distance = length(sub(camera.target, camera.eye));
    let mut out = Vec::new();
    for (code, axis, up) in specs.into_iter().filter(|s| wanted(s.0)) {
        let face_camera = Camera {
            eye: camera.eye,
            target: add(camera.eye, scale(axis, distance)),
            up,
            projection: Projection::Perspective { fov_y },
        };
        let face_basis = face_camera.basis()?;
        let View::Perspective { focal, aspect, .. } =
            View::new(&face_camera, f64::from(width) / f64::from(height))?
        else {
            return Err(RenderError::Camera("a lens face is not a perspective view"));
        };
        out.push(Face {
            camera: face_camera,
            width,
            height,
            code,
            axes: face_basis.map(|v| [dot(v, r), dot(v, u), dot(v, f)]),
            scale: [focal / aspect, focal],
        });
    }
    Ok(out)
}

/// A face's field of view (degrees), width and height under `layout`. The
/// narrow face spends f pixels per unit tangent at its centre, which
/// perspective's growth off the axis keeps above the lens's own density.
fn face_size(lens: &Lens, layout: Layout) -> (f64, u32, u32) {
    let max = f64::from(MAX_FACE_SIDE);
    match layout {
        Layout::Narrow { tx, ty } => {
            let ny = (2.0 * ty * lens.focal).ceil().clamp(1.0, max);
            let nx = (ny * tx / ty).ceil().clamp(1.0, max);
            ((2.0 * ty.atan()).to_degrees(), nx as u32, ny as u32)
        }
        Layout::Cube { side } => (90.0, side, side),
    }
}

/// What a GPU host needs to draw a [`Projection::Fisheye`] frame.
///
/// A lens picture is composed from ordinary perspective frames, one per
/// [`LensFace`]: the host draws each face exactly as it draws a planar
/// frame (prepass, shading, shadows, ambient occlusion) into an offscreen
/// colour and depth target of the face's size, from
/// [`LensFace::camera`]'s view and projection matrices and depth range,
/// **but with [`crate::FrameLights`], [`crate::FrameShadow`] and the other
/// shadow sets built from the fisheye camera**, so camera-fixed lights and
/// the shadow caster do not turn with each face. It then draws
/// [`crate::GLSL_ES_300_SCREEN_VERTEX`] with
/// [`crate::GLSL_ES_300_LENS_RESAMPLE_FRAGMENT`] once per face into the
/// picture, each pass writing only the pixels that face owns, with the
/// fisheye camera's depth range ([`Camera::depth_range`]). Lines and
/// points are drawn afterwards straight into the picture with
/// [`crate::GLSL_ES_300_LENS_LINE_VERTEX`] and
/// [`crate::GLSL_ES_300_LENS_POINT_VERTEX`], each segment instanced
/// [`LENS_LINE_PIECES`] times.
///
/// The CPU renderer ([`crate::render`]) composes the same faces and
/// matches this to the planar paths' tolerance.
///
/// **Cost.** A picture within 90° of the axis whose tangent reach fits a
/// 90° square is one face of about the picture's size. A wider one draws
/// up to six square faces (forward, the four sides when the picture
/// reaches them, the back past about 125° off the axis), each side twice
/// the lens's pixels per radian at the widest angle the picture shows,
/// capped at twice the picture's larger side and at 4096: up to six frames
/// of shading of up to (2·max(width, height))² pixels each, plus one
/// full-picture resample pass per face. The faces here
/// are a superset of those owning a pixel, chosen from the field alone;
/// one owning nothing costs only its draw time.
///
/// Ambient occlusion is computed per face and can differ slightly across
/// a face seam; nearest-texel resampling shows each face's own aliasing.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FrameLens {
    /// `u_lens_mapping`: 0 equidistant, 1 stereographic.
    pub mapping: i32,
    /// `u_lens_focal`: pixels per unit of g(θ), where a direction θ off the
    /// axis lands f·g(θ) from the picture centre.
    pub focal: f32,
    /// `u_lens_near`: the distance from the eye inside which a line or
    /// point is not drawn, model units.
    pub near: f32,
    /// The faces to draw, the forward face first.
    pub faces: Vec<LensFace>,
}

/// One perspective frame of a [`FrameLens`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LensFace {
    /// The face's perspective camera: the fisheye's eye, looking along one
    /// cube axis (or forward, for a single face).
    pub camera: Camera,
    /// The face's target width, pixels.
    pub width: u32,
    /// The face's target height, pixels.
    pub height: u32,
    /// `u_face`, the picture pixels this face owns: 0 forward, 1 right,
    /// 2 left, 3 up, 4 down, 5 back.
    pub face: u32,
    /// `u_face_right`, `u_face_up`, `u_face_forward`: the face's axes in
    /// the fisheye camera's right/up/forward coordinates.
    pub axes: [[f32; 3]; 3],
    /// `u_face_scale`: the face projection's x and y scale (a camera-space
    /// x/z of 1 lands this many half-widths right of centre).
    pub scale: [f32; 2],
}

impl FrameLens {
    /// The lens frame for a fisheye `camera` drawn `width` × `height`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Size`] for an empty image;
    /// [`RenderError::Camera`] for a planar camera, or one
    /// [`crate::render`] refuses; [`RenderError::FisheyeField`] for a
    /// field the lens does not draw.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_3d::{Camera, FisheyeMapping, FrameLens, Projection};
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Fisheye { fov: 180.0, mapping: FisheyeMapping::Equidistant },
    /// };
    /// let lens = FrameLens::new(&camera, 400, 300)?;
    /// assert_eq!(lens.faces[0].face, 0);
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    pub fn new(camera: &Camera, width: u32, height: u32) -> Result<FrameLens, RenderError> {
        if width == 0 || height == 0 {
            return Err(RenderError::Size { width, height });
        }
        let lens = Lens::for_camera(camera, width, height)?
            .ok_or(RenderError::Camera("the camera is not a fisheye"))?;
        let layout = lens.layout();
        let faces = faces(camera, &lens, layout, |c| lens.host_draws(layout, c))?;
        let narrow = |m: [[f64; 3]; 3]| m.map(|v| v.map(|c| c as f32));
        Ok(FrameLens {
            mapping: lens.mapping.code(),
            focal: lens.focal as f32,
            near: lens.near as f32,
            faces: faces
                .into_iter()
                .map(|f| LensFace {
                    camera: f.camera,
                    width: f.width,
                    height: f.height,
                    face: u32::from(f.code),
                    axes: narrow(f.axes),
                    scale: f.scale.map(|s| s as f32),
                })
                .collect(),
        })
    }
}
