//! A light meter whose bounce the host builds on the GPU.

use super::{IlluminanceError, LightMeter, MeterSettings};
use crate::gpu::bounce_build::pack;
use crate::{AssembledModel, Camera, FrameBounce, GpuBounceBuild, PreparedModel, RenderOptions};

/// A [`LightMeter`] made but not yet lit by bounce: trace [`Self::build`]
/// as [`GpuBounceBuild`] describes, read the last atlas back, and
/// [`Self::finish`] with it.
pub struct GpuLightMeter<'m> {
    meter: LightMeter<'m>,
    /// The bounce to build; `None` when the meter needs none, or when the
    /// scene is too large for the GPU and the bounce was built on the CPU.
    pub build: Option<GpuBounceBuild>,
}

impl std::fmt::Debug for GpuLightMeter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuLightMeter")
            .field("meter", &self.meter)
            .field("build", &self.build.is_some())
            .finish()
    }
}

impl<'m> LightMeter<'m> {
    /// [`Self::new`] with the bounce volume classified and packed for the
    /// host's GPU to build in place of [`Self::new`]'s CPU build. Its probes
    /// trace the bouncing lights and the glowing meshes, as the CPU's do;
    /// the host binds [`GpuBounceBuild::glow`] as `u_glow`.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn gpu_new(
        model: &'m AssembledModel,
        prepared: &PreparedModel,
        camera: &Camera,
        options: &RenderOptions,
        settings: &MeterSettings,
    ) -> Result<GpuLightMeter<'m>, IlluminanceError> {
        let mut meter = Self::unbuilt(model, prepared, camera, options, settings)?;
        let (threads, strength) = (meter.threads, meter.strength as f32);
        let build = meter
            .on_volume(|volume, scene, surfaces| {
                volume.classify_all(scene, threads);
                let grid = volume.grid();
                let template = FrameBounce {
                    atlas: Vec::new(),
                    atlas_width: 0,
                    atlas_height: 0,
                    origin: grid.origin.map(|v| v as f32),
                    spacing: grid.spacing.map(|v| v as f32),
                    dims: grid.dims.map(|d| d as i32),
                    strength,
                    follows_camera: false,
                };
                pack((scene, surfaces), volume, template)
            })
            .flatten();
        if build.is_none() {
            meter.on_volume(|volume, scene, _| volume.build(scene, threads));
        }
        Ok(GpuLightMeter { meter, build })
    }
}

impl<'m> GpuLightMeter<'m> {
    /// The meter, its bounce taken from `atlas`: the last atlas the build
    /// wrote, read back as RGBA32F (`atlas_width` × `atlas_height` × 4
    /// floats, bottom row first). `atlas` is ignored when [`Self::build`]
    /// is `None`.
    ///
    /// # Errors
    ///
    /// [`IlluminanceError::BadQuery`]`("atlas")` when `atlas` is not that
    /// size.
    pub fn finish(mut self, atlas: &[f32]) -> Result<LightMeter<'m>, IlluminanceError> {
        if self.build.is_some() {
            self.meter
                .on_volume(|volume, _, _| volume.load_atlas(atlas))
                .flatten()
                .ok_or(IlluminanceError::BadQuery("atlas"))?;
        }
        Ok(self.meter)
    }
}
