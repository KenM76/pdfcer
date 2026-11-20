//! A frame's bounce light built on the host's GPU (Pass 724.0): the scene
//! flattened into float textures and three fragment passes that trace the
//! rays [`crate::indirect`] traces on the CPU, writing the same atlas
//! [`FrameBounce`] carries.

#[macro_use]
mod glsl;
mod pictures;

pub use glsl::{
    GLSL_ES_300_BOUNCE_BUILD_VERTEX, GLSL_ES_300_BOUNCE_REDUCE_FRAGMENT,
    GLSL_ES_300_BOUNCE_TRACE_FRAGMENT, GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT,
};

use crate::camera::Camera;
use crate::camera::pick_index::flat::FlatPrim;
use crate::indirect::{
    BOUNCES, Lamp, RAYS, Scene, TEXELS, Volume, sphere_directions, texel_direction,
};
use crate::lighting::bounce::{BounceSetup, Painted, bounce_setup};
use crate::render::{RenderError, RenderOptions, triangle_uvs};
use crate::{AssembledModel, Bounce, BounceStats, FrameBounce, PreparedModel};

/// The width of the node, triangle and lamp textures, in texels.
pub(crate) const FLAT_WIDTH: usize = 2048;
/// The largest texture side a host is asked for.
pub(crate) const MAX_SIDE: usize = 4096;
/// Probes per row of the trace target, each 64 rays wide.
const TRACE_PROBES_PER_ROW: usize = 8;

/// An `RGBA32F` texture: `texels` holds `width` × `height` × 4 floats,
/// row 0 first.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct FloatTexture {
    pub texels: Vec<f32>,
    pub width: u32,
    pub height: u32,
}

/// An `RGBA8UI` 2D texture array: `texels` holds `width` × `height` ×
/// `layers` × 4 bytes, layer 0 row 0 first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ByteTexture {
    /// The bytes, 4 per texel.
    pub texels: Vec<u8>,
    /// Texels per row.
    pub width: u32,
    /// Rows per layer.
    pub height: u32,
    /// Layers, at least 1.
    pub layers: u32,
}

impl FloatTexture {
    /// `texels` laid `width` to a row, the last row padded with zeros.
    pub(crate) fn from_rows(width: usize, texels: Vec<[f32; 4]>) -> FloatTexture {
        let height = texels.len().div_ceil(width).max(1);
        let mut flat: Vec<f32> = texels.into_iter().flatten().collect();
        flat.resize(width * height * 4, 0.0);
        FloatTexture {
            texels: flat,
            width: width as u32,
            height: height as u32,
        }
    }
}

