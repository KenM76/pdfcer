//! `KHR_lights_punctual` import over synthetic files.

// Tests fail loudly by design.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;
use crate::{ModelLightKind, assemble, write_prc};

fn bytes() -> Vec<u8> {
    [0f32, 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

fn glb(json: &str) -> Vec<u8> {
    let mut j = json.as_bytes().to_vec();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let bin = bytes();
    let total = 12 + 8 + j.len() + 8 + bin.len();
    let mut out = b"glTF".to_vec();
    for v in [2, total as u32, j.len() as u32, 0x4E4F_534A] {
        out.extend(v.to_le_bytes());
    }
    out.extend(&j);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(0x004E_4942u32.to_le_bytes());
    out.extend(bin);
    out
}

/// A unit quad at z = 0 and the lights `lights` on nodes `nodes`.
fn doc(lights: &str, nodes: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}}, "scene":0,
  "extensionsUsed":["KHR_lights_punctual"],
  "extensions":{{"KHR_lights_punctual":{{"lights":[{lights}]}}}},
  "buffers":[{{"byteLength":48}}],
  "bufferViews":[{{"buffer":0,"byteLength":48}}],
  "accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}}],
  "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"mode":6}}]}}],
  "nodes":[{{"mesh":0}}{nodes}],
  "scenes":[{{"nodes":[0,1,2,3]}}]}}"#
    )
}

fn read(lights: &str, nodes: &str, options: GltfOptions) -> GltfImport {
    read_gltf(&glb(&doc(lights, nodes)), &mut |_| None, options).unwrap()
}

const AS_STORED: GltfOptions = GltfOptions {
    colours: GltfColours::Linear,
    up_axis: GltfUpAxis::AsStored,
    occlusion: GltfOcclusion::Bake,
    lights: GltfLights::Normalise,
};

const THREE: &str = r#"{"type":"point","intensity":100,"name":"lamp","range":5},
    {"type":"directional","intensity":3,"color":[1,1,1]},
    {"type":"spot","intensity":50,"spot":{"outerConeAngle":0.7853981633974483}}"#;

/// The point light above the quad, the sun, and the spot turned to point
/// down (a quarter turn about -X takes -Z to -Y).
const NODES: &str = r#",
  {"translation":[0.5,0.5,1],"extensions":{"KHR_lights_punctual":{"light":0}}},
  {"extensions":{"KHR_lights_punctual":{"light":1}}},
  {"translation":[0,3,0],"rotation":[-0.7071067811865476,0,0,0.7071067811865476],
   "extensions":{"KHR_lights_punctual":{"light":2}}}"#;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn lights_are_placed_and_normalised_at_the_centre() {
    let import = read(THREE, NODES, AS_STORED);
    let l = &import.scene.lights;
    assert_eq!(l.len(), 4, "{l:?}");
    assert_eq!(l[0].name.as_deref(), Some("lamp"));
    let ModelLightKind::Point {
        position,
        attenuation,
    } = l[0].kind
    else {
        panic!("{:?}", l[0])
    };
    assert_eq!(position, [0.5, 0.5, 1.0]);
    let ModelLightKind::Directional { direction } = l[1].kind else {
        panic!("{:?}", l[1])
    };
    assert_eq!(direction, [0.0, 0.0, -1.0]);
    let ModelLightKind::Spot {
        position: at,
        direction: down,
        attenuation: spot_fade,
        angle,
        exponent,
    } = l[2].kind
    else {
        panic!("{:?}", l[2])
    };
    assert!(close(at[1], 3.0));
    assert!(close(down[1], -1.0) && close(down[2], 0.0), "{down:?}");
    assert!(close(angle, std::f64::consts::FRAC_PI_4));
    // Falls to 1/4 midway between the inner (0) and outer cones.
    assert!(
        close((angle / 2.0).cos().powf(exponent), 0.25),
        "{exponent}"
    );
    // What reaches the centre (0.5, 0.5, 0) sums to 80%.
    let fade = |a: [f64; 3], d2: f64| 1.0 / (a[0] + a[2] * d2);
    let sum = fade(attenuation, 1.0) + l[1].intensity + fade(spot_fade, 0.25 + 2.5 * 2.5);
    assert!(close(sum, 0.8), "{sum}");
    assert_eq!(l[3].kind, ModelLightKind::Ambient);
    assert!(import.inferred.iter().any(|s| s.contains("scaled by")));
    assert!(import.inferred.iter().any(|s| s.contains("spot")));
    assert!(
        import.notes.iter().any(|s| s.contains("lamp: its range")),
        "{:?}",
        import.notes
    );
}

#[test]
fn stored_values_keep_physical_units() {
    let options = GltfOptions {
        lights: GltfLights::Stored,
        ..AS_STORED
    };
    let l = read(THREE, NODES, options).scene.lights;
    assert_eq!(l.len(), 3, "no ambient light is added");
    let ModelLightKind::Point { attenuation, .. } = l[0].kind else {
        panic!()
    };
    assert_eq!(attenuation, [0.0, 0.0, 0.01]);
    assert_eq!(l[1].intensity, 3.0);
}

