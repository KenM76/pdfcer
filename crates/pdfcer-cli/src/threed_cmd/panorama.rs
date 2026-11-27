//! `3d-render --panorama`: part of the sphere round the eye, a chosen
//! horizontal and vertical angle, levelled or tilted with the view.

#[cfg(feature = "3d")]
use super::RenderThreeDArgs;

/// The `3d-render` options that draw a partial panorama.
#[derive(Clone, Debug, Default, PartialEq, clap::Args)]
pub(crate) struct PanoramaArgs {
    /// A panorama of DEG degrees across the picture, above 0 and at most
    /// 360, centred on the view's heading and turning about the up axis
    /// (`--up`, as for `--360`). Only that part is drawn, so a 120-degree
    /// strip costs a third of the whole sphere and the 64-megapixel limit
    /// counts the strip alone. `--panorama 360 --panorama-vertical 180` is
    /// the `--360` picture. A `.jpg` carries the photo-sphere (GPano) tag
    /// naming the part of the sphere it shows, so photo viewers place it
    /// at its true angles. Default size: 2048 pixels along the longer side,
    /// the other side at the ratio the angles imply.
    #[arg(long, value_name = "DEG", value_parser = parse_horizontal,
          conflicts_with_all = ["lens", "ortho", "sphere"])]
    pub(crate) panorama: Option<f64>,
    /// The panorama's vertical angle: degrees, above 0 and at most 180
    /// (default 180), or `view` for the view's own, `--fov` (default 30).
    /// A band reaching past straight up or down is moved back to stop at
    /// the pole.
    #[arg(long, value_name = "DEG|view", value_parser = parse_vertical, requires = "panorama")]
    pub(crate) panorama_vertical: Option<PanoramaVertical>,
    /// The panorama's pitch: `level` (default) centres the band on the
    /// horizon whatever the view's tilt, `view` centres it on the view's
    /// line of sight. Either way the sweep turns about the up axis, so the
    /// horizon stays a straight row.
    #[arg(long, value_enum, requires = "panorama")]
    pub(crate) panorama_tilt: Option<PanoramaTiltArg>,
    /// How latitude runs down the panorama: `equirectangular` (default)
    /// spaces rows evenly in angle, `cylindrical` evenly in its tangent,
    /// keeping verticals straight and the horizon unsquashed in a wide,
    /// short strip; it cannot reach a pole, and GPano has no cylindrical
    /// type, so it is written untagged.
    #[arg(long, value_enum, requires = "panorama")]
    pub(crate) panorama_mapping: Option<PanoramaMappingArg>,
}

/// `--panorama-vertical`: an angle, or the view's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PanoramaVertical {
    Degrees(f64),
    View,
}

/// `--panorama-tilt`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PanoramaTiltArg {
    /// Centred on the horizon.
    #[default]
    Level,
    /// Centred on the view's line of sight.
    View,
}

/// `--panorama-mapping`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PanoramaMappingArg {
    /// Rows evenly spaced in latitude; the photo-sphere projection.
    #[default]
    Equirectangular,
    /// Rows evenly spaced in tan(latitude); verticals stay straight.
    Cylindrical,
}

fn parse_horizontal(s: &str) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(d) if d > 0.0 && d <= 360.0 => Ok(d),
        _ => Err(format!("`{s}` is not an angle above 0 and at most 360")),
    }
}

fn parse_vertical(s: &str) -> Result<PanoramaVertical, String> {
    if s.trim().eq_ignore_ascii_case("view") {
        return Ok(PanoramaVertical::View);
    }
    match s.trim().parse::<f64>() {
        Ok(d) if d > 0.0 && d <= 180.0 => Ok(PanoramaVertical::Degrees(d)),
        _ => Err(format!(
            "`{s}` is not `view` or an angle above 0 and at most 180"
        )),
    }
}

impl PanoramaArgs {
    /// The vertical angle in degrees, `--fov` (default 30) standing in for
    /// the view's own.
    fn vertical(&self, fov: Option<f64>) -> f64 {
        match self.panorama_vertical {
            None => 180.0,
            Some(PanoramaVertical::Degrees(d)) => d,
            Some(PanoramaVertical::View) => fov.unwrap_or(30.0),
        }
    }

    fn cylindrical(&self) -> bool {
        self.panorama_mapping == Some(PanoramaMappingArg::Cylindrical)
    }

    /// The width / height a square pixel at the horizon gives: the angle
    /// ratio, or for a cylindrical band centred on the horizon, longitude
    /// over the tangent span.
    fn aspect(&self, horizontal: f64, fov: Option<f64>) -> f64 {
        let v = self.vertical(fov);
        if self.cylindrical() {
            horizontal.to_radians() / (2.0 * (v / 2.0).to_radians().tan())
        } else {
            horizontal / v
        }
    }
}

/// The picture size of a `--panorama`: as given; the missing side at the
/// angles' ratio; 2048 along the longer side when neither is given. `None`
/// without `--panorama`.
pub(crate) fn panorama_size(
    p: &PanoramaArgs,
    fov: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
) -> Option<(u32, u32)> {
    let ratio = p.aspect(p.panorama?, fov);
    let side = |x: f64| x.round().clamp(1.0, f64::from(u32::MAX)) as u32;
    Some(match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, side(f64::from(w) / ratio)),
        (None, Some(h)) => (side(f64::from(h) * ratio), h),
        (None, None) if ratio >= 1.0 => (2048, side(2048.0 / ratio)),
        (None, None) => (side(2048.0 * ratio), 2048),
    })
}

