//! `3d-render --light-gain` and the `--sun-*` options: one brightness on
//! every light, and a sun the operator aims.

use crate::ShadowLightArg;

/// The sun's colour, `--sun-colour`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SunColourArg {
    /// From the sun's height: 2000 K on the horizon rising to 5500 K
    /// overhead.
    FromElevation,
}

/// The `3d-render` options that scale the lights and aim a sun.
#[derive(Clone, Copy, Debug, PartialEq, clap::Args)]
pub(crate) struct LightArgs {
    /// Multiply every light by this, 0.25 to 4 (default 1): the scheme's
    /// lights, the model's lights and lamps, the light at the camera and
    /// the ambient light, highlights included. Each colour channel stops
    /// at full brightness, so a high gain washes bright areas out to
    /// white. A material's own glow is not a light and is not scaled.
    #[arg(long, default_value_t = 1.0, value_parser = parse_gain)]
    pub(crate) light_gain: f64,
    /// Add a sun at this compass bearing, degrees (default 45): 0 is the
    /// model's front, 90 its right, 180 its back, 270 its left, turning
    /// counter-clockwise seen from above the `--up` axis. Any `--sun-*`
    /// option adds the sun to the lighting scheme's lights and makes it
    /// the light that casts the shadow, in place of `--shadow-light`;
    /// `--shadow-light sun-negative` puts it on the minus side of `--up`.
    #[arg(long, value_parser = parse_azimuth)]
    pub(crate) sun_azimuth: Option<f64>,
    /// The sun's height above the horizon, 0 to 90 degrees (default 45);
    /// a higher sun casts shorter shadows.
    #[arg(long, value_parser = parse_elevation)]
    pub(crate) sun_elevation: Option<f64>,
    /// The sun's colour temperature, 1000 to 40000 kelvin: 6500 is white,
    /// lower is warmer (2000 is a sunset orange), higher is bluer.
    #[arg(long, value_parser = parse_kelvin, conflicts_with = "sun_colour")]
    pub(crate) sun_kelvin: Option<f64>,
    /// `from-elevation` (the default): the sun's colour follows its
    /// height, 2000 K on the horizon to 5500 K overhead.
    #[arg(long, value_enum)]
    pub(crate) sun_colour: Option<SunColourArg>,
    /// The sun's strength, 0.25 to 4 (default 1), on top of
    /// `--light-gain`.
    #[arg(long, value_parser = parse_gain)]
    pub(crate) sun_gain: Option<f64>,
}

#[cfg(feature = "3d")]
impl LightArgs {
    /// The `note: lighting:` line for a gain other than 1.
    pub(crate) fn print_note(&self) {
        if self.light_gain != 1.0 {
            println!(
                "note: lighting: every light times {} (--light-gain); each channel \
                 stops at full brightness",
                self.light_gain
            );
        }
    }

    /// The engine's shadow light: the aimed sun when any `--sun-*` option
    /// is given, else `chosen`.
    pub(crate) fn shadow_light(
        &self,
        chosen: ShadowLightArg,
        up: pdfcer_3d::UpAxis,
    ) -> pdfcer_3d::ShadowLight {
        use pdfcer_3d::{ShadowLight, SunColour, SunLight};
        let negative = chosen == ShadowLightArg::SunNegative;
        let aimed = self.sun_azimuth.is_some()
            || self.sun_elevation.is_some()
            || self.sun_kelvin.is_some()
            || self.sun_colour.is_some()
            || self.sun_gain.is_some();
        if aimed {
            return ShadowLight::AimedSun(SunLight {
                up,
                negative,
                azimuth_deg: self.sun_azimuth.unwrap_or(45.0),
                elevation_deg: self.sun_elevation.unwrap_or(45.0),
                colour: self
                    .sun_kelvin
                    .map_or(SunColour::FromElevation, SunColour::Kelvin),
                gain: self.sun_gain.unwrap_or(1.0),
            });
        }
        match chosen {
            ShadowLightArg::Scheme => ShadowLight::FollowsScheme,
            _ => ShadowLight::Sun { up, negative },
        }
    }
}

/// How the shadows note names an aimed sun.
#[cfg(feature = "3d")]
pub(crate) fn sun_note(sun: &pdfcer_3d::SunLight) -> String {
    let from = match sun.colour {
        pdfcer_3d::SunColour::Kelvin(_) => "",
        _ => ", from its elevation",
    };
    format!(
        "a sun aimed at azimuth {}°, elevation {}°, {:.0} K{from}, strength {}, added to the \
         lights",
        sun.azimuth_deg,
        sun.elevation_deg,
        sun.kelvin(),
        sun.gain
    )
}

fn parse_gain(s: &str) -> Result<f64, String> {
    parse_within(s, 0.25, 4.0)
}

fn parse_azimuth(s: &str) -> Result<f64, String> {
    parse_within(s, f64::MIN, f64::MAX)
}

fn parse_elevation(s: &str) -> Result<f64, String> {
    parse_within(s, 0.0, 90.0)
}

fn parse_kelvin(s: &str) -> Result<f64, String> {
    parse_within(s, 1000.0, 40000.0)
}

fn parse_within(s: &str, lo: f64, hi: f64) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && (lo..=hi).contains(&v) => Ok(v),
        Ok(v) if v.is_finite() => Err(format!("`{s}`: expected a number from {lo} to {hi}")),
        _ => Err(format!("`{s}`: expected a finite number")),
    }
}

#[cfg(all(test, feature = "3d"))]
mod tests {
    #[test]
    fn the_ranges_are_the_engines() {
        assert_eq!(
            super::parse_gain("0.25").ok(),
            Some(pdfcer_3d::MIN_LIGHT_GAIN)
        );
        assert_eq!(super::parse_gain("4").ok(), Some(pdfcer_3d::MAX_LIGHT_GAIN));
        for bad in ["0.2", "4.5", "nan", "inf", "x"] {
            assert!(super::parse_gain(bad).is_err(), "{bad}");
        }
        assert_eq!(
            super::parse_kelvin("1000").ok(),
            Some(pdfcer_3d::MIN_SUN_KELVIN)
        );
        assert_eq!(
            super::parse_kelvin("40000").ok(),
            Some(pdfcer_3d::MAX_SUN_KELVIN)
        );
        assert!(super::parse_elevation("90").is_ok() && super::parse_elevation("90.5").is_err());
        assert!(super::parse_azimuth("-720").is_ok() && super::parse_azimuth("inf").is_err());
    }
}
