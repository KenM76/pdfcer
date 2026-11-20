//! [`LightMeter::map`] measured on the host's GPU (Pass 724.1): the lattice
//! laid out here, every point measured by one fragment of
//! [`GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT`](crate::GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT).

use super::{IlluminanceMap, Layout, MapSide, Plan};
use crate::gpu::bounce_build::{
    EPS_SHARE, FLAT_WIDTH, MAX_SIDE, SceneTextures, lamp_rows, local_to, pack_scene,
};
use crate::illuminance::{IlluminanceError, LightMeter, luminance};
use crate::indirect::sphere_directions;
use crate::lighting::bounce::Painted;
use crate::{Bounds, ByteTexture, FloatTexture, FrameBounce};

/// Everything a host needs to measure a [`LightMeter::map`] on its GPU,
/// from [`LightMeter::gpu_map`]; [`Self::finish`] turns what it reads back
/// into the map.
///
/// Positions in it are model units less [`Self::origin`], so f32 keeps
/// their precision.
///
/// # The pass
///
/// Upload [`Self::nodes`], [`Self::triangles`], [`Self::uvs`],
/// [`Self::materials`], [`Self::lamps`], [`Self::glow`], [`Self::points`],
/// [`Self::normals`] and [`Self::directions`] as `RGBA32F` textures,
/// [`Self::pictures`] as an `RGBA8UI` `TEXTURE_2D_ARRAY`, and the atlas of
/// [`Self::bounce`] (when there is one) as `RGBA32F`, all read with
/// `texelFetch` only. Draw
/// [`GLSL_ES_300_BOUNCE_BUILD_VERTEX`](crate::GLSL_ES_300_BOUNCE_BUILD_VERTEX)
/// with
/// [`GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT`](crate::GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT)
/// as `drawArrays(TRIANGLES, 0, 3)` over an `RGBA32F` target of
/// [`Self::target_width`] × [`Self::target_height`] with blending off:
/// samplers `u_nodes`, `u_tris`, `u_uvs`, `u_materials`, `u_pictures`,
/// `u_lamps`, `u_glow`, `u_points`, `u_normals`, `u_directions`,
/// `u_bounce_atlas`; `u_lamp_count`, `u_bounce_first`, `u_bounce_count`,
/// `u_glow_count`, `u_point_count`, `u_direction_count`, `u_far`,
/// `u_gather_far`, `u_eps`, `u_sky` from the fields of the same names;
/// `u_bounce` whether [`Self::bounce`] is `Some`, and `u_bounce_origin`,
/// `u_bounce_spacing`, `u_bounce_dims`, `u_bounce_strength` from it. A
/// host bounded by a GPU watchdog draws it in row bands with the scissor
/// test. Read the target back (`readPixels`, `RGBA`, `FLOAT`) and pass it
/// to [`Self::finish`].
///
/// # How it differs from [`LightMeter::map`]
///
/// The same lattice, the same lights, the same gather directions and the
/// same probes, traced in f32 against the same triangles; rays leave a
/// surface [`Self::eps`] off it, wider than the CPU's offset. A texture
/// whose picture would take [`Self::pictures`] past 16 layers is seen at
/// each triangle's centroid colour. The measured agreement is in
/// `docs/core-api/01-reading-and-model.md`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GpuLightMap {
    /// What every position is less, model units.
    pub origin: [f64; 3],
    /// As [`crate::GpuBounceBuild::nodes`].
    pub nodes: FloatTexture,
    /// As [`crate::GpuBounceBuild::triangles`]; the mesh in the first
    /// texel's `w` picks the glow.
    pub triangles: FloatTexture,
    /// As [`crate::GpuBounceBuild::uvs`].
    pub uvs: FloatTexture,
    /// As [`crate::GpuBounceBuild::materials`].
    pub materials: FloatTexture,
    /// As [`crate::GpuBounceBuild::pictures`].
    pub pictures: ByteTexture,
    /// Every light, 4 texels each as [`crate::GpuBounceBuild::lamps`],
    /// then from lamp [`Self::bounce_first`] the [`Self::bounce_count`]
    /// lights that bounce.
    pub lamps: FloatTexture,
    /// Each mesh's glow radiance, one texel each, 2,048 wide.
    pub glow: FloatTexture,
    /// The points, one texel each, 2,048 wide.
    pub points: FloatTexture,
    /// Each point's unit normal, the side measured.
    pub normals: FloatTexture,
    /// The gather directions, unit, one texel each, 2,048 wide.
    pub directions: FloatTexture,
    /// Lights measured directly, from lamp 0.
    pub lamp_count: u32,
    pub bounce_first: u32,
    pub bounce_count: u32,
    /// Meshes with a glow texel; 0 when no glow is counted.
    pub glow_count: u32,
    pub point_count: u32,
    pub direction_count: u32,
    /// Longer than any path inside the model.
    pub far: f32,
    /// How far a gather ray looks when there is a bounce.
    pub gather_far: f32,
    /// How far a ray starts off a surface.
    pub eps: f32,
    /// The radiance a gather ray that meets nothing brings back.
    pub sky: [f32; 3],
    pub target_width: u32,
    pub target_height: u32,
    /// The meter's bounce, its atlas filled and its origin less
    /// [`Self::origin`]; `None` when it has none.
    pub bounce: Option<FrameBounce>,
    laid: Layout,
    ambient: f64,
    base: IlluminanceMap,
}

