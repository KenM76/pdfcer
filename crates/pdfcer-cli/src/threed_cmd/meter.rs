//! `3d-render --illuminance`: the light arriving on the surface drawn at an
//! image position, under the lights the picture is drawn with.

/// `--illuminance-basis`: whether a physical figure is wanted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MeterBasisArg {
    /// Lux when every light states a physical strength and the model's
    /// length unit is known; a relative figure otherwise.
    #[default]
    Auto,
    /// Always the picture's own shading units.
    Relative,
}

/// `--illuminance-tint`: what a lux figure does with a light's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MeterTintArg {
    /// Each light counts at its full stated strength, as a lux meter reads.
    #[default]
    Neutral,
    /// Each light's strength is weighted by the brightness of its colour.
    Coloured,
}

/// `--illuminance-glow`: the light of glowing surfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MeterGlowArg {
    /// A glowing surface lights what it faces: in lux by the luminance its
    /// material states; a glow that states none makes the reading relative.
    #[default]
    Counted,
    /// Glowing surfaces light nothing, as in the picture.
    LeftOut,
}

/// `--over-range`: what a colour brighter than full white draws as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum OverRangeArg {
    /// Rolled toward white under --exposure-lux, clipped otherwise.
    #[default]
    Auto,
    /// Each channel stops at full white on its own (a warm lamp turns amber).
    Clip,
    /// Scaled to full white, then mixed with white keeping its light (a warm
    /// lamp reads near white).
    TowardWhite,
}

/// The `3d-render` options that measure the light on a surface.
#[derive(Clone, Debug, PartialEq, clap::Args)]
pub(crate) struct MeterArgs {
    /// Report the light arriving on the surface drawn at image position
    /// X,Y (as --pick), on the side facing the camera: the total, the
    /// direct and bounced parts, the part straight from glowing surfaces
    /// (--illuminance-glow), and each light's direct part, shadows
    /// counted. The lights are the picture's: the model's own, an aimed
    /// sun and the --bounce settings. The figure is lux when every light
    /// in force states a physical strength (a glTF light does; a light
    /// from another PRC file does not) and the model's length unit is
    /// known; otherwise it is relative (1 lights a white surface to full
    /// white) and a note says why. Repeatable.
    #[arg(long, value_name = "X,Y", value_parser = crate::parse_point2, allow_hyphen_values = true)]
    pub(crate) illuminance: Vec<[f64; 2]>,
    /// Average each --illuminance over a disc of this radius, in model
    /// units, lying on the surface round the point. Samples that fall off
    /// the edge of the picked surface are counted and left out of the
    /// average. 0 (default) measures the point alone.
    #[arg(long, value_name = "R", default_value_t = 0.0)]
    pub(crate) illuminance_radius: f64,
    /// Samples taken over the disc, at most 4096. A reading that would
    /// cast more than 4,000,000 rays is refused: ask for fewer samples or
    /// fewer --illuminance-rays.
    #[arg(long, value_name = "N", default_value_t = 64)]
    pub(crate) illuminance_samples: usize,
    /// Rays cast from each sample to gather the bounced light, at most
    /// 4096. More is steadier and slower.
    #[arg(long, value_name = "N", default_value_t = 128)]
    pub(crate) illuminance_rays: usize,
    /// Whether lux is wanted when it can be had.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) illuminance_basis: MeterBasisArg,
    /// What a lux figure does with a light's colour.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) illuminance_tint: MeterTintArg,
    /// What --illuminance does with surfaces that glow (a material that
    /// gives off light), which a model may use where it has no lamp.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) illuminance_glow: MeterGlowArg,
    /// Millimetres per model unit for --illuminance, when the model states
    /// none or states the wrong one (1000 for a model in metres). Lux from
    /// a point or spot light depends on it.
    #[arg(long, value_name = "MM")]
    pub(crate) illuminance_unit_mm: Option<f64>,
    /// Draw the picture at a fixed exposure: a white surface receiving LUX
    /// lux is drawn full white, and every lit surface in proportion to the
    /// lux it receives, so a dimly lit model draws dark instead of being
    /// brightened to suit. Each light shines with the physical strength it
    /// states (a glTF light's candela or lux), not the strength an import
    /// scaled it to; the ambient fill and --light-gain play no part, and
    /// --illuminance-tint and --illuminance-unit-mm apply. Refused, naming
    /// the reasons, when a light states no physical strength, the model's
    /// length unit is unknown, or a headlight, aimed sun or
    /// background-lit bounce is in force. Unset (default): the picture is
    /// drawn as the lights are stored.
    #[arg(long, value_name = "LUX", value_parser = positive_lux)]
    pub(crate) exposure_lux: Option<f64>,
    /// What a colour brighter than full white draws as: `auto` (default)
    /// rolls it toward white under --exposure-lux and clips each channel
    /// otherwise; `clip` stops each channel at full white on its own, so a
    /// warm lamp lens turns amber; `toward-white` scales the colour until
    /// its brightest channel is full white, then mixes in white to keep the
    /// light the clip would throw away, so the lens reads near white. The
    /// graphics-card picture uses the same rule.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) over_range: OverRangeArg,
    #[command(flatten)]
    pub(crate) light_map: Box<super::light_map::LightMapArgs>,
}

fn positive_lux(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
        _ => Err(format!("`{s}` is not a positive number of lux")),
    }
}

