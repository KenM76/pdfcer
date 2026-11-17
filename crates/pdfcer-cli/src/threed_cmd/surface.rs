//! `3d-render --measure-surfaces`: the distance or the angle between the
//! surfaces drawn at two image positions.

/// Clap parser for two image positions as `X1,Y1,X2,Y2`.
fn parse_pair(s: &str) -> Result<[f64; 4], String> {
    let bad = || format!("`{s}` is not four numbers like 80,45.5,300,45.5");
    let parts: Vec<f64> = s
        .split(',')
        .map(|t| t.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    <[f64; 4]>::try_from(parts).map_err(|_| bad())
}

/// The `3d-render` options that measure between two surfaces.
#[derive(Clone, Debug, PartialEq, clap::Args)]
pub(crate) struct SurfaceArgs {
    /// Measure between the surfaces drawn at image positions X1,Y1 and
    /// X2,Y2 (as --pick). When the two are parallel within
    /// --measure-parallel-tolerance the distance between them is reported,
    /// in model units and in millimetres when the model's unit is known;
    /// otherwise the angle between them is. A surface is the flat part of
    /// the model's face round the picked point, fitted from its triangles;
    /// a `measure-surface` line per side says how many triangles it was
    /// fitted from and a note says when that is not the whole face.
    /// Repeatable.
    #[arg(long, value_name = "X1,Y1,X2,Y2", value_parser = parse_pair, allow_hyphen_values = true)]
    pub(crate) measure_surfaces: Vec<[f64; 4]>,
    /// Surfaces that meet at this angle or less, in degrees (0 to 90),
    /// count as parallel and report a distance; a wider pair reports its
    /// angle.
    #[arg(long, value_name = "DEG", default_value_t = pdfcer_3d_default::PARALLEL)]
    pub(crate) measure_parallel_tolerance: f64,
    /// Report a distance whatever the angle: from the second position to
    /// the plane of the first surface. The line says `forced=yes` when the
    /// pair was wider than the tolerance.
    #[arg(long)]
    pub(crate) measure_force_parallel: bool,
    /// A triangle belongs to a picked surface when it faces within this
    /// angle, in degrees (0 to 90), of the picked triangle and lies in its
    /// plane. Raise it to take a slightly bowed face as one surface.
    #[arg(long, value_name = "DEG", default_value_t = pdfcer_3d_default::FLAT)]
    pub(crate) measure_flat_tolerance: f64,
}

/// The engine's defaults, restated so the flags exist in a build without
/// the 3D engine; a test holds the two equal.
mod pdfcer_3d_default {
    pub(super) const PARALLEL: f64 = 0.5;
    pub(super) const FLAT: f64 = 0.5;
}

#[cfg(feature = "3d")]
impl SurfaceArgs {
    /// One `measure-surfaces` line per `--measure-surfaces`, a
    /// `measure-surface` line per side under it, then what was assumed.
    pub(super) fn print(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
    ) {
        use pdfcer_3d::{SurfacePick, SurfaceTolerance, measure_surfaces};
        if self.measure_surfaces.is_empty() {
            return;
        }
        let mut tolerance = SurfaceTolerance::default();
        tolerance.parallel_degrees = self.measure_parallel_tolerance;
        tolerance.force_parallel = self.measure_force_parallel;
        tolerance.flat_degrees = self.measure_flat_tolerance;
        let pick = |x: f64, y: f64| -> Result<SurfacePick, String> {
            let ray = camera
                .ray_through(x, y, options.width, options.height)
                .map_err(|err| format!("refused: {err}"))?;
            let hit =
                pdfcer_3d::pick(&model.meshes, &ray).ok_or_else(|| format!("miss at {x},{y}"))?;
            Ok(SurfacePick::from(&hit))
        };
        let (mut partial, mut measured) = (false, false);
        for &[x1, y1, x2, y2] in &self.measure_surfaces {
            let at = format!("measure-surfaces a={x1},{y1} b={x2},{y2}");
            let picks = pick(x1, y1).and_then(|a| Ok((a, pick(x2, y2)?)));
            let (a, b) = match picks {
                Ok(picks) => picks,
                Err(why) => {
                    println!("{at} {why}");
                    continue;
                }
            };
            match measure_surfaces(&model.meshes, &a, &b, &tolerance) {
                Err(err) => println!("{at} refused: {err}"),
                Ok(m) => {
                    measured = true;
                    partial |= !m.a.whole_face || !m.b.whole_face;
                    println!("{at} {}", relation(&m, model.units));
                    side('a', &a, &m.a);
                    side('b', &b, &m.b);
                }
            }
        }
        if partial {
            println!(
                "note: measure-surfaces: a surface marked whole_face=no is the flat part \
                 round the picked point, not a whole face of the model (the face is curved \
                 or stepped, or the file separates no faces)"
            );
        }
        if measured && model.units.is_some_and(|u| !u.from_cad_file) {
            println!(
                "note: measure-surfaces: the millimetre figures rest on a length unit the \
                 file does not state for its geometry"
            );
        }
    }
}

/// The figures of one measurement.
#[cfg(feature = "3d")]
fn relation(m: &pdfcer_3d::SurfaceMeasure, units: Option<pdfcer_3d::ModelUnits>) -> String {
    use pdfcer_3d::SurfaceRelation;
    let mm = |v: f64| units.map_or(String::new(), |u| format!(" ({:.6} mm)", v * u.mm_per_unit));
    let head = match m.relation {
        SurfaceRelation::Parallel { distance, forced } => format!(
            "relation=parallel distance={distance:.6}{} forced={}",
            mm(distance),
            if forced { "yes" } else { "no" }
        ),
        SurfaceRelation::Angled { degrees } => format!("relation=angled angle={degrees:.6}"),
        _ => "relation=unknown".to_owned(),
    };
    format!(
        "{head} off_parallel={:.6} normals_angle={:.6} point_distance={:.6}{} delta={}",
        m.angle_degrees,
        m.normals_degrees,
        m.pick_distance,
        mm(m.pick_distance),
        m.delta.map(|c| format!("{c:.6}")).join(","),
    )
}

/// One side's `measure-surface` line.
#[cfg(feature = "3d")]
fn side(name: char, pick: &pdfcer_3d::SurfacePick, patch: &pdfcer_3d::SurfacePatch) {
    let triple = |v: [f64; 3]| v.map(|c| format!("{c:.6}")).join(",");
    println!(
        "measure-surface side={name} mesh={} triangle={} face={} whole_face={} triangles={} \
         area={:.6} flatness={:.6} point={} normal={}",
        patch.mesh,
        pick.triangle,
        patch.face.map_or("none".to_owned(), |f| f.to_string()),
        if patch.whole_face { "yes" } else { "no" },
        patch.triangles,
        patch.area,
        patch.flatness_degrees,
        triple(pick.point),
        triple(patch.normal),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_is_four_finite_numbers() {
        assert_eq!(parse_pair("1, 2,3.5,-4"), Ok([1.0, 2.0, 3.5, -4.0]));
        assert!(parse_pair("1,2,3").is_err());
        assert!(parse_pair("1,2,3,nan").is_err());
    }

    #[cfg(feature = "3d")]
    #[test]
    fn the_flag_defaults_are_the_engines() {
        assert_eq!(
            pdfcer_3d_default::PARALLEL,
            pdfcer_3d::DEFAULT_PARALLEL_DEGREES
        );
        assert_eq!(pdfcer_3d_default::FLAT, pdfcer_3d::DEFAULT_FLAT_DEGREES);
    }
}
