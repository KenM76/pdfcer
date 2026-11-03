//! Headless rendering of triangle meshes to an RGBA image from a camera.
//!
//! A z-buffered scanline-free rasterizer: one sample per pixel centre,
//! lit per [`crate::LightingScheme`], no antialiasing. Faces are lit on
//! both sides because a producer's winding is not trusted to face outward.
//! Threads only through [`crate::parallel`]; no GUI dependency, so it runs
//! unchanged (single-threaded) on wasm32.

use crate::TriangleMesh;
use crate::ao::{self, Depths, Screen};
use crate::frame::Frame;
use crate::gloss::Gloss;
use crate::lamp_shadow::LampShadows;
use crate::lighting::{LightFrame, LightingScheme, Shader};
use crate::parallel;
use crate::raster::{Corner, Paint, PixelLights, Target, View, ViewCorner, at};
use crate::shadow::{Caster, DEFAULT_SHADOW_MAP_SIZE, LightSense, ShadowLight, ShadowMap};
use crate::shadow_cast::{ShadowAlpha, ShadowCasters};
use crate::shadow_set::{ExtraShadows, Shadows};
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
    pub(crate) fn basis(&self) -> Result<[[f64; 3]; 3], RenderError> {
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
    /// The colour of a surface with none of its own.
    pub colour: [u8; 3],
    /// How surfaces are lit. The default, [`LightingScheme::Artwork`], is
    /// the model's own lights, or one light at the camera when it has none.
    pub lighting: LightingScheme,
    /// Whether [`LightingScheme::Artwork`] adds the light at the camera to
    /// a model's own lights (off by default): for a model whose lights
    /// leave the side facing the viewer dark.
    pub artwork_headlight: bool,
    /// The frame a scheme's light directions are in.
    pub light_frame: LightFrame,
    /// Threads to draw with: 1 (the default) draws on the caller's thread,
    /// 0 uses every core. Ignored on wasm32. The image is the same at any
    /// count.
    pub threads: usize,
    /// Whether lights cast shadows (the default). The key light is the
    /// scheme's brightest light on the viewer's side, removing its share of
    /// the shade where something nearer that light is in the way; a scheme
    /// whose lights are all at the camera, such as the default, instead
    /// halves the shade where a fixed light above, left of and in front of
    /// the view is blocked. The other directional lights cast too, per
    /// [`shadow_casters`](Self::shadow_casters). No effect unlit.
    pub shadows: bool,
    /// Which way the scheme's light directions point, for shadows.
    pub light_sense: LightSense,
    /// Where the shadow-casting light is: one of the scheme's lights (the
    /// default) or a sun fixed in the model.
    pub shadow_light: ShadowLight,
    /// The shadow map's side, texels ([`DEFAULT_SHADOW_MAP_SIZE`](crate::DEFAULT_SHADOW_MAP_SIZE)), clamped
    /// to 64–[`MAX_SHADOW_MAP_SIZE`](crate::MAX_SHADOW_MAP_SIZE). It spans the model's bounding sphere,
    /// so a texel is the model's diameter / this.
    pub shadow_map_size: u32,
    /// Whether creases, corners and contacts are darkened by screen-space
    /// ambient occlusion (the default): each pixel keeps the share of its
    /// colour that nearby surfaces, within 4% of the model's bounding
    /// radius, leave open above it. No effect unlit.
    pub ambient_occlusion: bool,
    /// Whether a model's point and spot lights cast shadows too (the
    /// default), each lamp seeing the model through a cube of six depth
    /// maps around it. Only with [`RenderOptions::shadows`]. A lamp inside
    /// a closed mesh, such as a bulb in its shade, is shadowed by it.
    pub lamp_shadows: bool,
    /// Each lamp shadow cube face's side, texels
    /// ([`DEFAULT_LAMP_SHADOW_MAP_SIZE`](crate::DEFAULT_LAMP_SHADOW_MAP_SIZE)),
    /// clamped to 64–[`MAX_LAMP_SHADOW_MAP_SIZE`](crate::MAX_LAMP_SHADOW_MAP_SIZE).
    pub lamp_shadow_map_size: u32,
    /// Which directional lights cast ([`ShadowCasters::EveryLight`] by
    /// default, up to [`MAX_LIGHTS`](crate::MAX_LIGHTS), each shadow taking
    /// only its own light). Lights at the camera never cast.
    pub shadow_casters: ShadowCasters,
    /// How see-through and cut-out surfaces cast, in every map
    /// ([`ShadowAlpha::Transmit`] by default).
    pub shadow_alpha: ShadowAlpha,
    /// How a material's stored shininess becomes its highlight exponent
    /// ([`ShininessScale::Auto`](crate::ShininessScale::Auto)). Only a
    /// mesh with a [`crate::MaterialFinish`] draws highlights.
    pub shininess: crate::ShininessScale,
    /// Lines' width, pixels (1, the default): a model's wires
    /// ([`AssembledModel::lines`](crate::AssembledModel::lines)), drawn
    /// unlit and depth-tested. 0 draws as 1. Also the width of the edges a
    /// [`render_mode`](Self::render_mode) draws.
    pub line_width: u32,
    /// How faces, edges and vertices are drawn (ISO 32000-1 §13.6.4.4,
    /// `/RM`); [`RenderMode::default`](crate::RenderMode::default) is
    /// Solid, the frame this crate always drew.
    pub render_mode: crate::RenderMode,
}