/// Everything a host needs to build a frame's [`FrameBounce`] on its GPU
/// with the three `GLSL_ES_300_BOUNCE_*` shaders, in place of
/// [`PreparedModel::frame_bounce`] tracing it on the CPU.
///
/// Positions in it are model units less [`FrameBounce::origin`] of
/// [`Self::bounce`], so f32 keeps their precision.
///
/// # The passes
///
/// Upload [`Self::nodes`], [`Self::triangles`], [`Self::uvs`],
/// [`Self::materials`], [`Self::lamps`] and [`Self::probes`] as `RGBA32F`
/// textures and [`Self::pictures`] as an `RGBA8UI` `TEXTURE_2D_ARRAY`
/// (`texImage3D`), all read with
/// `texelFetch` only.
/// Allocate two atlas render targets of [`FrameBounce::atlas_width`] ×
/// [`FrameBounce::atlas_height`] and one trace target of
/// [`Self::trace_width`] × [`Self::trace_height`], all `RGBA32F`
/// (`EXT_color_buffer_float`). Every pass draws
/// [`GLSL_ES_300_BOUNCE_BUILD_VERTEX`] as `drawArrays(TRIANGLES, 0, 3)`
/// over its whole target, with blending off.
///
/// For each of [`Self::bounces`] bounces:
///
/// 1. **Trace** into the trace target with
///    [`GLSL_ES_300_BOUNCE_TRACE_FRAGMENT`]: `u_nodes`, `u_tris`,
///    `u_uvs`, `u_materials`, `u_pictures`, `u_lamps`, `u_probes` the
///    textures; `u_lamp_count`,
///    `u_probe_count`, `u_far`, `u_eps`, `u_sky`, `u_rays` from the fields
///    of the same names; `u_first` true on the first bounce only;
///    `u_bounce_atlas` the previous bounce's atlas (unread on the first);
///    `u_bounce_origin` 0, `u_bounce_spacing` and `u_bounce_dims` from
///    [`Self::bounce`]. A host bounded by a GPU watchdog may draw it in
///    row bands with the scissor test.
/// 2. **Reduce** into the other atlas target with
///    [`GLSL_ES_300_BOUNCE_REDUCE_FRAGMENT`]: `u_trace` the trace target,
///    `u_probes`, `u_probe_count`, `u_rays`, `u_texels`, `u_far`, `u_cap`.
///
/// The last atlas written is the bounce: bind it as `u_bounce_atlas` of
/// the main fragment shader, with every other uniform from
/// [`Self::bounce`] as [`FrameBounce`] describes, and nothing read back.
/// To keep it on the CPU, read it into [`FrameBounce::atlas`].
///
/// # How it differs from the CPU build
///
/// It traces the same rays in f32 against the same triangles and samples
/// textures as the CPU does at each hit. Shadow and continuation rays
/// leave a surface [`Self::eps`] off it, wider than the CPU's offset. A
/// texture whose picture would take [`Self::pictures`] past 16 layers is
/// reflected at each triangle's centroid colour instead. The measured
/// agreement is in `docs/core-api/01-reading-and-model.md`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GpuBounceBuild {
    /// The scene's bounding-volume hierarchy, root first, 2 texels per
    /// node, 2048 texels wide: `[lo, a]` then `[hi, b]`; an inner node
    /// has `a` its left child and `b` its right, a leaf `a` = −1 − its
    /// first triangle and `b` one past its last.
    pub nodes: FloatTexture,
    /// The triangles in leaf order, 4 texels each, 2,048 wide: three
    /// corners with the mesh, the triangle index and the texture slot (−1
    /// for none) in their `w`, then the straight RGBA colour drawn (0..1),
    /// alpha the share kept; for a textured triangle the base colour the
    /// texture combines with.
    pub triangles: FloatTexture,
    /// The triangles' texture coordinates in leaf order, 2 texels each,
    /// 2,048 wide: `[uv0, uv1]`, `[uv2, 0, 0]`; zero when untextured.
    pub uvs: FloatTexture,
    /// The textures, 5 texels each, 2,048 wide: `[offset low, width,
    /// height, origin]` (origin 0 bottom-left, 1 top-left), `[uv matrix
    /// row 0, wrap u]`, `[row 1, wrap v]` (wrap 0 repeat, 1 clamp, 2
    /// mirrored), `[function, offset high, 0, 0]` (0 modulate, 1 replace,
    /// 2 blend, 3 decal; offset = high × 2^24 + low), `[blend colour]`.
    pub materials: FloatTexture,
    /// Every texture's picture, straight RGBA from its top row, one after
    /// another from texel `offset` counted across 2,048-wide rows and then
    /// layers; one short layer when that is enough, else 2,048 × 2,048
    /// layers, at most 16 (256 MiB).
    pub pictures: ByteTexture,
    /// The lights that bounce, 4 texels each, 2,048 wide:
    /// `[to_light or position, kind]` (0 directional, 1 point, 2 spot),
    /// `[colour, reach]`, `[attenuation, cos_cutoff]`, `[direction,
    /// exponent]`.
    pub lamps: FloatTexture,
    /// The probes, one texel each, 2,048 wide: position and 1 when in use,
    /// else 0.
    pub probes: FloatTexture,
    pub lamp_count: u32,
    pub probe_count: u32,
    /// The ray directions, unit.
    pub rays: Vec<[f32; 3]>,
    /// The direction of each texel of a probe's 8 × 8 map, row-major.
    pub texels: Vec<[f32; 3]>,
    /// How far a ray looks: twice the lattice's diagonal.
    pub far: f32,
    /// The longest distance a probe stores.
    pub cap: f32,
    /// How far a ray starts off a surface.
    pub eps: f32,
    /// The radiance of a ray that meets nothing.
    pub sky: [f32; 3],
    pub trace_width: u32,
    pub trace_height: u32,
    /// The bounces to trace (3).
    pub bounces: u32,
    /// The finished bounce's uniforms, with an empty [`FrameBounce::atlas`].
    pub bounce: FrameBounce,
}

impl PreparedModel {
    /// What [`Self::frame_bounce`] traces, packed for the host's GPU to
    /// trace instead; `None` where that gives `None`, or when the scene
    /// needs a texture side over 4,096 (about 2 million triangles), where
    /// the host builds on the CPU. Classifying the probes runs here, on
    /// [`RenderOptions::threads`].
    ///
    /// ```
    /// use pdfcer_3d::{AssembledModel, Camera, PreparedModel, Projection, RenderOptions};
    /// let model = AssembledModel::default();
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Perspective { fov_y: 30.0 },
    /// };
    /// let prepared = PreparedModel::new(&model);
    /// let options = RenderOptions::default();
    /// assert!(prepared.gpu_bounce_build(&model, &camera, &options)?.is_none());
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// As [`Self::frame_bounce`].
    pub fn gpu_bounce_build(
        &self,
        model: &AssembledModel,
        camera: &Camera,
        options: &RenderOptions,
    ) -> Result<Option<GpuBounceBuild>, RenderError> {
        self.gpu_bounce_build_lit((model, &model.lights), camera, options)
    }

