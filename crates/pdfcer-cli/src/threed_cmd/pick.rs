//! `3d-render --pick` and `--project`: the model point drawn at an image
//! position and the way back, through the camera the image was rendered with.

use super::RenderThreeDArgs;
use pdfcer_3d::{Camera, TriangleMesh, pick};

/// One `pick` line per `--pick`: the hit, or `miss` over the background.
pub(super) fn print_picks(a: &RenderThreeDArgs<'_>, meshes: &[TriangleMesh], camera: &Camera) {
    let fmt3 = |p: [f64; 3]| p.map(|c| format!("{c:.6}")).join(",");
    for &[x, y] in a.pick {
        let at = format!("pick x={x} y={y}");
        match camera.ray_through(x, y, a.width, a.height) {
            Err(err) => println!("{at} refused: {err}"),
            Ok(ray) => match pick(meshes, &ray) {
                None => println!("{at} miss"),
                Some(hit) => println!(
                    "{at} mesh={} triangle={} point={} normal={} distance={:.6}",
                    hit.mesh,
                    hit.triangle,
                    fmt3(hit.point),
                    fmt3(hit.normal),
                    hit.distance
                ),
            },
        }
    }
}

/// One `project` line per `--project`: the image position, or `not drawn`.
pub(super) fn print_projects(a: &RenderThreeDArgs<'_>, camera: &Camera) {
    for &point in a.project {
        let at = format!("project point={}", point.map(|c| c.to_string()).join(","));
        match camera.pixel_of(point, a.width, a.height) {
            None => println!("{at} not drawn"),
            Some([x, y]) => println!("{at} x={x:.6} y={y:.6}"),
        }
    }
}
