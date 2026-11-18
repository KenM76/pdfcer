//! `--measure-edge` and `--measure-vertex`: one picked model edge or vertex.

use super::SurfaceArgs;

/// The pick under image position `x`,`y`; `None` after printing why there
/// is none on the line headed `at`.
pub(super) fn picked(
    at: &str,
    x: f64,
    y: f64,
    model: &pdfcer_3d::AssembledModel,
    camera: &pdfcer_3d::Camera,
    options: &pdfcer_3d::RenderOptions,
) -> Option<pdfcer_3d::SurfacePick> {
    let hit = camera
        .ray_through(x, y, options.width, options.height)
        .map_err(|err| format!("refused: {err}"))
        .and_then(|ray| pdfcer_3d::pick(&model.meshes, &ray).ok_or_else(|| "miss".to_owned()));
    match hit {
        Ok(hit) => Some(pdfcer_3d::SurfacePick::from(&hit)),
        Err(why) => {
            println!("{at} {why}");
            None
        }
    }
}

impl SurfaceArgs {
    /// One `measure-edge` line per `--measure-edge` and one
    /// `measure-vertex` line per `--measure-vertex`, then what was fitted.
    pub(super) fn print_edges(
        &self,
        model: &pdfcer_3d::AssembledModel,
        camera: &pdfcer_3d::Camera,
        options: &pdfcer_3d::RenderOptions,
    ) {
        let triple = |v: [f64; 3]| v.map(|c| format!("{c:.6}")).join(",");
        let mut measured = false;
        for &[x, y] in &self.measure_edge {
            let at = format!("measure-edge at={x},{y}");
            let Some(pick) = picked(&at, x, y, model, camera, options) else {
                continue;
            };
            let edge = pdfcer_3d::edge_at(
                &model.meshes,
                &pick,
                self.measure_flat_tolerance,
                self.measure_corner_tolerance,
            );
            match edge {
                Err(err) => println!("{at} refused: {err}"),
                Ok(e) => {
                    measured = true;
                    println!(
                        "{at} triangle={} {}",
                        pick.triangle,
                        edge_line(&e, model.units)
                    );
                }
            }
        }
        for &[x, y] in &self.measure_vertex {
            let at = format!("measure-vertex at={x},{y}");
            let Some(pick) = picked(&at, x, y, model, camera, options) else {
                continue;
            };
            match pdfcer_3d::vertex_at(&model.meshes, &pick) {
                Err(err) => println!("{at} refused: {err}"),
                Ok(p) => {
                    let mm = model.units.map_or(String::new(), |u| {
                        format!(" point_mm={}", triple(p.map(|c| c * u.mm_per_unit)))
                    });
                    println!(
                        "{at} triangle={} mesh={} point={}{mm}",
                        pick.triangle,
                        pick.mesh,
                        triple(p)
                    );
                }
            }
        }
        if measured {
            println!(
                "note: measure-edge: an edge is where two faces meet in the model's \
                 triangles, and its line or circle is fitted to its corners, not read from \
                 the file; residual is the farthest a corner lies off the fit"
            );
        }
    }
}

/// The figures of one edge.
fn edge_line(e: &pdfcer_3d::EdgeFigures, units: Option<pdfcer_3d::ModelUnits>) -> String {
    use pdfcer_3d::EdgeShape;
    let mm = |v: f64| units.map_or(String::new(), |u| format!(" ({:.6} mm)", v * u.mm_per_unit));
    let triple = |v: [f64; 3]| v.map(|c| format!("{c:.6}")).join(",");
    let face = |f: Option<usize>| f.map_or("none".to_owned(), |i| i.to_string());
    let shape = match e.shape {
        EdgeShape::Line { direction } => format!("shape=line direction={}", triple(direction)),
        EdgeShape::Circle {
            centre,
            axis,
            radius,
            residual,
            sweep_degrees,
            arc_length,
        } => format!(
            "shape=circle radius={radius:.6}{} diameter={:.6}{} centre={} axis={} \
             sweep={sweep_degrees:.6} arc_length={arc_length:.6}{} residual={residual:.6}",
            mm(radius),
            2.0 * radius,
            mm(2.0 * radius),
            triple(centre),
            triple(axis),
            mm(arc_length),
        ),
        _ => "shape=curve".to_owned(),
    };
    let [f, g] = e.faces;
    format!(
        "mesh={} faces={},{} points={} closed={} length={:.6}{} chord={:.6}{} {shape}",
        e.mesh,
        face(f),
        face(g),
        e.points.len(),
        if e.closed { "yes" } else { "no" },
        e.length,
        mm(e.length),
        e.chord,
        mm(e.chord),
    )
}
