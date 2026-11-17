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
    /// A glowing surface lights what it faces. A glow has no physical
    /// strength, so a model with one reads relative.
    #[default]
    Counted,
    /// Glowing surfaces light nothing, as in the picture.
    LeftOut,
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
}

#[cfg(feature = "3d")]
impl MeterArgs {
    fn settings(&self) -> pdfcer_3d::MeterSettings {
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
        if !meter.reasons().is_empty() {
            let why: Vec<String> = meter.reasons().iter().map(ToString::to_string).collect();
            println!(
                "note: illuminance: a relative figure, not lux: {}",
                why.join("; ")
            );
        }
        for note in meter.notes() {
            println!("note: illuminance: {note}");
        }
    }
}