#[test]
fn z_up_turns_the_lights_with_the_model() {
    let l = read(THREE, NODES, GltfOptions::default()).scene.lights;
    let ModelLightKind::Directional { direction } = l[1].kind else {
        panic!()
    };
    // (x, y, z) -> (x, -z, y): glTF's -Z becomes +Y.
    assert!(close(direction[1], 1.0), "{direction:?}");
    let ModelLightKind::Point { position, .. } = l[0].kind else {
        panic!()
    };
    assert!(
        close(position[1], -1.0) && close(position[2], 0.5),
        "{position:?}"
    );
}

#[test]
fn an_unknown_type_and_a_missing_light_are_notes() {
    let lights = r#"{"type":"area"},{"type":"point"},{"type":"point"}"#;
    let nodes = r#",
  {"extensions":{"KHR_lights_punctual":{"light":0}}},
  {"extensions":{"KHR_lights_punctual":{"light":9}}},
  {"extensions":{"KHR_lights_punctual":{"light":1}}}"#;
    let import = read(lights, nodes, AS_STORED);
    assert_eq!(import.scene.lights.len(), 2, "one point and the ambient");
    let notes = import.notes.join("; ");
    assert!(notes.contains("light type area is not drawn"), "{notes}");
    assert!(notes.contains("names a light the file lacks"), "{notes}");
}

#[test]
fn a_hard_cone_has_no_falloff() {
    let lights = r#"{"type":"spot","spot":{"innerConeAngle":0.5,"outerConeAngle":0.5}},
    {"type":"point"},{"type":"point"}"#;
    let l = read(lights, NODES, AS_STORED).scene.lights;
    let ModelLightKind::Spot {
        angle, exponent, ..
    } = l[0].kind
    else {
        panic!()
    };
    assert_eq!((angle, exponent), (0.5, 0.0));
}

#[test]
fn imported_lights_survive_prc() {
    let import = read(THREE, NODES, AS_STORED);
    let m = assemble(&write_prc(&import.scene).unwrap().prc).unwrap();
    assert_eq!(m.lights.len(), 4);
    assert_eq!(m.lights[0].name.as_deref(), Some("lamp"));
    let kinds: Vec<_> = m.lights.iter().map(|l| l.kind.name()).collect();
    assert_eq!(kinds, ["point", "directional", "spot", "ambient"]);
}

#[test]
fn dropped_lights_are_noted() {
    let options = GltfOptions {
        lights: GltfLights::Drop,
        ..AS_STORED
    };
    let import = read(THREE, NODES, options);
    assert!(import.scene.lights.is_empty());
    assert!(import.inferred.is_empty());
    assert!(
        import
            .notes
            .iter()
            .any(|s| s.contains("3 model lights were left out")),
        "{:?}",
        import.notes
    );
}

#[test]
fn a_file_without_lights_has_none() {
    let plain = doc("", "").replace("[0,1,2,3]", "[0]");
    let import = read_gltf(&glb(&plain), &mut |_| None, AS_STORED).unwrap();
    assert!(import.scene.lights.is_empty());
    assert!(import.inferred.is_empty());
}

const LAMP: &str = r#"{"type":"point","intensity":100}"#;
const ABOVE: &str = r#",
  {"translation":[0.5,0.5,1],"extensions":{"KHR_lights_punctual":{"light":0}}}"#;

#[test]
fn exposure_lights_the_metered_surface_to_80_percent() {
    let options = GltfOptions {
        lights: GltfLights::Exposure,
        ..AS_STORED
    };
    let import = read(LAMP, ABOVE, options);
    let ModelLightKind::Point { attenuation, .. } = import.scene.lights[0].kind else {
        panic!("{:?}", import.scene.lights)
    };
    // Both fan triangles' centroids sit 1 + 2/36 squared from the lamp.
    let d2 = 1.0 + 2.0 / 36.0;
    let lit = 1.0 / (attenuation[0] + attenuation[2] * d2);
    assert!(close(lit, 0.8), "{lit}");
    assert_eq!(import.scene.lights[1].kind, ModelLightKind::Ambient);
    assert!(
        import
            .inferred
            .iter()
            .any(|s| s.contains("median lit surface")),
        "{:?}",
        import.inferred
    );
}

#[test]
fn exposure_falls_back_to_the_centre_when_nothing_is_lit() {
    // A spot above the quad turned to face up (a half turn about X).
    let spot = r#"{"type":"spot","intensity":100}"#;
    let up = r#",
  {"translation":[0.5,0.5,1],"rotation":[1,0,0,0],
   "extensions":{"KHR_lights_punctual":{"light":0}}}"#;
    let options = GltfOptions {
        lights: GltfLights::Exposure,
        ..AS_STORED
    };
    let import = read(spot, up, options);
    assert!(
        import
            .notes
            .iter()
            .any(|s| s.contains("scaled at its centre")),
        "{:?}",
        import.notes
    );
    assert!(
        import.inferred.iter().any(|s| s.contains("model's centre")),
        "{:?}",
        import.inferred
    );
}

#[test]
fn exposure_is_the_default() {
    assert_eq!(GltfOptions::default().lights, GltfLights::Exposure);
}