/// The engine's extent for a `--panorama`, `None` without one.
#[cfg(feature = "3d")]
pub(super) fn extent(a: &RenderThreeDArgs<'_>) -> Option<pdfcer_3d::PanoramaExtent> {
    let p = a.panorama;
    Some(pdfcer_3d::PanoramaExtent {
        horizontal: p.panorama?,
        vertical: Some(p.vertical(a.fov)),
        tilt: match p.panorama_tilt.unwrap_or_default() {
            PanoramaTiltArg::Level => pdfcer_3d::PanoramaTilt::Level,
            PanoramaTiltArg::View => pdfcer_3d::PanoramaTilt::View,
        },
        mapping: if p.cylindrical() {
            pdfcer_3d::PanoramaMapping::Cylindrical
        } else {
            pdfcer_3d::PanoramaMapping::Equirectangular
        },
    })
}

/// The photo-sphere XMP packet for a JPEG of `camera`'s picture: the whole
/// sphere's tag for `--360`, the cropped tag for an equirectangular
/// `--panorama`, none otherwise.
#[cfg(feature = "3d")]
pub(super) fn sphere_xmp(camera: &pdfcer_3d::Camera, (w, h): (u32, u32)) -> Option<String> {
    use pdfcer_3d::{FisheyeMapping, PanoramaMapping, Projection};
    use pdfcer_render::export::{partial_photo_sphere_xmp, photo_sphere_xmp};
    match camera.projection {
        Projection::Fisheye {
            mapping: FisheyeMapping::Equirectangular,
            ..
        } => Some(photo_sphere_xmp(w, h)),
        Projection::Panorama {
            horizontal,
            vertical,
            pitch,
            mapping: PanoramaMapping::Equirectangular,
        } => partial_photo_sphere_xmp(w, h, horizontal, vertical, pitch),
        _ => None,
    }
}

/// The `note: panorama:` lines: the band drawn, its pitch, the tag, and a
/// size whose pixels are not square.
#[cfg(feature = "3d")]
pub(super) fn print_note(a: &RenderThreeDArgs<'_>, camera: &pdfcer_3d::Camera) {
    let pdfcer_3d::Projection::Panorama {
        horizontal,
        vertical,
        pitch,
        ..
    } = camera.projection
    else {
        return;
    };
    let p = a.panorama;
    let tilt = match p.panorama_tilt.unwrap_or_default() {
        PanoramaTiltArg::Level => "levelled: the horizon is the middle row".to_owned(),
        PanoramaTiltArg::View => {
            format!("tilted with the view: centred {pitch:.3} degrees above the horizon")
        }
    };
    println!("note: panorama: {horizontal} x {vertical} degrees round the eye, {tilt}");
    let [x, y, z] = camera.up.map(|c| format!("{c}"));
    println!("note: panorama: the sweep turns about the up axis {x},{y},{z}");
    if p.cylindrical() {
        println!(
            "note: panorama: a cylindrical panorama carries no photo-sphere tag (GPano has no \
             cylindrical projection), so viewers show it flat"
        );
    } else if super::picture::is_jpeg(a.output) {
        println!(
            "note: panorama: the JPEG carries the photo-sphere (GPano) tag with the crop it is \
             of the whole sphere, so viewers that read it place it at its true angles"
        );
    }
    let ratio = p.aspect(horizontal, a.fov);
    let drawn = f64::from(a.width) / f64::from(a.height.max(1));
    if !p.cylindrical() && (drawn / ratio - 1.0).abs() > 0.01 {
        println!(
            "note: panorama: the picture is {}x{}, not {horizontal}:{vertical}, so its pixels \
             span more degrees one way than the other",
            a.width, a.height
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)] // Tests fail loudly by design.
    use super::*;

    fn args(h: f64, v: Option<PanoramaVertical>) -> PanoramaArgs {
        PanoramaArgs {
            panorama: Some(h),
            panorama_vertical: v,
            ..PanoramaArgs::default()
        }
    }

    #[test]
    fn the_size_keeps_the_angles_ratio() {
        let strip = args(120.0, Some(PanoramaVertical::Degrees(40.0)));
        assert_eq!(panorama_size(&strip, None, None, None), Some((2048, 683)));
        assert_eq!(
            panorama_size(&strip, None, Some(600), None),
            Some((600, 200))
        );
        assert_eq!(
            panorama_size(&strip, None, None, Some(100)),
            Some((300, 100))
        );
        assert_eq!(panorama_size(&strip, None, Some(7), Some(9)), Some((7, 9)));
        let tall = args(60.0, Some(PanoramaVertical::View));
        assert_eq!(
            panorama_size(&tall, Some(120.0), None, None),
            Some((1024, 2048))
        );
        assert_eq!(
            panorama_size(&args(360.0, None), None, None, None),
            Some((2048, 1024))
        );
        assert_eq!(
            panorama_size(&PanoramaArgs::default(), None, None, None),
            None
        );
    }

    #[test]
    fn a_cylindrical_size_is_square_at_the_horizon() {
        let strip = PanoramaArgs {
            panorama_mapping: Some(PanoramaMappingArg::Cylindrical),
            ..args(90.0, Some(PanoramaVertical::Degrees(90.0)))
        };
        let h = (2048.0 * 2.0 / std::f64::consts::FRAC_PI_2).round() as u32;
        assert_eq!(
            panorama_size(&strip, None, Some(2048), None),
            Some((2048, h))
        );
    }

    #[test]
    fn the_angles_are_parsed_in_range() {
        assert_eq!(parse_horizontal("360"), Ok(360.0));
        assert!(parse_horizontal("0").is_err() && parse_horizontal("361").is_err());
        assert_eq!(parse_vertical("View"), Ok(PanoramaVertical::View));
        assert_eq!(parse_vertical("45"), Ok(PanoramaVertical::Degrees(45.0)));
        assert!(parse_vertical("181").is_err() && parse_vertical("nan").is_err());
    }
}
