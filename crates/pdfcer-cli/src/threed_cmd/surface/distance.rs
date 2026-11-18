//! `--measure-distance A B`: nearest, farthest and centre-to-centre
//! between two picked entities.

use super::SurfaceArgs;
use super::edge::picked;
use super::target::{Kind, Onto};

impl SurfaceArgs {
    /// One `measure-distance` line per `--measure-distance` pair, then what
    /// was fitted.
    pub(super) fn print_distances(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
    ) {
        use pdfcer_3d::{MeasureEntity, Selection};
        let mut measured = false;
        for [a, b] in self.measure_distance.as_chunks::<2>().0 {
            let head = format!("measure-distance a={a} b={b}");
            let select = |t: &super::target::Target| {
                let pick = picked(&head, t.at[0], t.at[1], model, camera, options)?;
                let entity = match t.kind {
                    Kind::Vertex => MeasureEntity::Vertex,
                    Kind::Edge => MeasureEntity::Edge,
                    Kind::Face => MeasureEntity::Face,
                };
                Some(Selection { pick, entity })
            };
            let (Some(sa), Some(sb)) = (select(a), select(b)) else {
                continue;
            };
            let d = pdfcer_3d::entity_distance(
                &model.meshes,
                &sa,
                &sb,
                self.measure_flat_tolerance,
                self.measure_corner_tolerance,
            );
            match d {
                Err(err) => println!("{head} refused: {err}"),
                Ok(d) => {
                    measured = true;
                    let normal = self.measure_project_onto.map(|onto| match onto {
                        Onto::Screen => pdfcer_3d::vec3::sub(camera.target, camera.eye),
                        Onto::Normal(n) => n,
                    });
                    println!("{head} {}", distance_line(&d, model.units, normal));
                }
            }
        }
        if measured {
            println!(
                "note: measure-distance: maximum is between the entities' corners, so a \
                 curved entity's can fall short of the true curve's; a circular edge's \
                 centre is fitted, not read from the file"
            );
        }
    }
}

/// The figures of one distance.
fn distance_line(
    d: &pdfcer_3d::EntityDistance,
    units: Option<pdfcer_3d::ModelUnits>,
    normal: Option<[f64; 3]>,
) -> String {
    let triple = |v: [f64; 3]| v.map(|c| format!("{c:.6}")).join(",");
    let span = |name: &str, s: &pdfcer_3d::Span| {
        let mm = units.map_or(String::new(), |u| {
            format!(" ({:.6} mm)", s.length * u.mm_per_unit)
        });
        let projected = normal
            .and_then(|n| Some((s.projected(n)?, s.along(n)?)))
            .map_or(String::new(), |(p, along)| {
                format!(" {name}_projected={:.6} {name}_normal={along:.6}", p.length)
            });
        format!(
            "{name}={:.6}{mm} {name}_from={} {name}_to={} {name}_delta={}{projected}",
            s.length,
            triple(s.from),
            triple(s.to),
            triple(s.delta()),
        )
    };
    let centres = d
        .centres
        .map_or("centres=none".to_owned(), |c| span("centres", &c));
    format!(
        "{} {} {centres}",
        span("minimum", &d.minimum),
        span("maximum", &d.maximum)
    )
}
