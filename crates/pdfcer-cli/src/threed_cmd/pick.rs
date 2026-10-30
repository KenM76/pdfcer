//! `3d-render --pick`: the model point drawn at an image position, through
//! the camera the image was rendered with.

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