impl Default for RenderOptions {
    /// 1024 × 768, opaque white background, light grey surface, the
    /// default light, one thread, shadows and ambient occlusion on.
    fn default() -> Self {
        RenderOptions {
            width: 1024,
            height: 768,
            background: [255, 255, 255, 255],
            colour: [190, 192, 200],
            lighting: LightingScheme::Artwork,
            artwork_headlight: false,
            light_frame: LightFrame::Camera,
            threads: 1,
            shadows: true,
            light_sense: LightSense::TowardsLight,
            shadow_light: ShadowLight::FollowsScheme,
            shadow_map_size: DEFAULT_SHADOW_MAP_SIZE,
            ambient_occlusion: true,
            lamp_shadows: true,
            lamp_shadow_map_size: crate::DEFAULT_LAMP_SHADOW_MAP_SIZE,
            shadow_casters: ShadowCasters::EveryLight,
            shadow_alpha: ShadowAlpha::Transmit,
            shininess: crate::ShininessScale::Auto,
            line_width: 1,
            render_mode: crate::RenderMode::default(),
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
/// Every triangle is filled in [`RenderOptions::colour`], lit per
/// [`RenderOptions::lighting`] by the angle between its face and each light;
/// nearer surfaces hide farther ones. Triangles with a non-finite or
/// out-of-range vertex are skipped. A perspective camera clips what lies behind it.
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
    let [r, g, b] = options.colour;
    let paints: Vec<Paint> = (0..meshes.len())
        .map(|i| Paint {
            colour: colours.get(i).copied().flatten().unwrap_or([r, g, b, 255]),
            texture: None,
        })
        .collect();
    let opaque: Vec<bool> = paints.iter().map(|p| p.colour[3] == 255).collect();
    render_painted((meshes, &[]), &paints, &opaque, camera, options)
}

/// Draws mesh `i` filled with `paints[i]`: meshes with `opaque[i]` first,
/// then the rest blended over them, both in mesh order; fully transparent
/// ones not at all. With [`RenderOptions::threads`] above one the image is
/// split into row bands drawn at once, each taking every triangle in the
/// same order, so the pixels are the same at any thread count.
pub(crate) fn render_painted(
    scene: (&[TriangleMesh], &[crate::ModelLight]),
    paints: &[Paint<'_>],
    opaque: &[bool],
    camera: &Camera,
    options: &RenderOptions,
) -> Result<Image, RenderError> {
    let (meshes, model_lights) = scene;
    let model = (meshes, model_lights, &[][..]);
    render_finished(model, paints, opaque, camera, options).map(|(image, _)| image)
}

/// [`render_painted`], each mesh drawn with its `finishes` entry, with each
/// pixel's depth key ([`View::project`]), `NEG_INFINITY` where nothing
/// opaque was drawn.
pub(crate) fn render_finished(
    (meshes, model_lights, finishes): crate::gloss::Model<'_>,
    paints: &[Paint<'_>],
    opaque: &[bool],
    camera: &Camera,
    options: &RenderOptions,
) -> Result<(Image, Vec<f64>), RenderError> {
    let (width, height) = (options.width, options.height);
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_RENDER_PIXELS {
        return Err(RenderError::Size { width, height });
    }
    let aspect = f64::from(width) / f64::from(height);
    let basis = camera.basis()?;
    let shader = Shader::for_frame(options, model_lights, &basis);
    let bounds = Bounds::of(meshes);
    let workers = parallel::workers(options.threads);
    let shadows = Shadows::build(
        (options, &shader, &basis),
        bounds.as_ref(),
        (meshes, paints, opaque),
        workers,
    );
    let view = View::new(camera, aspect)?;
    let gloss = Gloss::new(
        (meshes, model_lights, finishes),
        &shader,
        (camera, &view),
        options,
    )
    .map(|g| g.with_caster(shadows.caster.map(|c| c.kind)));
    let scene = Scene {
        camera,
        basis,
        view,
        shader,
        caster: shadows.caster,
        shadow: shadows.key,
        extra: shadows.extra,
        lamps: shadows.lamps,
        height,
        gloss,
    };
    let mut rgba = options.background.repeat(pixels as usize);
    let mut depth = vec![f64::NEG_INFINITY; pixels as usize];
    Frame {
        scene: &scene,
        meshes,
        paints,
        opaque,
    }
    .draw((&mut rgba, &mut depth), (width, height), workers);
    if let Some(bounds) = bounds.filter(|_| ambient_occlusion(options, &scene.shader)) {
        let depths = Depths {
            screen: Screen::new(&scene.view, width, height),
            keys: &depth,
        };
        ao::apply(
            &mut rgba,
            &depths,
            bounds.radius() * ao::RADIUS_SHARE,
            workers,
        );
    }
    Ok((
        Image {
            width,
            height,
            rgba,
        },
        depth,
    ))
}

/// Whether `options` asks for ambient occlusion and the frame is lit.
pub(crate) fn ambient_occlusion(options: &RenderOptions, shader: &Shader) -> bool {
    options.ambient_occlusion && !matches!(shader, Shader::Unlit)
}

/// The camera a frame is drawn from, with its basis and projection.
pub(crate) struct Scene<'c> {
    camera: &'c Camera,
    /// Right, up and forward.
    basis: [[f64; 3]; 3],
    view: View,
    shader: Shader,
    /// The light casting shadows and its map, when shadows are drawn.
    caster: Option<Caster>,
    shadow: Option<ShadowMap>,
    /// The further directional lights' shadows, when drawn.
    extra: Option<ExtraShadows>,
    /// The point and spot lights' shadows, when drawn.
    lamps: Option<LampShadows>,
    /// The image height, pixels.
    height: u32,
    /// The meshes' material finishes, when any draws one.
    gloss: Option<Gloss>,
}

impl Scene<'_> {
    /// The screen rows triangle `t` of `mesh` may cover, unclipped to the
    /// image; every row when part of it is behind the near plane, `None`
    /// when it is not drawn at all.
    pub(crate) fn rows(&self, mesh: &TriangleMesh, t: usize) -> Option<(f64, f64)> {
        let world = corners(mesh, mesh.triangles.get(t)?)?;
        let [right, up, forward] = self.basis;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for k in 0..3 {
            let d = sub(at3(&world, k), self.camera.eye);
            let v = crate::raster::view_point(dot(d, right), dot(d, up), dot(d, forward));
            if let View::Perspective { near, .. } = self.view
                && (dot(d, forward) < near || dot(d, forward).is_nan())
            {
                return Some((f64::NEG_INFINITY, f64::INFINITY));
            }
            let y = at(self.view.project(v, 1, self.height).at, 1);
            (lo, hi) = (lo.min(y), hi.max(y));
        }
        Some((lo, hi))
    }

    /// The shade at model point `p` with normal `n`, but for the point and
    /// spot lights [`Target::fill`] adds per pixel, and the share of it the
    /// shadow caster supplies.
    fn shade(&self, p: [f64; 3], n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let towards = match self.view {
            View::Perspective { .. } => normalize(sub(self.camera.eye, p)),
            View::Orthographic { .. } => Some(scale(at3(&self.basis, 2), -1.0)),
        };
        match &self.caster {
            Some(c) => c.split(&self.shader, n, towards),
            None => (self.shader.shade_far(n, towards), [0.0; 3]),
        }
    }

    /// Model point `p` with normal `n` of a triangle facing `face` in view
    /// space, with its texture coordinates, shade and shadow-map lookup
    /// point.
    fn view_corner(
        &self,
        (p, [nx, ny, nz]): ([f64; 3], [f64; 3]),
        face: [f64; 3],
        [u, v]: [f64; 2],
        ([r, g, b], [kr, kg, kb]): ([f64; 3], [f64; 3]),
    ) -> ViewCorner {
        let [right, up, forward] = self.basis;
        let d = sub(p, self.camera.eye);
        let [lx, ly, lz] = self
            .shadow
            .as_ref()
            .map_or([0.0; 3], |m| m.view.lookup_point(p, face));
        [
            dot(d, right),
            dot(d, up),
            dot(d, forward),
            r,
            g,
            b,
            u,
            v,
            kr,
            kg,
            kb,
            lx,
            ly,
            lz,
            p[0],
            p[1],
            p[2],
            nx,
            ny,
            nz,
        ]
    }

    /// Draw triangle `t` of `mesh` into `target`, lit and, when `paint` is
    /// textured, carrying the triangle's texture coordinates.
    pub(crate) fn draw(
        &self,
        target: &mut Target,
        mesh: &TriangleMesh,
        t: usize,
        paint: Paint<'_>,
    ) {
        let Some(world) = mesh.triangles.get(t).and_then(|tri| corners(mesh, tri)) else {
            return;
        };
        let Some(face) = normalize(cross(
            sub(at3(&world, 1), at3(&world, 0)),
            sub(at3(&world, 2), at3(&world, 0)),
        )) else {
            return;
        };
        let stored = stored_normals(mesh, t);
        let normal = |k: usize| {
            stored
                .and_then(|n| n.get(k).copied().flatten())
                .unwrap_or(face)
        };
        // Without stored normals the triangle is flat: one shade, lit along
        // the view ray to its first corner.
        let flat = stored
            .is_none()
            .then(|| self.shade(at3(&world, 0), normal(0)));
        let uvs = paint
            .texture
            .and_then(|tex| triangle_uvs(mesh, tex.uv_set, t));
        let view_corners = [0, 1, 2].map(|k| {
            let p = at3(&world, k);
            let uv = uvs.and_then(|c| c.get(k).copied()).unwrap_or([0.0; 2]);
            let lit = flat.unwrap_or_else(|| self.shade(p, normal(k)));
            self.view_corner((p, normal(k)), face, uv, lit)
        });
        let (width, height) = (target.width, target.height);
        let screen: Vec<Corner> = self
            .view
            .clip(view_corners)
            .into_iter()
            .map(|v| self.view.project(v, width, height))
            .collect();
        if let Some((&a, rest)) = screen.split_first() {
            for pair in rest.windows(2) {
                if let [b, c] = pair {
                    let lights = PixelLights {
                        shadow: self.shadow.as_ref(),
                        local: self.shader.local(),
                        lamps: self.lamps.as_ref(),
                        extra: self.extra.as_ref(),
                        face,
                        gloss: self
                            .gloss
                            .as_ref()
                            .and_then(|g| Some((g, g.finish_of(mesh)?))),
                    };
                    target.fill(&self.view, [a, *b, *c], paint, lights);
                }
            }
        }
    }
}

/// Triangle `t`'s texture coordinates in set `set`, or `None` when it has
/// none.
pub(crate) fn triangle_uvs(mesh: &TriangleMesh, set: usize, t: usize) -> Option<[[f64; 2]; 3]> {
    let slots = (*mesh.triangle_uvs.get(set)?.get(t)?)?;
    let mut out = [[0.0; 2]; 3];
    for (o, i) in out.iter_mut().zip(slots) {
        *o = *mesh.uvs.get(i as usize)?;
    }
    Some(out)
}

/// Triangle `t`'s stored corner normals, unit length; a corner whose normal
/// is zero or not finite is `None`, and so is the whole when the mesh
/// stores none.
pub(crate) fn stored_normals(mesh: &TriangleMesh, t: usize) -> Option<[Option<[f64; 3]>; 3]> {
    let slots = mesh.triangle_normals.get(t)?;
    Some(slots.map(|i| mesh.normals.get(i as usize).copied().and_then(normalize)))
}

/// The three corners of a triangle, or `None` when one is missing or not
/// finite.
pub(crate) fn corners(mesh: &TriangleMesh, tri: &[u32; 3]) -> Option<[[f64; 3]; 3]> {
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

/// Corner `i` of a triangle, the origin when out of range (the crate denies indexing).
pub(crate) fn at3(v: &[[f64; 3]; 3], i: usize) -> [f64; 3] {
    v.get(i).copied().unwrap_or([0.0; 3])
}

#[cfg(test)]
#[allow(clippy::unreachable)] // tests: an unreachable arm failing IS the report
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
            triangle_faces: Vec::new(),
            uvs: Vec::new(),
            triangle_uvs: Vec::new(),
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
            triangle_faces: Vec::new(),
            uvs: Vec::new(),
            triangle_uvs: Vec::new(),
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
            triangle_faces: Vec::new(),
            uvs: Vec::new(),
            triangle_uvs: Vec::new(),
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
            triangle_faces: Vec::new(),
            uvs: Vec::new(),
            triangle_uvs: Vec::new(),
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
            triangle_faces: Vec::new(),
            uvs: Vec::new(),
            triangle_uvs: Vec::new(),
        };
        let image = render(std::slice::from_ref(&mesh), &ortho(4.0), &small()).unwrap();
        assert!(image.rgba.chunks(4).all(|p| p == [255, 255, 255, 255]));
        assert_eq!(Bounds::of(&[mesh]).unwrap().max, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_scheme_lights_each_channel() {
        let lit = |lighting| {
            let options = RenderOptions {
                lighting,
                ..small()
            };
            let image = render(&[quad(0.0, 1.0, 0.0)], &ortho(4.0), &options).unwrap();
            pixel(&image, 20, 20)
        };
        assert_eq!(lit(LightingScheme::None), [190, 192, 200, 255]);
        // A face square to the camera: ambient .1 plus the camera light.
        assert_eq!(lit(LightingScheme::Headlamp), [171, 173, 200, 255]);
        assert_eq!(lit(LightingScheme::Artwork), [190, 192, 200, 255]);
        assert_eq!(lit(LightingScheme::Hard)[0], 255, "over-bright saturates");
    }

    #[test]
    fn every_thread_count_draws_the_same_pixels() {
        // Overlapping tilted squares, glass, and a floor crossing the near
        // plane, in a tall image so the bands are many.
        let mut meshes: Vec<TriangleMesh> = (0..12)
            .map(|k| {
                let mut q = quad(f64::from(k) * 0.3 - 2.0, 1.0 + f64::from(k) * 0.1, 0.4);
                for p in &mut q.positions {
                    p[1] += f64::from(k % 5) * 0.4 - 0.8;
                }
                q
            })
            .collect();
        meshes.push(TriangleMesh {
            positions: vec![[-5.0, -1.0, 20.0], [5.0, -1.0, 20.0], [0.0, -1.0, -20.0]],
            triangles: vec![[0, 1, 2]],
            ..quad(0.0, 1.0, 0.0)
        });
        let colours: Vec<_> = (0..meshes.len())
            .map(|k| {
                Some([
                    40 * (k % 6) as u8,
                    100,
                    200,
                    if k % 3 == 0 { 120 } else { 255 },
                ])
            })
            .collect();
        let camera = Camera {
            projection: Projection::Perspective { fov_y: 60.0 },
            ..ortho(1.0)
        };
        let draw = |threads| {
            let options = RenderOptions {
                width: 37,
                height: 301,
                lighting: LightingScheme::Night,
                threads,
                ..RenderOptions::default()
            };
            render_coloured(&meshes, &colours, &camera, &options).unwrap()
        };
        let one = draw(1);
        assert!(one.rgba.chunks(4).any(|p| p != [255, 255, 255, 255]));
        for threads in [2, 3, 7, 64, 0] {
            assert_eq!(draw(threads), one, "{threads} threads");
        }
    }

    #[test]
    fn a_lamp_lights_a_pool_on_one_large_face() {
        // No stored normals and four corners all far from the lamp: shaded
        // per corner, every pixel would get one shade.
        let lamp = crate::ModelLight::new(
            crate::ModelLightKind::Point {
                position: [0.0, 0.0, 1.0],
                attenuation: [0.0, 0.0, 1.0],
            },
            [1.0; 3],
        );
        let paint = Paint {
            colour: [100, 100, 100, 255],
            texture: None,
        };
        let image = render_painted(
            (&[quad(0.0, 1.0, 0.0)], &[lamp]),
            &[paint],
            &[true],
            &ortho(2.0),
            &small(),
        )
        .unwrap();
        let red = |x, y| pixel(&image, x, y)[0];
        assert!(
            red(20, 20) > red(2, 2) + 50,
            "{} {}",
            red(20, 20),
            red(2, 2)
        );
    }

    #[test]
    fn a_lamp_casts_a_card_s_shadow_on_the_floor() {
        // Lamp at (-1, 0, 2), card spanning x -0.5..0.5 at z 1: its shadow
        // on the floor spans x 0..2, the part past x 0.5 visible.
        let lamp = crate::ModelLight::new(
            crate::ModelLightKind::Point {
                position: [-1.0, 0.0, 2.0],
                attenuation: [1.0, 0.0, 0.0],
            },
            [1.0; 3],
        );
        let paint = Paint {
            colour: [200, 200, 200, 255],
            texture: None,
        };
        let draw = |lamp_shadows| {
            let options = RenderOptions {
                lamp_shadows,
                ambient_occlusion: false,
                ..small()
            };
            let meshes = [quad(0.0, 3.0, 0.0), quad(1.0, 0.5, 0.0)];
            render_painted(
                (&meshes, std::slice::from_ref(&lamp)),
                &[paint; 2],
                &[true; 2],
                &ortho(6.0),
                &options,
            )
            .unwrap()
        };
        let (on, off) = (draw(true), draw(false));
        let red = |image: &Image, x| pixel(image, x, 20)[0];
        // x = 1.3 is in the shadow, x = 2.6 past it, x = -2.5 behind the lamp.
        assert!(red(&on, 28) < 20 && red(&off, 28) > 100, "{}", red(&on, 28));
        assert_eq!(red(&on, 37), red(&off, 37));
        assert_eq!(red(&on, 3), red(&off, 3));
        // Off the shadow's edge the card's top is lit.
        assert_eq!(red(&on, 20), red(&off, 20));
    }
}
