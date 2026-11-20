//! Pass 724.3: writes `data.js` for `webgl/frame_tiles.html`, which draws a
//! frame with and without its lamp tiles and compares them byte for byte.
//!
//! Usage: `frame_tiles <room | grid | model.glb> <width> <height> <out/data.js>`.

use measure_3d::webgl::{b64, js_str, load};
use pdfcer_3d::{
    AssembledModel, Bounds, Camera, FrameLights, GLSL_ES_300_FRAGMENT, GLSL_ES_300_VERTEX,
    LIGHT_TABLE_WIDTH, ModelLight, ModelLightKind, PreparedModel, RenderOptions,
};
use std::fmt::Write as _;

/// A floor under a 40 × 40 grid of short-reach lamps.
fn grid() -> AssembledModel {
    let mut m = load("room");
    m.lights.clear();
    for i in 0..40 {
        for j in 0..40 {
            m.lights.push(ModelLight::new(
                ModelLightKind::Point {
                    position: [
                        0.0125 + 0.025 * f64::from(i),
                        0.0125 + 0.025 * f64::from(j),
                        0.05,
                    ],
                    attenuation: [1.0, 0.0, 400_000_000.0],
                },
                [0.6, 0.5, 0.4],
            ));
        }
    }
    m
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = if args[1] == "grid" {
        grid()
    } else {
        load(&args[1])
    };
    let (w, h): (u32, u32) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let bounds = Bounds::of(&model.meshes).expect("bounds");
    let camera = Camera::fit(&bounds, [-1.0, -1.3, -0.8], [0.0, 0.0, 1.0], true, 1.2).unwrap();
    let options = RenderOptions {
        width: w,
        height: h,
        threads: 0,
        ..RenderOptions::default()
    };
    let prepared = PreparedModel::new(&model);
    let data = prepared.draw_data(&model, &options);
    let lights = FrameLights::for_model(&camera, &options, &model.lights).unwrap();
    let aspect = f64::from(w) / f64::from(h);
    let view = camera.view_matrix().unwrap();
    let proj = camera.projection_matrix(aspect, &bounds).unwrap();
    let depth = camera.depth_range(aspect, &bounds).unwrap();
    let tiles = lights.tiles.texture();
    let mut s = String::new();
    let _ = writeln!(s, "const D = {{}};");
    let _ = writeln!(
        s,
        "D.vs = {}; D.fs = {};",
        js_str(GLSL_ES_300_VERTEX),
        js_str(GLSL_ES_300_FRAGMENT)
    );
    let _ = writeln!(
        s,
        "D.w = {w}; D.h = {h}; D.vertices = \"{}\";",
        b64(&data.vertices)
    );
    let draws: Vec<String> = data
        .draws
        .iter()
        .filter(|d| !d.translucent)
        .map(|d| format!("[{},{},{:?}]", d.first, d.count, d.colour))
        .collect();
    let _ = writeln!(s, "D.draws = [{}];", draws.join(","));
    let _ = writeln!(
        s,
        "D.lights = {{w:{}, h:{}, b:\"{}\"}};",
        LIGHT_TABLE_WIDTH,
        lights.light_table_rows(),
        b64(&lights.light_table())
    );
    let _ = writeln!(
        s,
        "D.tiles = {{w:{}, h:{}, b:\"{}\"}};",
        tiles.width.max(1),
        tiles.height.max(1),
        b64(&tiles.texels)
    );
    let t = &lights.tiles;
    let longest = t.lists.iter().map(Vec::len).max().unwrap_or(0);
    let mean = t.lists.iter().map(Vec::len).sum::<usize>() as f64 / t.lists.len().max(1) as f64;
    let _ = writeln!(
        s,
        "D.tile = [{}, {}, {}, {}]; D.longest = {longest}; D.mean = {mean:.1};",
        t.tile_size,
        t.columns,
        t.rows,
        t.lists.len()
    );
    let _ = writeln!(
        s,
        "D.view = {view:?}; D.proj = {proj:?}; D.depth = {depth:?}; D.mode = {}; D.ambient = {:?}; D.count = {}; D.local_count = {}; D.eye = {:?}; D.forward = {:?}; D.ortho = {}; D.toward_white = {};",
        lights.mode.code(),
        lights.ambient,
        lights.count,
        lights.local_count,
        lights.eye,
        lights.forward,
        lights.orthographic,
        lights.toward_white
    );
    std::fs::write(&args[4], s).unwrap();
    println!(
        "lamps {} tiles {}x{} longest {longest} mean {mean:.1}",
        lights.local_count, t.columns, t.rows
    );
}