impl GpuLightMap {
    /// The map from the target read back: [`Self::target_width`] ×
    /// [`Self::target_height`] RGBA floats, row 0 first. Each figure is
    /// the sum of its texel's red, green and blue and the ambient light.
    ///
    /// # Errors
    ///
    /// [`IlluminanceError::BadQuery`] when `read` is shorter than the
    /// target.
    pub fn finish(self, read: &[f32]) -> Result<IlluminanceMap, IlluminanceError> {
        let wanted = 4 * self.target_width as usize * self.target_height as usize;
        if read.len() < wanted {
            return Err(IlluminanceError::BadQuery("read"));
        }
        let figures: Vec<f64> = read
            .as_chunks::<4>()
            .0
            .iter()
            .take(self.point_count as usize)
            .map(|t| t.iter().take(3).map(|&v| f64::from(v)).sum::<f64>() + self.ambient)
            .collect();
        let mut map = self.base;
        map.meshes = super::fill_patches(self.laid, &figures);
        Ok(map)
    }
}

impl LightMeter<'_> {
    /// What [`Self::map`] measures, packed for the host's GPU to measure
    /// instead ([`GpuLightMap`]); `None` when the scene needs a texture
    /// side over 4,096 (about 2 million triangles), where the host maps on
    /// the CPU.
    ///
    /// # Errors
    ///
    /// As [`Self::map`], less [`IlluminanceError::Cancelled`].
    pub fn gpu_map(
        &self,
        spacing: Option<f64>,
        side: MapSide,
    ) -> Result<Option<GpuLightMap>, IlluminanceError> {
        let (plan, laid) = self.lay_out(spacing, side)?;
        let origin = match &self.volume {
            Some(v) => v.grid().origin,
            None => Bounds::of(self.meshes).map_or([0.0; 3], |b| b.min),
        };
        let surfaces = Painted {
            meshes: self.meshes,
            paints: &self.paints,
        };
        let Some(scene) = pack_scene(&self.index, &surfaces, origin) else {
            return Ok(None);
        };
        let Some((textures, direction_count)) = self.gpu_lattice(&plan, origin) else {
            return Ok(None);
        };
        let [lamps, glow, points, normals, directions] = textures;
        let gather_far = self
            .volume
            .as_ref()
            .map_or(self.far, |v| 2.0 * v.grid().diagonal());
        let SceneTextures {
            nodes,
            triangles,
            uvs,
            materials,
            pictures,
        } = scene;
        Ok(Some(GpuLightMap {
            origin,
            nodes,
            triangles,
            uvs,
            materials,
            pictures,
            lamps,
            glow,
            points,
            normals,
            direction_count,
            directions,
            lamp_count: self.lamps.len() as u32,
            bounce_first: self.lamps.len() as u32,
            bounce_count: self.bouncing.len() as u32,
            glow_count: self.glow.len() as u32,
            point_count: plan.points.len() as u32,
            far: self.far as f32,
            gather_far: gather_far as f32,
            eps: (EPS_SHARE * self.far) as f32,
            sky: self.sky.map(|v| v as f32),
            target_width: FLAT_WIDTH as u32,
            target_height: plan.points.len().div_ceil(FLAT_WIDTH).max(1) as u32,
            bounce: self.gpu_bounce(origin),
            laid,
            ambient: luminance(self.ambient),
            base: self.filled(Vec::new(), &[]),
        }))
    }

    /// The lamp, glow, point, normal and gather-direction textures and the
    /// number of directions; `None` past [`MAX_SIDE`].
    fn gpu_lattice(&self, plan: &Plan, origin: [f64; 3]) -> Option<([FloatTexture; 5], u32)> {
        let gathers = self.volume.is_some() || !self.glow.is_empty();
        let directions = match gathers {
            true => sphere_directions(self.gather_rays.saturating_mul(2), false),
            false => Vec::new(),
        };
        let row = |p: [f32; 3]| [p[0], p[1], p[2], 0.0];
        let f32s = |c: [f64; 3]| c.map(|v| v as f32);
        let lamps = [&self.lamps[..], &self.bouncing[..]].concat();
        let textures = [
            lamp_rows(&lamps, origin),
            self.glow.iter().map(|&g| row(f32s(g))).collect(),
            plan.points
                .iter()
                .map(|(p, _)| row(local_to(origin, *p)))
                .collect(),
            plan.points.iter().map(|(_, n)| row(f32s(*n))).collect(),
            directions.iter().map(|&d| row(f32s(d))).collect(),
        ]
        .map(|t| FloatTexture::from_rows(FLAT_WIDTH, t));
        let tall = textures.iter().any(|t| t.height as usize > MAX_SIDE);
        (!tall).then_some((textures, directions.len() as u32))
    }

    /// The meter's bounce as a frame's, its origin less `origin`.
    fn gpu_bounce(&self, origin: [f64; 3]) -> Option<FrameBounce> {
        let f32s = |c: [f64; 3]| c.map(|v| v as f32);
        self.volume.as_ref().map(|v| {
            let atlas = v.atlas();
            FrameBounce {
                atlas: atlas.texels,
                atlas_width: atlas.width as u32,
                atlas_height: atlas.height as u32,
                origin: local_to(origin, atlas.origin),
                spacing: f32s(atlas.spacing),
                dims: atlas.dims.map(|d| d as i32),
                strength: self.strength as f32,
                follows_camera: false,
            }
        })
    }
}
