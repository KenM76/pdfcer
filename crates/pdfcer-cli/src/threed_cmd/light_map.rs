//! `3d-render --light-map`: the picture as the model's surfaces coloured by
//! the illuminance falling on them, banded, with the legend printed.

/// `--light-map-blend`: how a figure between two bands is coloured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum LightMapBlendArg {
    /// The colour of the band the figure falls in.
    #[default]
    Stepped,
    /// Mixed between the band below and the band above.
    Graded,
}

/// `--light-map-side`: which side of each surface is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum LightMapSideArg {
    /// The side the camera sees, as --illuminance takes it.
    #[default]
    Seen,
    /// The side the model's normals (else its winding) face, from any view.
    Normals,
}

/// The `3d-render` options that draw a light map.
#[derive(Clone, Debug, Default, PartialEq, clap::Args)]
pub(crate) struct LightMapArgs {
    /// Draw a light map instead of the picture: every surface unlit, in
    /// the colour of the illuminance falling on it, measured at points no
    /// more than --light-map-spacing apart as --illuminance measures a
    /// point (the same lights, --bounce settings, --illuminance-basis,
    /// -tint, -glow, -rays and -unit-mm) and blended between them. The bands
    /// default to the levels of the Canada Occupational Health and Safety
    /// Regulations, SOR/86-304 Schedules I to IV, 10 to 1000 lux. A
    /// `light-map-band` line per band and the unit follow the `rendered`
    /// line. A model with no lux figure is mapped relative, and the
    /// default bands are then the same colours spread evenly from its
    /// lowest figure to its highest.
    #[arg(long, conflicts_with = "exposure_lux")]
    pub(crate) light_map: bool,
    /// The bands, as FROM:RRGGBB pairs rising, comma-separated, at most 16:
    /// a figure from FROM up to the next band's FROM draws in that colour,
    /// one below the first band in the first colour. For example
    /// `0:202060,100:3cc85a,500:f5781e`.
    #[arg(long, value_name = "BANDS", value_parser = parse_bands, requires = "light_map")]
    pub(crate) light_map_bands: Option<BandList>,
    /// How a figure between two bands' bounds is coloured.
    #[arg(long, value_enum, default_value_t, requires = "light_map")]
    pub(crate) light_map_blend: LightMapBlendArg,
    /// The most distance between two measured points, in model units:
    /// each triangle's edges are cut to it. Default: a 64th of the
    /// model's diagonal. 0 measures triangle corners only. Finer is
    /// slower; a map past 4,000,000 points is refused.
    #[arg(long, value_name = "UNITS", requires = "light_map")]
    pub(crate) light_map_spacing: Option<f64>,
    /// Which side of each surface is measured.
    #[arg(long, value_enum, default_value_t, requires = "light_map")]
    pub(crate) light_map_side: LightMapSideArg,
}

/// `--light-map-bands` as given: FROM and colour per band. A newtype, as
/// clap reads an `Option<Vec<_>>` as a repeated flag.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BandList(Vec<(f64, [u8; 3])>);

/// `--light-map-bands`, checked for shape; the order is checked with the
/// table.
fn parse_bands(s: &str) -> Result<BandList, String> {
    s.split(',')
        .map(|pair| {
            let bad = || format!("`{pair}` is not FROM:RRGGBB");
            let (from, hex) = pair.trim().split_once(':').ok_or_else(bad)?;
            let from: f64 = from.trim().parse().map_err(|_| bad())?;
            let hex = hex.trim().trim_start_matches('#');
            if hex.len() != 6 || !hex.is_ascii() {
                return Err(bad());
            }
            let mut colour = [0u8; 3];
            for (c, i) in colour.iter_mut().zip([0, 2, 4]) {
                let byte = hex.get(i..i + 2).ok_or_else(bad)?;
                *c = u8::from_str_radix(byte, 16).map_err(|_| bad())?;
            }
            Ok((from, colour))
        })
        .collect::<Result<_, _>>()
        .map(BandList)
}

#[cfg(feature = "3d")]
impl LightMapArgs {
    fn blend(&self) -> pdfcer_3d::BandBlend {
        match self.light_map_blend {
            LightMapBlendArg::Stepped => pdfcer_3d::BandBlend::Stepped,
            LightMapBlendArg::Graded => pdfcer_3d::BandBlend::Graded,
        }
    }

    /// `--light-map-bands` as a table; `None` when not given.
    fn given_bands(&self) -> Result<Option<pdfcer_3d::LuxBands>, String> {
        let Some(given) = &self.light_map_bands else {
            return Ok(None);
        };
        let bands = given
            .0
            .iter()
            .map(|&(from, colour)| pdfcer_3d::LuxBand { from, colour })
            .collect();
        pdfcer_3d::LuxBands::new(bands, self.blend())
            .map(Some)
            .map_err(|err| format!("--light-map-bands: {err}"))
    }

