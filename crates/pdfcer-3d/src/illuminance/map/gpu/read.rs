//! [`LightMeter::read`] with its gather measured on the host's GPU.

use super::super::Plan;
use crate::FloatTexture;
use crate::gpu::bounce_build::{FLAT_WIDTH, MAX_SIDE, local_to};
use crate::illuminance::{Illuminance, IlluminanceError, IlluminanceQuery, LightMeter, Sample};

/// One reading whose bounced and glow light the host measures on its GPU,
/// from [`LightMeter::gpu_read`]; [`Self::finish`] turns what it reads back
/// into the [`Illuminance`].
///
/// Draw the pass of the meter's [`LightMeter::gpu_scene`] as
/// [`super::GpuLightMap`] describes, with `u_points`, `u_normals`,
/// `u_point_count` and the target size taken from here and `u_lamp_count`
/// 0: the direct light is already measured, on the CPU.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GpuLightRead {
    /// The disc's samples, one texel each, 2,048 wide, less the scene's
    /// [`super::GpuLightMap::origin`].
    pub points: FloatTexture,
    /// Each sample's unit normal, the side measured.
    pub normals: FloatTexture,
    /// Samples, `u_point_count`.
    pub point_count: u32,
    /// The target's width, 2,048.
    pub target_width: u32,
    /// The target's rows.
    pub target_height: u32,
    /// Whether there is a gather to draw; when not, [`Self::finish`]
    /// ignores its argument.
    pub needs_pass: bool,
    /// The reading with only its direct and ambient light.
    base: Illuminance,
}

impl GpuLightRead {
    /// The reading, its bounced and glow light the mean of the green and
    /// blue of each sample's texel in `read` ([`Self::target_width`] ×
    /// [`Self::target_height`] RGBA floats, row 0 first).
    ///
    /// # Errors
    ///
    /// [`IlluminanceError::BadQuery`]`("read")` when `read` is shorter than
    /// the target and a pass is needed.
    pub fn finish(self, read: &[f32]) -> Result<Illuminance, IlluminanceError> {
        let mut out = self.base;
        if !self.needs_pass {
            return Ok(out);
        }
        let wanted = 4 * self.target_width as usize * self.target_height as usize;
        if read.len() < wanted {
            return Err(IlluminanceError::BadQuery("read"));
        }
        let texels = read
            .as_chunks::<4>()
            .0
            .iter()
            .take(self.point_count as usize);
        let (mut bounced, mut emitted) = (0.0, 0.0);
        for t in texels {
            bounced += f64::from(t[1]);
            emitted += f64::from(t[2]);
        }
        let k = 1.0 / f64::from(self.point_count.max(1));
        out.bounced = bounced * k;
        out.emitted = emitted * k;
        out.total = out.direct + out.bounced + out.ambient + out.emitted;
        Ok(out)
    }
}

impl LightMeter<'_> {
    /// [`Self::read`] with the direct light measured here and the bounced
    /// and glow light packed for the host's GPU to gather against
    /// [`Self::gpu_scene`], which is packed once for every reading.
    ///
    /// # Errors
    ///
    /// As [`Self::read`].
    pub fn gpu_read(&self, query: &IlluminanceQuery) -> Result<GpuLightRead, IlluminanceError> {
        let (spots, asked) = self.spots(query)?;
        let samples: Vec<Sample> = spots
            .iter()
            .map(|s| self.measure_direct(s.point, s.normal))
            .collect();
        let base = self.mean(&samples, asked - spots.len());
        let plan = Plan {
            points: spots.iter().map(|s| (s.point, s.normal)).collect(),
            ..Plan::default()
        };
        let origin = self.gpu_origin();
        let row = |p: [f32; 3]| [p[0], p[1], p[2], 0.0];
        let [points, normals] = [
            plan.points
                .iter()
                .map(|(p, _)| row(local_to(origin, *p)))
                .collect(),
            plan.points
                .iter()
                .map(|(_, n)| row(n.map(|v| v as f32)))
                .collect(),
        ]
        .map(|t| FloatTexture::from_rows(FLAT_WIDTH, t));
        debug_assert!(points.height as usize <= MAX_SIDE);
        Ok(GpuLightRead {
            points,
            normals,
            point_count: spots.len() as u32,
            target_width: FLAT_WIDTH as u32,
            target_height: spots.len().div_ceil(FLAT_WIDTH).max(1) as u32,
            needs_pass: self.volume.is_some() || !self.glow.is_empty(),
            base,
        })
    }
}
