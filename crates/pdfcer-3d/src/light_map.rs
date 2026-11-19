//! A light map: a model's surfaces coloured by the illuminance falling on
//! them ([`crate::IlluminanceMap`]), banded by a caller's table
//! ([`LuxBands`]), on the CPU ([`render_light_map`]) and on the graphics
//! card ([`GLSL_ES_300_LIGHT_MAP_VERTEX`], [`GLSL_ES_300_LIGHT_MAP_FRAGMENT`]).
//! The surfaces are drawn unlit in the band colours; the legend (each
//! band's lower bound, in the map's unit) is the shell's to show.

mod draw;
pub(crate) mod levels;
mod shader;

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests;

pub use draw::render_light_map;
pub use shader::{GLSL_ES_300_LIGHT_MAP_FRAGMENT, GLSL_ES_300_LIGHT_MAP_VERTEX};

/// The most bands a [`LuxBands`] holds (the shader's uniform arrays).
pub const MAX_LUX_BANDS: usize = 16;

/// One band: figures from `from` up to the next band's `from` draw in
/// `colour` (sRGB bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LuxBand {
    /// The band's lower bound, in the map's unit.
    pub from: f64,
    /// Its colour.
    pub colour: [u8; 3],
}

/// How a figure between two bands' bounds is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BandBlend {
    /// The colour of the band it falls in: a stepped scale.
    #[default]
    Stepped,
    /// Mixed between the colours of the bound below and the bound above,
    /// in proportion to the figure (each sRGB byte linearly); a gradient.
    Graded,
}

/// Why a band table was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LuxBandsError {
    /// No bands.
    #[error("a band table needs at least one band")]
    Empty,
    /// More than [`MAX_LUX_BANDS`].
    #[error("{count} bands; the most is {limit}")]
    TooMany {
        /// Bands given.
        count: usize,
        /// The cap.
        limit: usize,
    },
    /// Band `0`'s bound is not finite, or not above the band before it.
    #[error("band {0}'s lower bound is not a finite number above the one before")]
    NotAscending(usize),
}

/// A band table: lower bounds strictly rising, each with a colour.
/// A figure below the first bound takes the first colour.
#[derive(Debug, Clone, PartialEq)]
pub struct LuxBands {
    bands: Vec<LuxBand>,
    blend: BandBlend,
}

/// [`LuxBands`] as the light-map fragment shader's uniforms, unused slots
/// zero.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BandUniforms {
    /// `u_band_from`.
    pub from: [f32; MAX_LUX_BANDS],
    /// `u_band_colour`, each channel 0–1.
    pub colour: [[f32; 3]; MAX_LUX_BANDS],
    /// `u_band_count`.
    pub count: i32,
    /// `u_graded`.
    pub graded: bool,
}

impl LuxBands {
    /// A table of `bands` in rising order.
    ///
    /// # Errors
    ///
    /// [`LuxBandsError`] for no bands, too many, or bounds that are not
    /// finite and strictly rising.
    pub fn new(bands: Vec<LuxBand>, blend: BandBlend) -> Result<Self, LuxBandsError> {
        if bands.is_empty() {
            return Err(LuxBandsError::Empty);
        }
        if bands.len() > MAX_LUX_BANDS {
            return Err(LuxBandsError::TooMany {
                count: bands.len(),
                limit: MAX_LUX_BANDS,
            });
        }
        let mut below = f64::NEG_INFINITY;
        for (i, b) in bands.iter().enumerate() {
            if !(b.from.is_finite() && b.from > below) {
                return Err(LuxBandsError::NotAscending(i));
            }
            below = b.from;
        }
        Ok(LuxBands { bands, blend })
    }

    /// The same colours and blend with the bounds spread evenly from `lo`
    /// to `hi`: band `i` of `n` starts at `lo + (hi - lo) * i / n`, so `hi`
    /// itself falls in the last band. A range that is empty, reversed or
    /// not finite is widened to `lo` plus the larger of `|lo|` and 1 (`lo`
    /// itself 0 when not finite), so the table is always valid.
    #[must_use]
    pub fn spread(&self, lo: f64, hi: f64) -> LuxBands {
        let lo = if lo.is_finite() { lo } else { 0.0 };
        let fallback = lo.abs().max(1.0);
        let width = if hi.is_finite() && hi > lo {
            hi - lo
        } else {
            fallback
        };
        let at = |width: f64| -> Vec<LuxBand> {
            let n = self.bands.len() as f64;
            let step = |i: usize| lo + width * i as f64 / n;
            let spread = self.bands.iter().enumerate();
            spread
                .map(|(i, b)| LuxBand {
                    from: step(i),
                    colour: b.colour,
                })
                .collect()
        };
        let mut bands = at(width);
        if bands
            .iter()
            .zip(bands.iter().skip(1))
            .any(|(a, b)| b.from <= a.from)
        {
            bands = at(fallback);
        }
        LuxBands {
            bands,
            blend: self.blend,
        }
    }

    /// The table a map draws in by default: [`Self::canada_ohs_steps`] for
    /// a map in lux; for a relative map, whose figures the regulation's
    /// levels say nothing about, the same colours spread evenly from its
    /// lowest figure to its highest ([`Self::spread`]), 0 to 1 when it has
    /// none.
    #[must_use]
    pub fn for_map(map: &crate::IlluminanceMap, blend: BandBlend) -> LuxBands {
        let steps = LuxBands::canada_ohs_steps(blend);
        match map.unit {
            crate::IlluminanceUnit::Lux => steps,
            _ => {
                let (lo, hi) = map.range().unwrap_or((0.0, 1.0));
                steps.spread(lo, hi)
            }
        }
    }

    /// The bands, rising.
    #[must_use]
    pub fn bands(&self) -> &[LuxBand] {
        &self.bands
    }

    /// How a figure between bounds is coloured.
    #[must_use]
    pub fn blend(&self) -> BandBlend {
        self.blend
    }

    /// The colour figure `f` draws in; a figure that is not a number takes
    /// the first band's.
    #[must_use]
    pub fn colour(&self, f: f64) -> [u8; 3] {
        let first = self.bands.first().map_or([0; 3], |b| b.colour);
        let Some(i) = self.bands.iter().rposition(|b| b.from <= f) else {
            return first;
        };
        let (Some(lo), next) = (self.bands.get(i), self.bands.get(i + 1)) else {
            return first;
        };
        match (self.blend, next) {
            (BandBlend::Graded, Some(hi)) => {
                let t = (f - lo.from) / (hi.from - lo.from);
                let mut out = [0u8; 3];
                for ((o, a), b) in out.iter_mut().zip(lo.colour).zip(hi.colour) {
                    let (a, b) = (f64::from(a), f64::from(b));
                    *o = (a + t * (b - a)).round().clamp(0.0, 255.0) as u8;
                }
                out
            }
            _ => lo.colour,
        }
    }

    /// The table as the fragment shader's uniforms.
    #[must_use]
    pub fn uniforms(&self) -> BandUniforms {
        let mut u = BandUniforms {
            from: [0.0; MAX_LUX_BANDS],
            colour: [[0.0; 3]; MAX_LUX_BANDS],
            count: self.bands.len() as i32,
            graded: self.blend == BandBlend::Graded,
        };
        for ((f, c), b) in u.from.iter_mut().zip(&mut u.colour).zip(&self.bands) {
            *f = b.from as f32;
            *c = b.colour.map(|x| f32::from(x) / 255.0);
        }
        u
    }
}
