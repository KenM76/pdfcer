//! Pass 724.2: writes `data.js` for `webgl/meter_map.html` holding many
//! light-meter point readings' GPU gathers, each compared with the CPU's
//! bounced plus glow light.
//!
//! Usage: `meter_read <room | roomglow | model.glb> <count> <out/data.js>`.

use measure_3d::webgl::{b64, b64_bytes, js_str, load, surface_queries, texture_line};
use pdfcer_3d::{
    Bounds, Camera, FloatTexture, LightMeter, MeterSettings, PreparedModel, RenderOptions,
};
use std::fmt::Write as _;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = load(&args[1]);
    let count: usize = args[2].parse().expect("count");
    let bounds = Bounds::of(&model.meshes).expect("bounds");
    let camera = Camera::fit(&bounds, [-1.0, -1.3, -0.8], [0.0, 0.0, 1.0], true, 1.5).unwrap();
    let options = RenderOptions {
        threads: 0,
        ..RenderOptions::default()
    };
    let prepared = PreparedModel::new(&model);
    let settings = MeterSettings::default();
    let t = Instant::now();
    let meter = LightMeter::new(&model, &prepared, &camera, &options, &settings).unwrap();
    let new_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let scene = meter.gpu_scene().expect("fits the GPU");
    let queries = surface_queries(&model, count);
    let reads: Vec<_> = queries
        .iter()
        .filter_map(|q| meter.gpu_read(q).ok())
        .collect();
    let pack_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let cpu: Vec<f32> = queries
        .iter()
        .filter_map(|q| meter.read(q).ok())
        .map(|r| (r.bounced + r.emitted) as f32)
        .collect();
    let cpu_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(cpu.len(), reads.len());
    let rows = |f: fn(&pdfcer_3d::GpuLightRead) -> &FloatTexture| -> FloatTexture {
        let texels: Vec<[f32; 4]> = reads
            .iter()
            .flat_map(|r| {
                let t = f(r);
                t.texels.as_chunks::<4>().0[..r.point_count as usize].to_vec()
            })
            .collect();
        let height = texels.len().div_ceil(2048).max(1);
        let mut flat: Vec<f32> = texels.into_iter().flatten().collect();
        flat.resize(2048 * height * 4, 0.0);
        let mut out = f(&reads[0]).clone();
        (out.texels, out.width, out.height) = (flat, 2048, height as u32);
        out
    };
    let points = rows(|r| &r.points);
    let normals = rows(|r| &r.normals);
    let n = reads.len();
    let mut js = String::from("const D = {};\n");
    for (name, src) in [
        ("vs", pdfcer_3d::GLSL_ES_300_BOUNCE_BUILD_VERTEX),
        ("fs", pdfcer_3d::GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT),
    ] {
        let _ = writeln!(js, "D.{name} = {};", js_str(src));
    }
    for (name, t) in [
        ("nodes", &scene.nodes),
        ("tris", &scene.triangles),
        ("uvs", &scene.uvs),
        ("materials", &scene.materials),
        ("lamps", &scene.lamps),
        ("glow", &scene.glow),
        ("points", &points),
        ("normals", &normals),
        ("directions", &scene.directions),
    ] {
        js.push_str(&texture_line(name, t));
    }
    let _ = writeln!(
        js,
        "D.pictures = {{w:{}, h:{}, l:{}, b:\"{}\"}};",
        scene.pictures.width,
        scene.pictures.height,
        scene.pictures.layers,
        b64_bytes(&scene.pictures.texels)
    );
    match &scene.bounce {
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
        "Object.assign(D, {{lamp_count:0, bounce_first:{}, bounce_count:{}, glow_count:{}, point_count:{n}, direction_count:{}, far:{}, gather_far:{}, eps:{}, sky:{:?}, tw:2048, direction_step:16, th:{}, new_ms:{new_ms}, cpu_ms:{cpu_ms}, pack_ms:{pack_ms}, triangles:{}}});",
        scene.bounce_first,
        scene.bounce_count,
        scene.glow_count,
        scene.direction_count,
        scene.far,
        scene.gather_far,
        scene.eps,
        scene.sky,
        n.div_ceil(2048).max(1),
        scene.triangles.texels.len() / 16,
    );
    let _ = writeln!(js, "D.cpu = \"{}\";", b64(&cpu));
    std::fs::write(&args[3], js).expect("written");
    println!("meter {new_ms:.0} ms, {n} cpu reads {cpu_ms:.0} ms, pack {pack_ms:.0} ms");
}
