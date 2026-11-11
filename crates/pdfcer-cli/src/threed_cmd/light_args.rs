//! `3d-render --light-gain`, the `--sun-*` options, `--lamp-cost` and the
//! `--bounce*` options: one brightness on every light, a sun the operator
//! aims, how many lamps a frame draws, and the light that bounces off the
//! model's surfaces.

#[cfg(feature = "3d")]
use crate::ShadowLightArg;

/// The sun's colour, `--sun-colour`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SunColourArg {
    /// From the sun's height: 2000 K on the horizon rising to 5500 K
    /// overhead.
    FromElevation,
}

/// `--bounce`: which lights bounce off the model's surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum BounceArg {
    /// No bounced light.
    Off,
    /// The lights fixed in the scene: the model's own lights and lamps, an
    /// aimed sun and scheme lights fixed to the world.
    ModelLights,
    /// Those and the lights that follow the camera, the light at the camera
    /// included.
    EveryLight,
}

/// `--bounce-sky`: what light a bounce ray meeting nothing brings back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum BounceSkyArg {
    /// None.
    Dark,
    /// The background colour's.
    Background,
}

/// `--lamp-cost`: every lamp reaching the view, or only the nearest few.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LampCostArg {
    /// `full`.
    Full,
    /// `nearest:N`.
    Nearest(u16),
}

/// The `3d-render` options that scale the lights, aim a sun and choose the
/// lamps drawn.
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
    /// Which of the model's point and spot lights each frame draws. `full`
    /// (the default) draws every lamp whose light reaches the view: the
    /// same picture as drawing them all, as a lamp out of reach adds
    /// nothing. `nearest:N` draws only the N adding most light at the
    /// camera's target: faster with many lamps, but darker wherever the
    /// lamps left out shone, so it changes the picture. Never chosen for
    /// you.
    #[arg(long, default_value = "full", value_parser = parse_lamp_cost)]
    pub(crate) lamp_cost: LampCostArg,
    /// Which lights bounce off the model's surfaces onto others, so a lit
    /// red wall tints the floor beside it and light reaches round corners.
    /// `model-lights` (the default) bounces the lights fixed in the scene:
    /// the model's own lights and lamps, an aimed sun and world-fixed
    /// scheme lights. A model lit only from the camera, as most CAD files
    /// are, gets no bounce unless `every-light` adds the camera's lights.
    /// `off` draws direct light only.
    #[arg(long, value_enum, default_value_t = BounceArg::ModelLights)]
    pub(crate) bounce: BounceArg,
    /// How strongly bounced light adds, 0 to 4 (default 1, the physical
    /// amount); 0 turns it off.
    #[arg(long, default_value_t = 1.0, value_parser = parse_bounce_strength)]
    pub(crate) bounce_strength: f64,
    /// `dark` (the default): a bounce ray that leaves the model brings no
    /// light back. `background`: it brings the background colour's, so an
    /// open model is lit a little by its surroundings.
    #[arg(long, value_enum, default_value_t = BounceSkyArg::Dark)]
    pub(crate) bounce_sky: BounceSkyArg,
}

#[cfg(feature = "3d")]
impl LightArgs {
    /// The `note: lighting:` lines: a gain other than 1, and how many of a
    /// model's lamps the frame drew.
    pub(crate) fn print_note(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
    ) {
        if self.light_gain != 1.0 {
            println!(
                "note: lighting: every light times {} (--light-gain); each channel \
                 stops at full brightness",
                self.light_gain
            );
        }
        let lamps = pdfcer_3d::LampStats::for_model(camera, options, &model.lights);
        if let Some(line) = lamps.ok().as_ref().and_then(lamp_note) {
            println!("note: lighting: {line}");
        }
        let bounce = pdfcer_3d::BounceStats::for_model(camera, options, &model.lights);
        if let Some(line) = bounce.ok().and_then(|s| bounce_note(self, s)) {
            println!("note: lighting: bounce: {line}");
        }
    }

