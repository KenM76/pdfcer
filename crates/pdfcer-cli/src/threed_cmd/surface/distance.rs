//! `--measure-distance A B`: nearest, farthest and centre-to-centre
//! between two picked entities.

use super::SurfaceArgs;
use super::edge::picked;
use super::target::{Kind, Onto};

impl SurfaceArgs {
    /// The `--measure-frame` system, when given (the parser refused the
    /// frames this cannot build).
    pub(super) fn frame(&self) -> Option<pdfcer_3d::CoordinateFrame> {
        let f = self.measure_frame?;
        pdfcer_3d::CoordinateFrame::new(f.origin, f.x_axis, f.y_toward)
    }

    /// The model-space normal of the --measure-project-onto plane.
    pub(super) fn projection_normal(&self, camera: &pdfcer_3d::Camera) -> Option<[f64; 3]> {
        self.measure_project_onto.map(|onto| match onto {
            Onto::Screen => pdfcer_3d::vec3::sub(camera.target, camera.eye),
            Onto::Normal(n) => n,
        })
    }

    /// The note naming the `--measure-frame` axes, after the lines that
    /// used them.
    pub(super) fn print_frame_note(&self) {
        let Some(f) = self.frame() else { return };
        let triple = |v: [f64; 3]| v.map(|c| format!("{c:.6}")).join(",");
        let [x, y, z] = f.axes();
        println!(
            "note: measure-frame: _frame figures are in the system at origin={} with \
             x={} y={} z={} (Y squared to X, Z = X x Y)",
            triple(f.origin()),
            triple(x),
            triple(y),
            triple(z)
        );
    }

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
                    let normal = self.projection_normal(camera);
                    let frame = self.frame();
                    println!("{head} {}", distance_line(&d, model.units, normal, frame));
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
    frame: Option<pdfcer_3d::CoordinateFrame>,
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
        let local = frame.map_or(String::new(), |f| {
            format!(
                " {name}_from_frame={} {name}_to_frame={} {name}_delta_frame={}",
                triple(f.point_to_local(s.from)),
                triple(f.point_to_local(s.to)),
                triple(f.vector_to_local(s.delta())),
            )
        });
        format!(
            "{name}={:.6}{mm} {name}_from={} {name}_to={} {name}_delta={}{projected}{local}",
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
