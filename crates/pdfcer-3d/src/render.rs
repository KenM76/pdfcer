//! Headless rendering of triangle meshes to an RGBA image from a camera.
//!
//! A z-buffered scanline-free rasterizer: one sample per pixel centre,
//! lit per [`crate::LightingScheme`], no antialiasing. Faces are lit on
//! both sides because a producer's winding is not trusted to face outward.
//! Threads only through [`crate::render::parallel`]; no GUI dependency, so it runs
//! unchanged (single-threaded) on wasm32.

pub(crate) mod ao;
pub(crate) mod edges;
pub(crate) mod error;
pub(crate) mod frame;
pub(crate) mod lines;
pub(crate) mod mode;
pub(crate) mod mode_frame;
pub(crate) mod mode_lines;
pub(crate) mod parallel;
pub(crate) mod prepared;
pub(crate) mod raster;
pub(crate) mod work;

use crate::TriangleMesh;
use crate::camera::Camera;
pub use crate::camera::bounds::Bounds;
pub use crate::camera::projection::{FisheyeMapping, Projection};
use crate::lighting::bounce::BounceLight;
use crate::lighting::gloss::Gloss;
use crate::lighting::{LightFrame, LightingScheme, Shader};
use crate::shadow::cast::{ShadowAlpha, ShadowCasters};
use crate::shadow::lamp::LampShadows;
use crate::shadow::set::{ExtraShadows, Shadows};
use crate::shadow::{Caster, DEFAULT_SHADOW_MAP_SIZE, LightSense, ShadowLight, ShadowMap};
use crate::vec3::{dot, normalize, scale, span, sub, unit_cross};
use ao::{Depths, Screen};
pub use error::RenderError;
use frame::Frame;
use raster::{Corner, Paint, PixelLights, Target, View, ViewCorner, at};
use work::{Work, charge_frame};

/// The largest image [`render`] draws, in pixels (width × height).
pub const MAX_RENDER_PIXELS: u64 = 64 * 1024 * 1024;

/// What [`render`] draws. Equal (and [`Eq`]) field by field, with
/// [`RenderOptions::light_gain`] compared by its bits.
#[derive(Debug, Clone, Copy)]
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
    /// [`shadow_casters`](Self::shadow_casters). A casting light on the far
    /// side of a face from the viewer gives none of its light to the side
    /// seen, at any [`shadow_map_size`](Self::shadow_map_size), so a sun does
    /// not light a room through a thin ceiling. Off, every light lights both
    /// sides of every face. No effect unlit.
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
    /// a closed mesh, such as a bulb in its shade, is shadowed by it, and a
    /// casting lamp on the far side of a face gives the side seen nothing. Up
    /// to [`MAX_LAMP_SHADOWS`](crate::MAX_LAMP_SHADOWS) cast, those adding
    /// most at the camera target ([`crate::FrameLampShadows`]).
    pub lamp_shadows: bool,
    /// Each lamp shadow cube face's side, texels
    /// ([`DEFAULT_LAMP_SHADOW_MAP_SIZE`](crate::DEFAULT_LAMP_SHADOW_MAP_SIZE)),
    /// clamped to 64–[`MAX_LAMP_SHADOW_MAP_SIZE`](crate::MAX_LAMP_SHADOW_MAP_SIZE).
    pub lamp_shadow_map_size: u32,
    /// Where a model's point and spot lights stop, for speed
    /// ([`LampCut::Auto`](crate::LampCut::Auto), which changes no pixel).
    pub lamp_cut: crate::LampCut,
    /// How many lamps a frame draws ([`LampCost::Full`](crate::LampCost::Full),
    /// every lamp meeting the view, which changes no pixel).
    pub lamp_cost: crate::LampCost,
    /// Which directional lights cast ([`ShadowCasters::EveryLight`] by
    /// default, up to [`MAX_SHADOW_CASTERS`](crate::MAX_SHADOW_CASTERS), each shadow taking
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
    /// [`render_mode`](Self::render_mode) draws. The GPU path draws any
    /// width ([`crate::GLSL_ES_300_WIDE_LINE_VERTEX`]).
    pub line_width: u32,
    /// How faces, edges and vertices are drawn (ISO 32000-1 §13.6.4.4,
    /// `/RM`); [`RenderMode::default`](crate::RenderMode::default) is
    /// Solid, the frame this crate always drew.
    pub render_mode: crate::RenderMode,
    /// One multiplier on every light (1, the default): each scheme light,
    /// the model's lights and lamps, the headlight and the ambient, diffuse
    /// and specular alike, not a material's own emission. Each channel is
    /// clamped to 1 after shading, on both paths. Outside
    /// [`MIN_LIGHT_GAIN`](crate::MIN_LIGHT_GAIN)–[`MAX_LIGHT_GAIN`](crate::MAX_LIGHT_GAIN)
    /// or not finite: [`RenderError::LightGain`].
    pub light_gain: f64,
    /// Light bounced between surfaces ([`crate::IndirectOptions`]): on for
    /// the lights fixed in the model by default, so a model lit only from
    /// the camera draws as before. The GPU path draws it from
    /// [`crate::FrameBounce`].
    pub indirect: crate::IndirectOptions,
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
            lamp_cut: crate::LampCut::Auto,
            lamp_cost: crate::LampCost::Full,
            shadow_casters: ShadowCasters::EveryLight,
            shadow_alpha: ShadowAlpha::Transmit,
            shininess: crate::ShininessScale::Auto,
            line_width: 1,
            render_mode: crate::RenderMode::default(),
            light_gain: 1.0,
            indirect: crate::IndirectOptions::default(),
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