    /// The `--bounce*` options as the engine's setting.
    pub(crate) fn indirect(&self) -> pdfcer_3d::IndirectOptions {
        use pdfcer_3d::{Bounce, BounceSky};
        let mut o = pdfcer_3d::IndirectOptions::default();
        o.bounce = match self.bounce {
            BounceArg::Off => Bounce::Off,
            BounceArg::ModelLights => Bounce::ModelLights,
            BounceArg::EveryLight => Bounce::EveryLight,
        };
        o.bounce_strength = self.bounce_strength;
        o.bounce_sky = match self.bounce_sky {
            BounceSkyArg::Dark => BounceSky::Dark,
            BounceSkyArg::Background => BounceSky::Background,
        };
        o
    }

    /// `--lamp-cost` as the engine's setting.
    pub(crate) fn lamp_cost(&self) -> pdfcer_3d::LampCost {
        match self.lamp_cost {
            LampCostArg::Full => pdfcer_3d::LampCost::Full,
            LampCostArg::Nearest(n) => pdfcer_3d::LampCost::NearestPerFrame(n),
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

/// The lamp counts line; `None` for a model without lamps.
#[cfg(feature = "3d")]
fn lamp_note(s: &pdfcer_3d::LampStats) -> Option<String> {
    if s.in_model == 0 {
        return None;
    }
    let mut line = format!(
        "lamps: {} in the model, {} reaching the view, {} lit this frame",
        s.in_model, s.in_view, s.lit_this_frame
    );
    if s.lit_this_frame < s.in_view {
        line.push_str(&format!(
            "; --lamp-cost nearest left out {} reaching the view, so the picture is darker \
             than with every lamp",
            s.in_view - s.lit_this_frame
        ));
    }
    Some(line)
}

/// The bounce line; `None` when the operator turned it off.
#[cfg(feature = "3d")]
fn bounce_note(a: &LightArgs, s: pdfcer_3d::BounceStats) -> Option<String> {
    if a.bounce == BounceArg::Off || a.bounce_strength == 0.0 {
        return None;
    }
    if s.bouncing == 0 {
        return Some(if s.camera_lights > 0 {
            "none: the model is lit only from the camera, whose lights do not bounce by \
             default (--bounce every-light bounces them)"
                .to_owned()
        } else {
            "none: nothing lights the model".to_owned()
        });
    }
    let mut line = format!(
        "{} light(s) bounce off the model's surfaces, strength {}",
        s.bouncing, a.bounce_strength
    );
    if a.bounce == BounceArg::ModelLights && s.camera_lights > 0 {
        line.push_str(&format!(
            "; {} following the camera do not (--bounce every-light adds them)",
            s.camera_lights
        ));
    }
    Some(line)
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

fn parse_lamp_cost(s: &str) -> Result<LampCostArg, String> {
    let t = s.trim();
    if t == "full" {
        return Ok(LampCostArg::Full);
    }
    t.strip_prefix("nearest:")
        .and_then(|n| n.trim().parse::<u16>().ok())
        .map(LampCostArg::Nearest)
        .ok_or_else(|| format!("`{s}`: expected `full` or `nearest:N`, N from 0 to 65535"))
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

fn parse_bounce_strength(s: &str) -> Result<f64, String> {
    parse_within(s, 0.0, 4.0)
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
        use super::LampCostArg::{Full, Nearest};
        assert_eq!(super::parse_lamp_cost("full"), Ok(Full));
        assert_eq!(super::parse_lamp_cost("nearest:8"), Ok(Nearest(8)));
        assert_eq!(
            super::parse_bounce_strength("4").ok(),
            Some(pdfcer_3d::MAX_BOUNCE_STRENGTH)
        );
        assert!(super::parse_bounce_strength("4.1").is_err());
        for bad in ["nearest", "nearest:-1", "nearest:70000", "all"] {
            assert!(super::parse_lamp_cost(bad).is_err(), "{bad}");
        }
    }
}
