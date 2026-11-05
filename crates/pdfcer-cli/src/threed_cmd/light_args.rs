//! `3d-render --light-gain`: one brightness on every light.

/// The `3d-render` options that scale the lights.
#[derive(Clone, Copy, Debug, PartialEq, clap::Args)]
pub(crate) struct LightArgs {
    /// Multiply every light by this, 0.25 to 4 (default 1): the scheme's
    /// lights, the model's lights and lamps, the light at the camera and
    /// the ambient light, highlights included. Each colour channel stops
    /// at full brightness, so a high gain washes bright areas out to
    /// white. A material's own glow is not a light and is not scaled.
    #[arg(long, default_value_t = 1.0, value_parser = parse_gain)]
    pub(crate) light_gain: f64,
}

impl LightArgs {
    /// The `note: lighting:` line for a gain other than 1.
    #[cfg(feature = "3d")]
    pub(crate) fn print_note(&self) {
        if self.light_gain != 1.0 {
            println!(
                "note: lighting: every light times {} (--light-gain); each channel \
                 stops at full brightness",
                self.light_gain
            );
        }
    }
}

fn parse_gain(s: &str) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && (0.25..=4.0).contains(&v) => Ok(v),
        _ => Err(format!("`{s}`: expected a number from 0.25 to 4")),
    }
}

#[cfg(all(test, feature = "3d"))]
mod tests {
    #[test]
    fn the_gain_range_is_the_engines() {
        assert_eq!(
            super::parse_gain("0.25").ok(),
            Some(pdfcer_3d::MIN_LIGHT_GAIN)
        );
        assert_eq!(super::parse_gain("4").ok(), Some(pdfcer_3d::MAX_LIGHT_GAIN));
        for bad in ["0.2", "4.5", "nan", "inf", "x"] {
            assert!(super::parse_gain(bad).is_err(), "{bad}");
        }
    }
}
