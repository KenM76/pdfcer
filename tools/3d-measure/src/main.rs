//! Corpus counts and ray speed for Pass 705.0 (decision 224).
//!
//! Usage: `measure-3d <model.prc>...`. Per model it prints the model-fixed
//! lights by kind (occurrence and view lights), the finished meshes, how
//! many have white specular >= 0.9 in every channel, how many glow
//! (non-zero emissive), texture notes, and `PickIndex` closest-hit rays per
//! second for rays aimed through the model's bounds. A totals line ends
//! the run. The rays come from a fixed LCG so runs repeat.

use pdfcer_3d::{AssembledModel, ModelLight, ModelLightKind, PickIndex};
use std::time::Instant;

#[derive(Default)]
struct Totals {
    models: usize,
    with_fixed_lights: usize,
    finished: usize,
    white_spec: usize,
    glowing: usize,
    meshes: usize,
    rays: u64,
    ray_secs: f64,
}

fn kind(l: &ModelLight) -> &'static str {
    match l.kind {
        ModelLightKind::Ambient => "ambient",
        ModelLightKind::Point { .. } => "point",
        ModelLightKind::Directional { .. } => "directional",
        ModelLightKind::Spot { .. } => "spot",
        _ => "other",
    }
}

fn kinds<'a>(lights: impl Iterator<Item = &'a ModelLight>) -> String {
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for l in lights {
        *counts.entry(kind(l)).or_default() += 1;
    }
    counts
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Closest-hit rays per second: from random points on a sphere around the
/// bounds towards random points inside them.
fn ray_rate(m: &AssembledModel, t: &mut Totals) -> Option<(f64, f64)> {
    let (lo, hi) = measure_3d::bounds(m)?;
    let index = PickIndex::new(&m.meshes);
    let n = 200_000u64;
    let start = Instant::now();
    let hits = measure_3d::trace(&index, lo, hi, n);
    let secs = start.elapsed().as_secs_f64();
    t.rays += n;
    t.ray_secs += secs;
    Some((n as f64 / secs, hits as f64 / n as f64))
}

fn measure(path: &str, t: &mut Totals) {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => return println!("{path}\tunreadable: {e}"),
    };
    let m = match pdfcer_3d::assemble(&data) {
        Ok(m) => m,
        Err(e) => return println!("{path}\tnot assembled: {e}"),
    };
    t.models += 1;
    let view_lights = m.view_lights.iter().flat_map(|v| v.lights.iter());
    let fixed = m
        .lights
        .iter()
        .chain(view_lights.clone())
        .filter(|l| !matches!(l.kind, ModelLightKind::Ambient))
        .count();
    t.with_fixed_lights += usize::from(fixed > 0);
    let finished: Vec<_> = m.finishes.iter().flatten().collect();
    let white = finished
        .iter()
        .filter(|f| f.specular.iter().all(|&c| c >= 0.9))
        .count();
    let glow = finished
        .iter()
        .filter(|f| f.emissive.iter().any(|&c| c > 0.0))
        .count();
    if std::env::var_os("MEASURE_FINISHES").is_some() {
        let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
        for f in &finished {
            let key = format!(
                "spec={:.2?} emis={:.2?} amb={:.2?} shin={:.1}",
                f.specular, f.emissive, f.ambient, f.shininess
            );
            *seen.entry(key).or_default() += 1;
        }
        for (k, n) in seen {
            println!("  finish x{n}: {k}");
        }
    }
    t.meshes += m.meshes.len();
    t.finished += finished.len();
    t.white_spec += white;
    t.glowing += glow;
    let rate = ray_rate(&m, t);
    println!(
        "{path}\tmeshes={} tris={} occurrence_lights=[{}] view_lights=[{}] finished={} white_spec={} glow={} textures={} notes={:?} rays_per_s={} hit_frac={}",
        m.meshes.len(),
        m.triangles,
        kinds(m.lights.iter()),
        kinds(view_lights),
        finished.len(),
        white,
        glow,
        m.textures.len(),
        m.texture_notes,
        rate.map_or("-".into(), |r| format!("{:.0}", r.0)),
        rate.map_or("-".into(), |r| format!("{:.2}", r.1)),
    );
}

fn main() {
    let mut t = Totals::default();
    for path in std::env::args().skip(1) {
        measure(&path, &mut t);
    }
    let pct = |a: usize, b: usize| {
        if b == 0 {
            0.0
        } else {
            100.0 * a as f64 / b as f64
        }
    };
    println!(
        "TOTAL models={} with_model_fixed_lights={} meshes={} finished={} white_spec={} ({:.1}% of finished) glow={} rays_per_s_overall={:.0}",
        t.models,
        t.with_fixed_lights,
        t.meshes,
        t.finished,
        t.white_spec,
        pct(t.white_spec, t.finished),
        t.glowing,
        if t.ray_secs > 0.0 {
            t.rays as f64 / t.ray_secs
        } else {
            0.0
        },
    );
}