    /// [`Self::gpu_bounce_build`] lit by `lights` in place of the model's
    /// own.
    pub(crate) fn gpu_bounce_build_lit(
        &self,
        (model, lights): (&AssembledModel, &[crate::ModelLight]),
        camera: &Camera,
        options: &RenderOptions,
    ) -> Result<Option<GpuBounceBuild>, RenderError> {
        let plan = crate::render::mode::Plan::of(options);
        let painted = self.paints(model, options);
        let pass = crate::render::mode_frame::face_pass(&plan, model, painted);
        let lit = crate::render::mode::frame_options(options);
        let scene = (&pass.meshes[..], lights, &[][..]);
        let Some(setup) = bounce_setup(scene, &pass.paints, camera, &lit)? else {
            return Ok(None);
        };
        let stats = BounceStats::for_model(camera, &lit, lights)?;
        let follows_camera = lit.indirect.bounce == Bounce::EveryLight && stats.camera_lights > 0;
        let BounceSetup {
            mut volume,
            index,
            surfaces,
            lamps,
            sky,
            strength,
        } = setup;
        let scene = Scene {
            meshes: surfaces.meshes,
            index: &index,
            surfaces: &surfaces,
            lamps: &lamps,
            sky,
            glow: &[],
        };
        volume.classify_all(&scene, lit.threads);
        let template = FrameBounce {
            atlas: Vec::new(),
            atlas_width: 0,
            atlas_height: 0,
            origin: volume.grid().origin.map(|v| v as f32),
            spacing: volume.grid().spacing.map(|v| v as f32),
            dims: volume.grid().dims.map(|d| d as i32),
            strength: strength as f32,
            follows_camera,
        };
        Ok(pack((&scene, &surfaces), &volume, template))
    }
}

/// The scene's triangles, textures and hierarchy, positions less
/// `origin`, as [`GpuBounceBuild`] lays them out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneTextures {
    pub(crate) nodes: FloatTexture,
    pub(crate) triangles: FloatTexture,
    pub(crate) uvs: FloatTexture,
    pub(crate) materials: FloatTexture,
    pub(crate) pictures: ByteTexture,
}

/// Position `p` less `origin`, in f32.
pub(crate) fn local_to(origin: [f64; 3], p: [f64; 3]) -> [f32; 3] {
    let ([x, y, z], [a, b, c]) = (p, origin);
    [(x - a) as f32, (y - b) as f32, (z - c) as f32]
}

/// The triangles in `index`, painted as `surfaces`, as textures; `None`
/// past [`MAX_SIDE`].
pub(crate) fn pack_scene(
    index: &crate::camera::pick_index::PickIndex,
    surfaces: &Painted,
    origin: [f64; 3],
) -> Option<SceneTextures> {
    let (flat_nodes, flat_prims) = index.flatten(origin);
    let nodes = flat_nodes.into_iter().flatten().collect();
    let (pictures, materials, slots) = pictures::pack_pictures(surfaces.paints);
    let (triangles, uvs) = triangle_texels(&flat_prims, surfaces, &slots);
    let [nodes, triangles, uvs] =
        [nodes, triangles, uvs].map(|t| FloatTexture::from_rows(FLAT_WIDTH, t));
    let scene = SceneTextures {
        nodes,
        triangles,
        uvs,
        materials,
        pictures,
    };
    let tall = [&scene.nodes, &scene.triangles, &scene.uvs, &scene.materials]
        .iter()
        .any(|t| t.height as usize > MAX_SIDE);
    (!tall).then_some(scene)
}

/// `lamps` as 4 texels each, positions less `origin`.
pub(crate) fn lamp_rows(lamps: &[Lamp], origin: [f64; 3]) -> Vec<[f32; 4]> {
    let local = |p| local_to(origin, p);
    lamps.iter().flat_map(|l| lamp_texels(l, &local)).collect()
}