    /// The light map, with the lines to print after the `rendered` line.
    ///
    /// # Errors
    ///
    /// The refusal to print: a bad band table, or why the meter or the
    /// renderer refused.
    pub(super) fn draw(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
        settings: &pdfcer_3d::MeterSettings,
    ) -> Result<(pdfcer_3d::Image, Vec<String>), String> {
        let given = self.given_bands()?;
        let prepared = pdfcer_3d::PreparedModel::new(model);
        let meter = pdfcer_3d::LightMeter::new(model, &prepared, camera, options, settings)
            .map_err(|err| format!("--light-map: {err}"))?;
        let spacing = match self.light_map_spacing {
            Some(s) if s > 0.0 => Some(s),
            Some(_) => None,
            None => diagonal(model).map(|d| d / 64.0),
        };
        let side = match self.light_map_side {
            LightMapSideArg::Seen => pdfcer_3d::MapSide::FacingCamera,
            LightMapSideArg::Normals => pdfcer_3d::MapSide::AlongNormals,
        };
        let map = meter
            .map(spacing, side, &mut |_| true)
            .map_err(|err| format!("--light-map: {err}"))?;
        let bands = given.unwrap_or_else(|| pdfcer_3d::LuxBands::for_map(&map, self.blend()));
        let image = pdfcer_3d::render_light_map(&map, &bands, camera, options)
            .map_err(|err| err.to_string())?;
        Ok((image, self.legend(&map, &bands)))
    }

    fn legend(&self, map: &pdfcer_3d::IlluminanceMap, bands: &pdfcer_3d::LuxBands) -> Vec<String> {
        let unit = match map.unit {
            pdfcer_3d::IlluminanceUnit::Lux => "lux",
            _ => "relative",
        };
        let mut out: Vec<String> = bands
            .bands()
            .iter()
            .map(|b| {
                let [r, g, bl] = b.colour;
                format!(
                    "light-map-band from={} unit={unit} colour={r:02x}{g:02x}{bl:02x}",
                    b.from
                )
            })
            .collect();
        let (lo, hi) = map.range().unwrap_or((0.0, 0.0));
        out.push(format!(
            "note: light-map: figures from {lo:.6} to {hi:.6} {unit}"
        ));
        let relative = map.unit != pdfcer_3d::IlluminanceUnit::Lux;
        if self.light_map_bands.is_none() && relative {
            out.push(
                "note: light-map: the figures are relative, so the bands are the default colours spread evenly from the lowest figure to the highest; the regulation's levels do not apply"
                    .to_owned(),
            );
        } else if self.light_map_bands.is_none() {
            let s = pdfcer_3d::CANADA_OHS_SOURCE;
            out.push(format!(
                "note: light-map: bands at the levels of {} ({}), current to {}, {}; {}. {}",
                s.title, s.citation, s.current_to, s.url, s.reproduced, s.scope
            ));
        }
        if !map.reasons.is_empty() {
            let why: Vec<String> = map.reasons.iter().map(ToString::to_string).collect();
            out.push(format!(
                "note: light-map: a relative figure, not lux: {}",
                why.join("; ")
            ));
        }
        out.extend(map.notes.iter().map(|n| format!("note: light-map: {n}")));
        out
    }
}

/// The model's bounding-box diagonal; `None` for no finite point.
#[cfg(feature = "3d")]
fn diagonal(model: &pdfcer_3d::AssembledModel) -> Option<f64> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let points = model.meshes.iter().flat_map(|m| &m.positions);
    for p in points.filter(|p| p.iter().all(|c| c.is_finite())) {
        for ((l, h), c) in lo.iter_mut().zip(&mut hi).zip(p) {
            *l = l.min(*c);
            *h = h.max(*c);
        }
    }
    let d = lo
        .iter()
        .zip(&hi)
        .map(|(l, h)| (h - l).powi(2))
        .sum::<f64>()
        .sqrt();
    (d.is_finite() && d > 0.0).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::parse_bands;

    #[test]
    fn bands_parse_as_from_colour_pairs() {
        assert_eq!(
            parse_bands("0:202060, 100:#3CC85A").unwrap().0,
            vec![(0.0, [0x20, 0x20, 0x60]), (100.0, [0x3c, 0xc8, 0x5a])]
        );
        for bad in ["100", "x:202060", "0:20206", "0:2020zz", "0:éé20"] {
            assert!(parse_bands(bad).is_err(), "{bad}");
        }
    }
}