/// Draw `meshes` as seen by `camera`.
///
/// Every triangle is filled in [`RenderOptions::colour`], lit per
/// [`RenderOptions::lighting`] by the angle between its face and each light;
/// nearer surfaces hide farther ones. Triangles with a non-finite or
/// out-of-range vertex are skipped.
///
/// A perspective camera clips at a near plane 1e-4 of the eye-to-target
/// distance in front of the eye: whatever is nearer, behind the camera
/// included, is not drawn, and a triangle crossing the plane is cut along
/// it. An orthographic camera does not clip; geometry behind the eye draws
/// as if in front of it.
///
/// # Errors
///
/// [`RenderError::Size`] for an empty or over-ceiling image;
/// [`RenderError::Camera`] for a camera that cannot form a view (eye on the
/// target, `up` parallel to the view, a field of view outside 0–180° or so
/// small its focal length is not finite, a non-positive orthographic
/// height); [`RenderError::TooMuchWork`] for a model whose triangles would
/// take past [`crate::MAX_RENDER_PIXEL_TESTS`] to draw.
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
/// pixel's depth key ([`View::project`]; under a fisheye 1 / the distance
/// from the eye), `NEG_INFINITY` where nothing opaque was drawn.
pub(crate) fn render_finished(
    model: crate::lighting::gloss::Model<'_>,
    paints: &[Paint<'_>],
    opaque: &[bool],
    camera: &Camera,
    options: &RenderOptions,
) -> Result<(Image, Vec<f64>), RenderError> {
    let (width, height) = checked_size(options)?;
    let lens = crate::camera::lens::Lens::for_camera(camera, width, height)?;
    // Built once, for every face of a fisheye alike.
    let bounce = crate::lighting::bounce::frame_bounce(model, paints, camera, options)?;
    let bounce = bounce.as_ref();
    if let Some(lens) = lens {
        let view = (camera, &lens, bounce);
        return crate::camera::lens_frame::render(model, paints, opaque, view, options);
    }
    render_view(model, paints, opaque, camera, options, (None, bounce))
}

/// `options`' image size, refused when empty or over the ceiling.
fn checked_size(options: &RenderOptions) -> Result<(u32, u32), RenderError> {
    let (width, height) = (options.width, options.height);
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_RENDER_PIXELS {
        return Err(RenderError::Size { width, height });
    }
    Ok((width, height))
}

/// [`render_finished`] for a planar `camera`, its camera-fixed lights,
/// shadow caster and lamps placed by `parent` (a fisheye, when this draws
/// one of its faces; [`crate::lighting::lamp_view::frame_shader`]) or else by it,
/// with the frame's `bounce` light.
pub(crate) fn render_view(
    (meshes, model_lights, finishes): crate::lighting::gloss::Model<'_>,
    paints: &[Paint<'_>],
    opaque: &[bool],
    camera: &Camera,
    options: &RenderOptions,
    (parent, bounce): (Option<&Camera>, Option<&BounceLight>),
) -> Result<(Image, Vec<f64>), RenderError> {
    let (width, height) = checked_size(options)?;
    let pixels = u64::from(width) * u64::from(height);
    let aspect = f64::from(width) / f64::from(height);
    let basis = camera.basis()?;
    let lights = parent.unwrap_or(camera).basis()?;
    let (shader, _) =
        crate::lighting::lamp_view::frame_shader(options, model_lights, camera, parent)?;
    let bounds = Bounds::of(meshes);
    let workers = parallel::workers(options.threads);
    let view = View::new(camera, aspect)?;
    let mut work = Work::default();
    let drawn = crate::render::frame::drawn(meshes, paints, opaque).map(|(m, t, _)| (m, t));
    charge_frame(&mut work, (camera, &basis, &view), drawn, (width, height))?;
    let shadows = Shadows::build(
        (options, &shader, &lights),
        bounds.as_ref(),
        (meshes, paints, opaque),
        (workers, &mut work),
    )?;
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
        bounce,
    };
    let frame = Frame {
        scene: &scene,
        meshes,
        paints,
        opaque,
    };
    let mut rgba = options.background.repeat(pixels as usize);
    let mut depth = vec![f64::NEG_INFINITY; pixels as usize];
    frame.draw((&mut rgba, &mut depth), (width, height), workers);
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
    /// Light bounced between surfaces, when drawn.
    bounce: Option<&'c BounceLight>,
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
            let v = crate::render::raster::view_point(dot(d, right), dot(d, up), dot(d, forward));
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

    /// The unit direction from model point `p` to the viewer; `None` with
    /// the eye at `p`.
    fn towards(&self, p: [f64; 3]) -> Option<[f64; 3]> {
        match self.view {
            View::Perspective { .. } => normalize(sub(self.camera.eye, p)),
            View::Orthographic { .. } => Some(scale(at3(&self.basis, 2), -1.0)),
        }
    }

    /// What lights the pixels of a triangle of `mesh` through model point
    /// `p` with unit normal `face`. The viewer is on one side of its plane
    /// everywhere, so one point decides which face of it is seen.
    fn pixel_lights<'m>(
        &'m self,
        mesh: &'m TriangleMesh,
        face: [f64; 3],
        p: [f64; 3],
    ) -> PixelLights<'m> {
        let seen = match self.towards(p) {
            Some(t) if dot(face, t) < 0.0 => scale(face, -1.0),
            _ => face,
        };
        PixelLights {
            shadow: self.shadow.as_ref(),
            key_far: self.caster.is_some_and(|c| c.is_behind(seen)),
            local: self.shader.local(),
            lamps: self.lamps.as_ref(),
            extra: self.extra.as_ref(),
            face: seen,
            gloss: self
                .gloss
                .as_ref()
                .and_then(|g| Some((g, g.finish_of(mesh)?))),
            bounce: self.bounce,
        }
    }

    /// The shade at model point `p` with normal `n`, but for the point and
    /// spot lights [`Target::fill`] adds per pixel, and the share of it the
    /// shadow caster supplies.
    fn shade(&self, p: [f64; 3], n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let towards = self.towards(p);
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
        let Some(face) = unit_cross(
            span(at3(&world, 0), at3(&world, 1)),
            span(at3(&world, 0), at3(&world, 2)),
        ) else {
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
            let lights = self.pixel_lights(mesh, face, at3(&world, 0));
            for pair in rest.windows(2) {
                if let [b, c] = pair {
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
mod tests;
