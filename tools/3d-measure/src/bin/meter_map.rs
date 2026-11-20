//! Pass 724.1: writes `data.js` for `webgl/meter_map.html`, which measures
//! a light-meter map on the GPU and compares each point with the CPU's.
//!
//! Usage: `meter_map <room | model.glb> <spacing | none> <bounce | flat>
//! <out/data.js> [gather_rays]`.

use measure_3d::webgl::{b64, b64_bytes, js_str, load, texture_line};
use pdfcer_3d::{
    Bounce, Bounds, Camera, LightMeter, MapSide, MeterSettings, PreparedModel, RenderOptions,
};
use std::fmt::Write as _;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = load(&args[1]);
    let spacing = args[2].parse::<f64>().ok();
    let out = &args[4];
    let bounds = Bounds::of(&model.meshes).expect("bounds");
    let camera = Camera::fit(&bounds, [-1.0, -1.3, -0.8], [0.0, 0.0, 1.0], true, 1.5).unwrap();
    let mut options = RenderOptions {
        threads: 0,
        ..RenderOptions::default()
    };
    if args[3] == "flat" {
        options.indirect.bounce = Bounce::Off;
    }
    let prepared = PreparedModel::new(&model);
    let mut settings = MeterSettings::default();
    if let Some(g) = args.get(5).and_then(|g| g.parse().ok()) {
        settings.gather_rays = g;
    }
    if args.get(6).is_some_and(|g| g == "noglow") {
        settings.glow = pdfcer_3d::GlowLight::LeftOut;
    }
    let t = Instant::now();
    let meter = LightMeter::new(&model, &prepared, &camera, &options, &settings).unwrap();
    let new_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let cpu = meter
        .map(spacing, MapSide::FacingCamera, &mut |_| true)
        .unwrap();
    let cpu_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let gpu = meter
        .gpu_map(spacing, MapSide::FacingCamera)
        .unwrap()
        .expect("fits the GPU");
    let pack_ms = t.elapsed().as_secs_f64() * 1e3;
    let points = gpu.point_count as usize;
    let size = 4 * (gpu.target_width * gpu.target_height) as usize;
    // Finishing with point i's texel reading i gives each lattice point's
    // index, plus the ambient a zero texel finishes to.
    let ambient = gpu.clone().finish(&vec![0.0; size]).unwrap();
    let ambient = ambient.range().map_or(0.0, |r| r.0);
    let mut index = vec![0.0f32; size];
    for i in 0..points {
        index[4 * i] = i as f32;
    }
    let indexed = gpu.clone().finish(&index).unwrap();
    let mut per_point = vec![f32::NAN; points];
    let patches = |m: &pdfcer_3d::IlluminanceMap| -> Vec<Vec<f64>> {
        m.meshes
            .iter()
            .flatten()
            .flatten()
            .map(|p| p.figures.clone())
            .collect()
    };
    for (slots, figures) in patches(&indexed).iter().zip(patches(&cpu)) {
        for (slot, figure) in slots.iter().zip(figures) {
            let i = (slot - ambient).round() as usize;
            if let Some(v) = per_point.get_mut(i) {
                *v = (figure - ambient) as f32;
            }
        }
    }
    let mut js = String::from("const D = {};\n");
    for (name, src) in [
        ("vs", pdfcer_3d::GLSL_ES_300_BOUNCE_BUILD_VERTEX),
        ("fs", pdfcer_3d::GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT),
    ] {
        let _ = writeln!(js, "D.{name} = {};", js_str(src));
    }
    for (name, t) in [
        ("nodes", &gpu.nodes),
        ("tris", &gpu.triangles),
        ("uvs", &gpu.uvs),
        ("materials", &gpu.materials),
        ("lamps", &gpu.lamps),
        ("glow", &gpu.glow),
        ("points", &gpu.points),
        ("normals", &gpu.normals),
        ("directions", &gpu.directions),
    ] {
        js.push_str(&texture_line(name, t));
    }
    let _ = writeln!(
        js,
        "D.pictures = {{w:{}, h:{}, l:{}, b:\"{}\"}};",
        gpu.pictures.width,
        gpu.pictures.height,
        gpu.pictures.layers,
        b64_bytes(&gpu.pictures.texels)
    );
    match &gpu.bounce {
        Some(b) => {
            let _ = writeln!(
                js,
                "D.bounce = {{w:{}, h:{}, origin:{:?}, spacing:{:?}, dims:{:?}, strength:{}, b:\"{}\"}};",
                b.atlas_width,
                b.atlas_height,
                b.origin,
                b.spacing,
                b.dims,
                b.strength,
                b64(&b.atlas)
            );
        }
        None => js.push_str("D.bounce = null;\n"),
    }
    let _ = writeln!(
        js,
        "Object.assign(D, {{lamp_count:{}, bounce_first:{}, bounce_count:{}, glow_count:{}, point_count:{}, direction_count:{}, far:{}, gather_far:{}, eps:{}, sky:{:?}, tw:{}, th:{}, new_ms:{new_ms}, cpu_ms:{cpu_ms}, pack_ms:{pack_ms}, triangles:{}}});",
        gpu.lamp_count,
        gpu.bounce_first,
        gpu.bounce_count,
        gpu.glow_count,
        gpu.point_count,
        gpu.direction_count,
        gpu.far,
        gpu.gather_far,
        gpu.eps,
        gpu.sky,
        gpu.target_width,
        gpu.target_height,
        gpu.triangles.texels.len() / 16,
    );
    let _ = writeln!(js, "D.cpu = \"{}\";", b64(&per_point));
    std::fs::write(out, js).expect("written");
    println!(
        "meter {new_ms:.0} ms, cpu map {cpu_ms:.0} ms, pack {pack_ms:.0} ms, points {points}, lamps {}, bounce {}, directions {}",
        gpu.lamp_count,
        gpu.bounce.is_some(),
        gpu.direction_count
    );
}
