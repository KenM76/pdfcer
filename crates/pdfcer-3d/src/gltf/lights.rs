//! `KHR_lights_punctual` lights as [`ModelLight`]s in scene space.
//!
//! glTF intensities are physical (candela, lux) and have no ambient term;
//! PRC's are unitless. [`GltfLights::Exposure`] and [`GltfLights::Normalise`]
//! scale every light by one factor (metered on the model's surfaces, or
//! summed at its centre) and add an ambient light of `AMBIENT`, both
//! disclosed as inferred.

use pdfcer_json::Json;

use super::build::srgb;
use super::{GltfImport, GltfLights, member};
use crate::{IDENTITY, MAX_DEPTH, Matrix, ModelLight, ModelLightKind, multiply, transform_point};

/// The ambient light a normalised import adds.
pub(super) const AMBIENT: f64 = 0.2;
/// The nearest a point or spot light is taken to be, as a fraction of the
/// model's diagonal, so a lamp inside the model does not blow out.
const FLOOR: f64 = 0.05;

/// One light as placed: its definition, world position and direction.
pub(super) struct Placed<'a> {
    def: &'a Json,
    pub(super) position: [f64; 3],
    pub(super) direction: [f64; 3],
    /// Whether its colour is linear, to be encoded to sRGB.
    linear: bool,
}

/// Reads the lights the scene's nodes carry into `out.scene.lights`.
/// Call after the roots are chosen and turned.
pub(super) fn lights(doc: &Json, mode: GltfLights, linear: bool, out: &mut GltfImport) {
    let defs = doc
        .get("extensions")
        .and_then(|e| e.get("KHR_lights_punctual"))
        .map_or(&[][..], |k| member(k, "lights"));
    let (mut placed, bounds, instances) = walk(doc, defs, out);
    for p in &mut placed {
        p.linear = linear;
    }
    if placed.is_empty() {
        return;
    }
    if mode == GltfLights::Drop {
        out.notes.push(format!(
            "{} model lights were left out, as asked",
            placed.len()
        ));
        return;
    }
    let diagonal = bounds.map_or(1.0, |(lo, hi)| {
        crate::vec3::length(crate::vec3::sub(hi, lo))
    });
    let centre = bounds.map_or([0.0; 3], |(lo, hi)| each(lo, hi, |l, h| (l + h) / 2.0));
    let floor = (FLOOR * diagonal).max(f64::MIN_POSITIVE);
    let mut mode = mode;
    let scale = match mode {
        GltfLights::Exposure => {
            let lamps: Vec<_> = placed
                .iter()
                .filter_map(|p| {
                    let (kind, colour, intensity) = read(p);
                    let bright = colour.iter().copied().fold(0.0, f64::max) * intensity;
                    super::exposure::lamp(p, kind, bright)
                })
                .collect();
            super::exposure::scale(&lamps, &instances, &out.scene.meshes, floor).unwrap_or_else(
                || {
                    out.notes.push(
                        "no surface is lit by the model's lights, so they were scaled at \
                         its centre instead"
                            .to_owned(),
                    );
                    mode = GltfLights::Normalise;
                    normal_scale(&placed, centre, floor)
                },
            )
        }
        GltfLights::Normalise => normal_scale(&placed, centre, floor),
        GltfLights::Stored | GltfLights::Drop => 1.0,
    };
    let mut spots = 0;
    for (i, p) in placed.iter().enumerate() {
        if let Some(l) = light(p, scale, floor, mode, i, out) {
            spots += usize::from(matches!(l.kind, ModelLightKind::Spot { .. }));
            out.scene.lights.push(l);
        }
    }
    disclose(mode, scale, spots, out);
}

fn disclose(mode: GltfLights, scale: f64, spots: usize, out: &mut GltfImport) {
    let n = out.scene.lights.len();
    let how = match mode {
        GltfLights::Exposure => "the median lit surface of the model gets",
        GltfLights::Normalise => "together they light the model's centre to",
        GltfLights::Stored | GltfLights::Drop => "",
    };
    if !how.is_empty() && n > 0 {
        out.inferred.push(format!(
            "{n} model lights scaled by {scale:.4} so {how} {:.0}% (glTF intensities are \
             physical, PRC's are not), plus an ambient light of {:.0}%",
            100.0 * (1.0 - AMBIENT),
            100.0 * AMBIENT
        ));
        out.scene.lights.push(ModelLight {
            name: Some("ambient".into()),
            ..ModelLight::new(ModelLightKind::Ambient, [AMBIENT; 3])
        });
    }
    if spots > 0 {
        out.inferred.push(format!(
            "{spots} spot lights' soft edge fitted to a falloff exponent (PRC has no inner cone)"
        ));
    }
}