/// The scene and classified `volume` as textures; `None` past [`MAX_SIDE`].
fn pack(
    (scene, surfaces): (&Scene, &Painted),
    volume: &Volume,
    mut bounce: FrameBounce,
) -> Option<GpuBounceBuild> {
    let grid = volume.grid();
    let origin = grid.origin;
    let SceneTextures {
        nodes,
        triangles,
        uvs,
        materials,
        pictures,
    } = pack_scene(scene.index, surfaces, origin)?;
    let probes = (0..grid.len())
        .map(|i| {
            let [x, y, z] = local_to(origin, grid.position(i));
            let used = volume.active().get(i).copied().unwrap_or(false);
            [x, y, z, if used { 1.0 } else { 0.0 }]
        })
        .collect();
    let [lamps, probes] =
        [lamp_rows(scene.lamps, origin), probes].map(|t| FloatTexture::from_rows(FLAT_WIDTH, t));
    if lamps.height.max(probes.height) as usize > MAX_SIDE {
        return None;
    }
    let atlas = volume.atlas_size();
    bounce.atlas_width = atlas.0 as u32;
    bounce.atlas_height = atlas.1 as u32;
    let far = 2.0 * grid.diagonal();
    let length = grid.spacing.iter().map(|s| s * s).sum::<f64>().sqrt();
    let to_f32 = |d: [f64; 3]| d.map(|v| v as f32);
    Some(GpuBounceBuild {
        nodes,
        triangles,
        uvs,
        materials,
        pictures,
        lamps,
        probes,
        lamp_count: scene.lamps.len() as u32,
        probe_count: grid.len() as u32,
        rays: sphere_directions(RAYS, false)
            .into_iter()
            .map(to_f32)
            .collect(),
        texels: (0..TEXELS).map(|t| to_f32(texel_direction(t))).collect(),
        far: far as f32,
        cap: (1.5 * length) as f32,
        eps: (EPS_SHARE * far) as f32,
        sky: to_f32(scene.sky),
        trace_width: (TRACE_PROBES_PER_ROW * RAYS) as u32,
        trace_height: grid.len().div_ceil(TRACE_PROBES_PER_ROW).max(1) as u32,
        bounces: BOUNCES as u32,
        bounce,
    })
}

/// Each of `prims`' 4 triangle texels and 2 texture-coordinate texels;
/// `slots` gives each mesh's texture slot.
fn triangle_texels(
    prims: &[FlatPrim],
    surfaces: &Painted,
    slots: &[Option<f32>],
) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let (mut triangles, mut uvs) = (Vec::new(), Vec::new());
    for p in prims {
        let [c0, c1, c2] = p.corners;
        let slot = slots.get(p.mesh).copied().flatten();
        let colour = match (slot, surfaces.paints.get(p.mesh)) {
            (Some(_), Some(paint)) => paint.colour,
            _ => surfaces.colour_at(p.mesh, p.triangle, [1.0 / 3.0; 3]),
        };
        triangles.extend([
            [c0[0], c0[1], c0[2], p.mesh as f32],
            [c1[0], c1[1], c1[2], p.triangle as f32],
            [c2[0], c2[1], c2[2], slot.unwrap_or(-1.0)],
            colour.map(|c| f32::from(c) / 255.0),
        ]);
        let corners = slot
            .and_then(|_| {
                let paint = surfaces.paints.get(p.mesh)?;
                let mesh = surfaces.meshes.get(p.mesh)?;
                triangle_uvs(mesh, paint.texture?.uv_set, p.triangle)
            })
            .unwrap_or([[0.0; 2]; 3])
            .map(|c| c.map(|v| v as f32));
        let [a, b, c] = corners;
        uvs.extend([[a[0], a[1], b[0], b[1]], [c[0], c[1], 0.0, 0.0]]);
    }
    (triangles, uvs)
}

/// How far, as a share of `far`, a GPU ray starts off a surface: about 100
/// f32 steps at the lattice's edge.
pub(crate) const EPS_SHARE: f64 = 1e-5;

/// A lamp's four texels.
fn lamp_texels(lamp: &Lamp, local: &dyn Fn([f64; 3]) -> [f32; 3]) -> [[f32; 4]; 4] {
    let f = |v: f64| v.clamp(-3.4e38, 3.4e38) as f32;
    match lamp {
        Lamp::Far { to_light, colour } => [
            [f(to_light[0]), f(to_light[1]), f(to_light[2]), 0.0],
            [f(colour[0]), f(colour[1]), f(colour[2]), 0.0],
            [0.0; 4],
            [0.0; 4],
        ],
        Lamp::Near(l) => {
            let [x, y, z] = local(l.position);
            let [c, k1, k2] = l.attenuation.map(f);
            let (kind, dir, cutoff, exponent) = match l.cone {
                Some(cone) => (2.0, cone.direction, cone.cos_cutoff, cone.exponent),
                None => (1.0, [0.0; 3], -2.0, 0.0),
            };
            [
                [x, y, z, kind],
                [f(l.colour[0]), f(l.colour[1]), f(l.colour[2]), f(l.reach)],
                [c, k1, k2, f(cutoff)],
                [f(dir[0]), f(dir[1]), f(dir[2]), f(exponent)],
            ]
        }
    }
}