#[cfg(feature = "3d")]
impl MeterArgs {
    pub(super) fn settings(&self) -> pdfcer_3d::MeterSettings {
        let mut s = pdfcer_3d::MeterSettings::default();
        s.basis = match self.illuminance_basis {
            MeterBasisArg::Auto => pdfcer_3d::MeterBasis::Auto,
            MeterBasisArg::Relative => pdfcer_3d::MeterBasis::Relative,
        };
        s.tint = match self.illuminance_tint {
            MeterTintArg::Neutral => pdfcer_3d::LightTint::Neutral,
            MeterTintArg::Coloured => pdfcer_3d::LightTint::Coloured,
        };
        s.mm_per_unit = self.illuminance_unit_mm;
        s.gather_rays = self.illuminance_rays;
        s.glow = match self.illuminance_glow {
            MeterGlowArg::Counted => pdfcer_3d::GlowLight::Counted,
            MeterGlowArg::LeftOut => pdfcer_3d::GlowLight::LeftOut,
        };
        s
    }

    /// The picture at `--exposure-lux`, or as stored without it, with the
    /// notes to print after the `rendered` line.
    ///
    /// # Errors
    ///
    /// The refusal to print: why the picture cannot be drawn in lux, or the
    /// renderer's error.
    pub(super) fn draw(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
    ) -> Result<(pdfcer_3d::Image, Vec<String>), String> {
        if self.light_map.light_map {
            return self
                .light_map
                .draw(model, camera, options, &self.settings());
        }
        let exposure = match self.exposure_lux {
            Some(white_lux) => pdfcer_3d::Exposure::Fixed { white_lux },
            None => pdfcer_3d::Exposure::Metered,
        };
        let options = &pdfcer_3d::RenderOptions {
            over_range: match self.over_range {
                OverRangeArg::Auto => pdfcer_3d::OverRange::Auto,
                OverRangeArg::Clip => pdfcer_3d::OverRange::Clip,
                OverRangeArg::TowardWhite => pdfcer_3d::OverRange::TowardWhite,
            },
            ..*options
        };
        let exposed = exposure
            .lighting(model, options, &self.settings())
            .map_err(|err| format!("--exposure-lux: {err}"))?;
        let prepared = pdfcer_3d::PreparedModel::new(model);
        let image = exposed
            .render(&prepared, model, camera)
            .map_err(|err| err.to_string())?;
        let mut notes = Vec::new();
        if let Some(white) = exposed.white_lux {
            notes.push(format!(
                "note: exposure: fixed, full white at {white} lux; each light at the physical strength it states, --light-gain and any ambient fill left out"
            ));
            notes.extend(exposed.notes.iter().map(|n| format!("note: exposure: {n}")));
        }
        if exposed.options.over_range.rolls() {
            notes.push(
                "note: over-range: a colour brighter than full white is rolled toward white (--over-range clip clips each channel)"
                    .to_owned(),
            );
        }
        Ok((image, notes))
    }

    /// One `illuminance` line per `--illuminance`, an `illuminance-light`
    /// line per light under it, then what the meter assumed.
    pub(super) fn print(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
    ) {
        use pdfcer_3d::{IlluminanceQuery, IlluminanceUnit, LightMeter, PreparedModel};
        if self.illuminance.is_empty() {
            return;
        }
        let prepared = PreparedModel::new(model);
        let meter = match LightMeter::new(model, &prepared, camera, options, &self.settings()) {
            Ok(meter) => meter,
            Err(err) => return println!("illuminance refused: {err}"),
        };
        let unit = match meter.unit() {
            IlluminanceUnit::Lux => "lux",
            _ => "relative",
        };
        for &[x, y] in &self.illuminance {
            let at = format!("illuminance x={x} y={y}");
            let ray = match camera.ray_through(x, y, options.width, options.height) {
                Ok(ray) => ray,
                Err(err) => {
                    println!("{at} refused: {err}");
                    continue;
                }
            };
            let Some(hit) = pdfcer_3d::pick(&model.meshes, &ray) else {
                println!("{at} miss");
                continue;
            };
            let query = IlluminanceQuery::from_pick(&hit, &ray)
                .over_disc(self.illuminance_radius, self.illuminance_samples);
            match meter.read(&query) {
                Err(err) => println!("{at} refused: {err}"),
                Ok(r) => {
                    println!(
                        "{at} mesh={} point={} radius={} samples_on={} samples_off={} \
                         unit={unit} total={:.6} direct={:.6} bounced={:.6} ambient={:.6} \
                         emitted={:.6}",
                        hit.mesh,
                        hit.point.map(|c| format!("{c:.6}")).join(","),
                        self.illuminance_radius,
                        r.on_surface,
                        r.off_edge,
                        r.total,
                        r.direct,
                        r.bounced,
                        r.ambient,
                        r.emitted
                    );
                    for l in &r.lights {
                        println!(
                            "illuminance-light x={x} y={y} name={:?} direct={:.6}",
                            l.name, l.direct
                        );
                    }
                }
            }
        }
        print_notes(&meter);
    }
}

/// The meter's reasons for a relative figure, a way to lux when glows are
/// the only one, then what it assumed.
#[cfg(feature = "3d")]
fn print_notes(meter: &pdfcer_3d::LightMeter<'_>) {
    if !meter.reasons().is_empty() {
        let why: Vec<String> = meter.reasons().iter().map(ToString::to_string).collect();
        println!(
            "note: illuminance: a relative figure, not lux: {}",
            why.join("; ")
        );
        let glow_only = meter
            .reasons()
            .iter()
            .all(|r| matches!(r, pdfcer_3d::RelativeReason::GlowingParts(_)));
        if glow_only {
            println!(
                "note: illuminance: --illuminance-glow left-out reads lux from the model's lamps alone"
            );
        }
    }
    for note in meter.notes() {
        println!("note: illuminance: {note}");
    }
}