/// The lights the scene's nodes reach, in world space, and the world
/// bounds of the meshes drawn.
type Bounds = Option<([f64; 3], [f64; 3])>;

/// Each drawn mesh and its world matrix.
type Instances = Vec<(usize, Matrix)>;

fn walk<'a>(
    doc: &Json,
    defs: &'a [Json],
    out: &mut GltfImport,
) -> (Vec<Placed<'a>>, Bounds, Instances) {
    let nodes = member(doc, "nodes");
    let local: Vec<_> = out.scene.meshes.iter().map(mesh_bounds).collect();
    let mut placed = Vec::new();
    let mut bounds: Bounds = None;
    let mut instances = Vec::new();
    let mut seen = vec![false; out.scene.nodes.len()];
    let mut stack: Vec<(usize, Matrix, usize)> = out
        .scene
        .roots
        .iter()
        .rev()
        .map(|&r| (r, IDENTITY, 0))
        .collect();
    while let Some((i, parent, depth)) = stack.pop() {
        let Some(node) = out.scene.nodes.get(i) else {
            continue;
        };
        let visited = seen.get_mut(i).map(|s| std::mem::replace(s, true));
        if depth > MAX_DEPTH || visited != Some(false) {
            continue;
        }
        let m = node.matrix.map_or(parent, |l| multiply(&parent, &rows(&l)));
        instances.extend(node.mesh.map(|k| (k, m)));
        if let Some((lo, hi)) = node.mesh.and_then(|k| *local.get(k)?) {
            for c in 0..8 {
                let mut corner = lo;
                for (a, (o, h)) in corner.iter_mut().zip(hi).enumerate() {
                    if c >> a & 1 == 1 {
                        *o = h;
                    }
                }
                let p = apply(&m, corner, 1.0);
                bounds = Some(match bounds {
                    None => (p, p),
                    Some((l, h)) => (each(l, p, f64::min), each(h, p, f64::max)),
                });
            }
        }
        let def = nodes.get(i).and_then(|n| {
            n.get("extensions")?
                .get("KHR_lights_punctual")?
                .get("light")?
                .as_u32()
        });
        match def.map(|d| defs.get(d as usize)) {
            Some(Some(def)) => placed.push(Placed {
                def,
                position: apply(&m, [0.0; 3], 1.0),
                direction: crate::model::light::unit(apply(&m, [0.0, 0.0, -1.0], 0.0))
                    .unwrap_or([0.0, 0.0, -1.0]),
                linear: true,
            }),
            Some(None) => out
                .notes
                .push(format!("node {i} names a light the file lacks")),
            None => {}
        }
        stack.extend(node.children.iter().rev().map(|&c| (c, m, depth + 1)));
    }
    (placed, bounds, instances)
}

/// `f` of each pair of components.
fn each(a: [f64; 3], b: [f64; 3], f: impl Fn(f64, f64) -> f64) -> [f64; 3] {
    let mut out = a;
    for (o, b) in out.iter_mut().zip(b) {
        *o = f(*o, b);
    }
    out
}

fn mesh_bounds(mesh: &crate::SceneMesh) -> Bounds {
    let mut out: Bounds = None;
    for &p in mesh.primitives.iter().flat_map(|p| &p.positions) {
        out = Some(match out {
            None => (p, p),
            Some((l, h)) => (each(l, p, f64::min), each(h, p, f64::max)),
        });
    }
    out
}

/// The light's type, colour (sRGB, as materials are) and intensity.
fn read<'a>(p: &Placed<'a>) -> (Option<&'a str>, [f64; 3], f64) {
    let def = p.def;
    let colour = def
        .get("color")
        .and_then(Json::as_array)
        .filter(|a| a.len() == 3)
        .and_then(|a| {
            let mut c = [0.0; 3];
            for (o, j) in c.iter_mut().zip(a) {
                *o = j.as_f64()?.max(0.0);
            }
            Some(c)
        })
        .unwrap_or([1.0; 3]);
    let intensity = def
        .get("intensity")
        .and_then(Json::as_f64)
        .filter(|i| i.is_finite())
        .unwrap_or(1.0)
        .max(0.0);
    let colour = if p.linear { colour.map(srgb) } else { colour };
    (def.get("type").and_then(Json::as_str), colour, intensity)
}

