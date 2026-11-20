//! Pass 724.2: builds a light meter's bounce on the GPU and compares its
//! readings with the CPU-built meter's.
//!
//! Usage: `meter_build <room | roomglow | model.glb> <out/data.js>` writes
//! `data.js` for `webgl/meter_build.html`, which prints the built atlas;
//! `meter_build <model> --compare <atlas.txt>` finishes the meter with it
//! and compares every lattice point's figure with the CPU's.

use measure_3d::webgl::{b64, b64_bytes, js_str, load, surface_queries, texture_line};
use pdfcer_3d::{
    Bounds, Camera, IlluminanceMap, LightMeter, MapSide, MeterSettings, PreparedModel,
    RenderOptions,
};
use std::fmt::Write as _;
use std::time::Instant;

fn figures(m: &IlluminanceMap) -> Vec<f64> {
    m.meshes
        .iter()
        .flatten()
        .flatten()
        .flat_map(|p| p.figures.clone())
        .collect()
}

fn unb64(s: &str) -> Vec<f32> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let s = s.trim().trim_end_matches('=').as_bytes();
    let mut bytes = Vec::with_capacity(s.len() * 3 / 4);
    for c in s.chunks(4) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (k, &c)| n | u32::from(val(c)) << (18 - 6 * k));
        for k in 0..c.len() - 1 {
            bytes.push((n >> (16 - 8 * k)) as u8);
        }
    }
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = load(&args[1]);
    let bounds = Bounds::of(&model.meshes).expect("bounds");
    let camera = Camera::fit(&bounds, [-1.0, -1.3, -0.8], [0.0, 0.0, 1.0], true, 1.5).unwrap();
    let options = RenderOptions {
        threads: 0,
        ..RenderOptions::default()
    };
    let prepared = PreparedModel::new(&model);
    let settings = MeterSettings::default();
    let t = Instant::now();
    let gpu = LightMeter::gpu_new(&model, &prepared, &camera, &options, &settings).unwrap();
    let pack_ms = t.elapsed().as_secs_f64() * 1e3;
    if args[2] == "--compare" {
        let atlas = unb64(&std::fs::read_to_string(&args[3]).expect("atlas"));
        let gpu = gpu.finish(&atlas).unwrap();
        let t = Instant::now();
        let cpu = LightMeter::new(&model, &prepared, &camera, &options, &settings).unwrap();
        let cpu_ms = t.elapsed().as_secs_f64() * 1e3;
        let small = model
            .meshes
            .iter()
            .map(|m| m.triangles.len())
            .sum::<usize>()
            < 5000;
        let (a, b) = if small {
            let map = |m: &LightMeter| {
                figures(&m.map(None, MapSide::FacingCamera, &mut |_| true).unwrap())
            };
            (map(&cpu), map(&gpu))
        } else {
            let queries = surface_queries(&model, 400);
            let read = |m: &LightMeter| -> Vec<f64> {
                queries
                    .iter()
                    .map(|q| m.read(q).map_or(f64::NAN, |r| r.total))
                    .collect()
            };
            (read(&cpu), read(&gpu))
        };
        let (mut sum, mut abs, mut max, mut big, mut total) = (0.0, 0.0, 0.0f64, 0, 0.0);
        let pairs: Vec<(f64, f64)> = a
            .into_iter()
            .zip(b)
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .collect();
        let (a, b): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
        for (x, y) in a.iter().zip(&b) {
            let d = y - x;
            sum += d;
            abs += d.abs();
            max = max.max(d.abs());
            total += x;
            if d.abs() > 0.05 * x.abs().max(0.02) {
                big += 1;
            }
        }
        let n = a.len().max(1) as f64;
        println!(
            "cpu meter {cpu_ms:.0} ms, pack {pack_ms:.0} ms; points {}, cpu mean {:.5}, gpu-cpu mean {:.3e}, mean |d| {:.3e}, max |d| {:.3e}, >5% {:.3}%",
            a.len(),
            total / n,
            sum / n,
            abs / n,
            max,
            100.0 * f64::from(big) / n
        );
        return;
    }
    let Some(b) = &gpu.build else {
        panic!("no GPU bounce for this model");
    };
    let vec3s = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
    let mut js = String::from("const D = {};\n");
    for (name, src) in [
        ("vs", pdfcer_3d::GLSL_ES_300_BOUNCE_BUILD_VERTEX),
        ("trace", pdfcer_3d::GLSL_ES_300_BOUNCE_TRACE_FRAGMENT),
        ("reduce", pdfcer_3d::GLSL_ES_300_BOUNCE_REDUCE_FRAGMENT),
    ] {
        let _ = writeln!(js, "D.{name} = {};", js_str(src));
    }
    for (name, t) in [
        ("nodes", &b.nodes),
        ("tris", &b.triangles),
        ("lamps", &b.lamps),
        ("uvs", &b.uvs),
        ("materials", &b.materials),
        ("probes", &b.probes),
        ("glow", &b.glow),
    ] {
        js.push_str(&texture_line(name, t));
    }
    let _ = writeln!(
        js,
        "D.pictures = {{w:{}, h:{}, l:{}, b:\"{}\"}};",
        b.pictures.width,
        b.pictures.height,
        b.pictures.layers,
        b64_bytes(&b.pictures.texels)
    );
    let f = &b.bounce;
    let _ = writeln!(
        js,
        "Object.assign(D, {{lamp_count:{}, probe_count:{}, glow_count:{}, rays:{:?}, texels:{:?}, far:{}, cap:{}, eps:{}, sky:{:?}, tw:{}, th:{}, bounces:{}, aw:{}, ah:{}, spacing:{:?}, dims:{:?}, pack_ms:{pack_ms}, triangles:{}}});",
        b.lamp_count,
        b.probe_count,
        b.glow_count,
        vec3s(&b.rays),
        vec3s(&b.texels),
        b.far,
        b.cap,
        b.eps,
        b.sky,
        b.trace_width,
        b.trace_height,
        b.bounces,
        f.atlas_width,
        f.atlas_height,
        f.spacing,
        f.dims,
        b.triangles.texels.len() / 16,
    );
    let _ = b64(&[]);
    std::fs::write(&args[2], js).expect("written");
    println!(
        "pack {pack_ms:.0} ms, probes {}, lamps {}, glow {}, atlas {}x{}",
        b.probe_count, b.lamp_count, b.glow_count, f.atlas_width, f.atlas_height
    );
}
