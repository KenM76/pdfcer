//! A ramp's spot half for a vector writer on an ink page: the colorants a
//! `Separation`/`DeviceN` shading names and the tints it authors, so each
//! band can deposit them on planes of their own as the colorant buffer's
//! shading route does under separation simulation (ISO 32000-2 §10.8.3,
//! §8.6.6.4–5).

use std::sync::Arc;

use pdfcer_core::settings::OverprintZeroTintScope;

use super::{ColorRamp, GRADIENT_SAMPLES, Shading};
use crate::canvas::SpotInk;
use crate::overprint;

/// The authored colour at one point of a spot ramp: the process tints the
/// source states and one tint per spot colorant, in the ramp's order.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotTints {
    /// C, M, Y, K as the source states them; zero where it names none.
    pub process: [f32; 4],
    /// One tint per colorant of the ramp, `0.0..=1.0`.
    pub spots: Vec<f32>,
}

/// A gradient's spot half, sampled where [`super::GradientSpec::cmyk`] is.
#[derive(Debug, Clone)]
pub struct SpotRamp {
    /// The colorants, in the ramp's order, each at tint 0: the curve the
    /// colorant buffer gives the colorant's plane.
    pub inks: Vec<SpotInk>,
    /// `(offset 0..=1, authored colour)` at [`GRADIENT_SAMPLES`]` + 1`
    /// evenly spaced offsets.
    pub samples: Vec<(f32, SpotTints)>,
}

/// Inks compare by colorant name, the identity §7.3.5 gives them.
impl PartialEq for SpotRamp {
    fn eq(&self, other: &Self) -> bool {
        self.samples == other.samples
            && self.inks.len() == other.inks.len()
            && self
                .inks
                .iter()
                .zip(&other.inks)
                .all(|(a, b)| a.colorant == b.colorant)
    }
}

impl SpotRamp {
    /// The authored colour at `s`, interpolated between samples.
    #[must_use]
    pub fn at(&self, s: f32) -> Option<SpotTints> {
        let s = s.clamp(0.0, 1.0);
        let i = self.samples.iter().position(|(u, _)| *u >= s)?;
        let (ub, b) = self.samples.get(i)?;
        let Some((ua, a)) = i.checked_sub(1).and_then(|j| self.samples.get(j)) else {
            return Some(b.clone());
        };
        let f = if ub > ua { (s - ua) / (ub - ua) } else { 0.0 };
        let lerp = |x: f32, y: f32| (y - x).mul_add(f, x);
        let mut process = a.process;
        for (o, y) in process.iter_mut().zip(b.process) {
            *o = lerp(*o, y);
        }
        let spots = a
            .spots
            .iter()
            .zip(&b.spots)
            .map(|(x, y)| lerp(*x, *y))
            .collect();
        Some(SpotTints { process, spots })
    }
}

impl Shading {
    /// The spot colorants this analytic shading's ramp names, as inks at
    /// tint 0; empty for a mesh or a space naming no spot.
    #[must_use]
    pub(crate) fn spot_inks(&self) -> Vec<SpotInk> {
        if self.mesh.is_some() {
            return Vec::new();
        }
        let colorants = self
            .ramp
            .as_ref()
            .map_or(&[][..], ColorRamp::spot_colorants);
        if colorants.is_empty() {
            return Vec::new();
        }
        let space = &*self.color_space;
        let kind = overprint::classify(space, false, OverprintZeroTintScope::DeviceCmykOnly);
        let components: Vec<usize> = kind
            .as_ref()
            .map(|k| {
                overprint::authored_spots(k, &vec![0.0; space.components()])
                    .into_iter()
                    .map(|(c, _, _)| c)
                    .collect()
            })
            .unwrap_or_default();
        colorants
            .iter()
            .enumerate()
            .map(|(i, (name, lut))| SpotInk {
                colorant: Arc::clone(name),
                tint: 0.0,
                lut: Arc::clone(lut),
                own_appearance: components
                    .get(i)
                    .is_some_and(|c| overprint::colorant_space(space, *c).is_some()),
            })
            .collect()
    }

    /// [`SpotRamp`] for a gradient sampled from `t0` to `t1` (reversed
    /// when the gradient runs backwards); `None` without a spot half.
    pub(super) fn spot_ramp(&self, [t0, t1]: [f32; 2], reversed: bool) -> Option<SpotRamp> {
        let ramp = self.ramp.as_ref()?;
        let inks = self.spot_inks();
        if inks.is_empty() {
            return None;
        }
        let mut samples = Vec::with_capacity(GRADIENT_SAMPLES + 1);
        for i in 0..=GRADIENT_SAMPLES {
            #[allow(clippy::cast_precision_loss)]
            let u = i as f32 / GRADIENT_SAMPLES as f32;
            let along = if reversed { 1.0 - u } else { u };
            let (process, spots) = ramp.at_authored(t0 + along * (t1 - t0))?;
            let spots = spots.to_vec();
            samples.push((u, SpotTints { process, spots }));
        }
        Some(SpotRamp { inks, samples })
    }
}
