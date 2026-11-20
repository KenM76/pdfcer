//! Pass 724.0: writes `data.js` for `webgl/bounce_build.html`, which builds
//! a frame's bounce on the GPU and compares it with the CPU's atlas.
//!
//! Usage: `bounce_build <room | model.glb> <out/data.js>`.

use measure_3d::webgl::{b64, b64_bytes, js_str, load};
use pdfcer_3d::{Bounds, Camera, PreparedModel, RenderOptions};
use std::fmt::Write as _;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = load(&args[1]);
    let out = &args[2];
    let bounds = Bounds::of(&model.meshes).expect("bounds");
    let camera = Camera::fit(&bounds, [-1.0, -1.3, -0.8], [0.0, 0.0, 1.0], true, 1.5).unwrap();
    let options = RenderOptions {
        threads: 0,
        ..RenderOptions::default()
    };
    let prepared = PreparedModel::new(&model);
    let t = Instant::now();
    let cpu = prepared.frame_bounce(&model, &camera, &options).unwrap();
    let cpu_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let gpu = prepared
        .gpu_bounce_build(&model, &camera, &options)
        .unwrap();
    let pack_ms = t.elapsed().as_secs_f64() * 1e3;
    let (Some(cpu), Some(gpu)) = (cpu, gpu) else {
        panic!("no bounce for this model");
    };
    let vec3s = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
    let mut js = String::new();
    let tex = |js: &mut String, name: &str, t: &pdfcer_3d::FloatTexture| {
        let _ = writeln!(
            js,
            "D.{name} = {{w:{}, h:{}, b:\"{}\"}};",
            t.width,
            t.height,
            b64(&t.texels)
        );
    };
    js.push_str("const D = {};\n");
    let _ = writeln!(
        js,
        "D.vs = {};",
        js_str(pdfcer_3d::GLSL_ES_300_BOUNCE_BUILD_VERTEX)
    );
    let _ = writeln!(
        js,
        "D.trace = {};",
        js_str(pdfcer_3d::GLSL_ES_300_BOUNCE_TRACE_FRAGMENT)
    );
    let _ = writeln!(
        js,
        "D.reduce = {};",
        js_str(pdfcer_3d::GLSL_ES_300_BOUNCE_REDUCE_FRAGMENT)
    );
    tex(&mut js, "nodes", &gpu.nodes);
    tex(&mut js, "tris", &gpu.triangles);
    tex(&mut js, "lamps", &gpu.lamps);
    tex(&mut js, "uvs", &gpu.uvs);
    tex(&mut js, "materials", &gpu.materials);
    let _ = writeln!(
        js,
        "D.pictures = {{w:{}, h:{}, l:{}, b:\"{}\"}};",
        gpu.pictures.width,
        gpu.pictures.height,
        gpu.pictures.layers,
        b64_bytes(&gpu.pictures.texels)
    );
    tex(&mut js, "probes", &gpu.probes);
    let b = &gpu.bounce;
    let _ = writeln!(
        js,
        "Object.assign(D, {{lamp_count:{}, probe_count:{}, rays:{:?}, texels:{:?}, far:{}, cap:{}, eps:{}, sky:{:?}, tw:{}, th:{}, bounces:{}, aw:{}, ah:{}, spacing:{:?}, dims:{:?}, cpu_ms:{cpu_ms}, pack_ms:{pack_ms}, triangles:{}}});",
        gpu.lamp_count,
        gpu.probe_count,
        vec3s(&gpu.rays),
        vec3s(&gpu.texels),
        gpu.far,
        gpu.cap,
        gpu.eps,
        gpu.sky,
        gpu.trace_width,
        gpu.trace_height,
        gpu.bounces,
        b.atlas_width,
        b.atlas_height,
        b.spacing,
        b.dims,
        gpu.triangles.texels.len() / 16,
    );
    let _ = writeln!(js, "D.cpu = \"{}\";", b64(&cpu.atlas));
    std::fs::write(out, js).expect("written");
    println!(
        "cpu {cpu_ms:.0} ms, pack {pack_ms:.0} ms, probes {}, lamps {}, atlas {}x{}",
        gpu.probe_count, gpu.lamp_count, b.atlas_width, b.atlas_height
    );
}