/// What one light adds at `centre`, before scaling: its brightest channel
/// times its intensity, over the floored squared distance for a point or
/// spot light (a spot's cone is not considered).
fn at_centre(p: &Placed, centre: [f64; 3], floor: f64) -> f64 {
    let (kind, colour, intensity) = read(p);
    let bright = colour.iter().copied().fold(0.0, f64::max) * intensity;
    match kind {
        Some("directional") => bright,
        Some("point" | "spot") => {
            let d = crate::vec3::sub(p.position, centre);
            bright / (crate::vec3::dot(d, d) + floor * floor)
        }
        _ => 0.0,
    }
}

fn normal_scale(placed: &[Placed], centre: [f64; 3], floor: f64) -> f64 {
    let sum: f64 = placed.iter().map(|p| at_centre(p, centre, floor)).sum();
    if sum > 0.0 && sum.is_finite() {
        (1.0 - AMBIENT) / sum
    } else {
        1.0
    }
}

fn light(
    p: &Placed,
    scale: f64,
    floor: f64,
    mode: GltfLights,
    i: usize,
    out: &mut GltfImport,
) -> Option<ModelLight> {
    let (kind, colour, intensity) = read(p);
    let label = p.def.get("name").and_then(Json::as_str).map(str::to_owned);
    let what = label.clone().unwrap_or_else(|| format!("light {i}"));
    if p.def.get("range").is_some() {
        out.notes.push(format!(
            "{what}: its range was dropped (PRC lights reach everywhere)"
        ));
    }
    let k = scale * intensity;
    // 1 / (k I) as quadratic attenuation; the constant term floors the
    // distance as normalisation did.
    let colour = if k > 0.0 { colour } else { [0.0; 3] };
    let attenuation = match (k > 0.0, mode) {
        (false, _) => [1.0, 0.0, 0.0],
        (true, GltfLights::Normalise | GltfLights::Exposure) => [floor * floor / k, 0.0, 1.0 / k],
        (true, GltfLights::Stored | GltfLights::Drop) => [0.0, 0.0, 1.0 / k],
    };
    let mut l = match kind {
        Some("directional") => ModelLight {
            intensity: k,
            ..ModelLight::new(
                ModelLightKind::Directional {
                    direction: p.direction,
                },
                colour,
            )
        },
        Some("point") => ModelLight::new(
            ModelLightKind::Point {
                position: p.position,
                attenuation,
            },
            colour,
        ),
        Some("spot") => ModelLight::new(spot(p, attenuation), colour),
        other => {
            out.notes.push(format!(
                "{what}: light type {} is not drawn",
                other.unwrap_or("(none)")
            ));
            return None;
        }
    };
    l.name = label;
    Some(l)
}

/// A spot's cone: PRC's is hard-edged at `angle` and fades as
/// `cos(theta)^exponent`, so the outer cone is the angle and the exponent is
/// fitted to fall to 1/4 midway between the inner and outer cones; a cone
/// with no soft edge has exponent 0.
pub(super) fn spot(p: &Placed, attenuation: [f64; 3]) -> ModelLightKind {
    let angle = |key, default: f64| {
        p.def
            .get("spot")
            .and_then(|s| s.get(key))
            .and_then(Json::as_f64)
            .filter(|a| a.is_finite())
            .unwrap_or(default)
    };
    let outer = angle("outerConeAngle", std::f64::consts::FRAC_PI_4)
        .clamp(0.0, std::f64::consts::FRAC_PI_2);
    let inner = angle("innerConeAngle", 0.0).clamp(0.0, outer);
    let mid = (inner + outer) / 2.0;
    let exponent = if outer - inner < 1e-6 || mid.cos() >= 1.0 {
        0.0
    } else {
        (0.25f64.ln() / mid.cos().ln()).clamp(0.0, 128.0)
    };
    ModelLightKind::Spot {
        position: p.position,
        direction: p.direction,
        attenuation,
        angle: outer,
        exponent,
    }
}

/// glTF's column-major matrix as a [`Matrix`].
fn rows(m: &[f64; 16]) -> Matrix {
    let mut out = IDENTITY;
    for (c, col) in m.as_chunks::<4>().0.iter().enumerate() {
        for (row, v) in out.iter_mut().zip(col) {
            if let Some(cell) = row.get_mut(c) {
                *cell = *v;
            }
        }
    }
    out
}

/// `m × (v, w)`: a point when `w` is 1, a direction when 0.
fn apply(m: &Matrix, v: [f64; 3], w: f64) -> [f64; 3] {
    let p = transform_point(m, v);
    if w == 0.0 {
        crate::vec3::sub(p, transform_point(m, [0.0; 3]))
    } else {
        p
    }
}
