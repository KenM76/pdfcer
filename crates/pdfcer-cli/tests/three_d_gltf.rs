//! `pdfcer 3d-convert` and `3d-embed --model x.glb` on synthetic glTF.

#![cfg(feature = "3d")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A per-test folder under Cargo's target tmpdir, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("gltf_{tag}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// A quad: four `f32` positions then six `u16` indices, 60 bytes.
fn quad_bin() -> Vec<u8> {
    let p: [f32; 12] = [0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.];
    let mut b: Vec<u8> = p.iter().flat_map(|f| f.to_le_bytes()).collect();
    b.extend([0u16, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_le_bytes()));
    b
}

/// A frame holding a see-through pane and a masked grille; `buffer` is
/// extra keys for buffer 0 (an external `uri`, or none for a GLB).
fn gltf_json(buffer: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,
  "buffers":[{{"byteLength":60{buffer}}}],
  "bufferViews":[{{"buffer":0,"byteLength":48}},{{"buffer":0,"byteOffset":48,"byteLength":12}}],
  "accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}},
    {{"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"}}],
  "materials":[{{"name":"glass","alphaMode":"BLEND","pbrMetallicRoughness":{{"baseColorFactor":[0.2,0.4,0.9,0.4]}}}},
    {{"name":"grille","alphaMode":"MASK","pbrMetallicRoughness":{{"baseColorFactor":[1,1,1,0.9]}}}}],
  "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}},
    {{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":1}}]}}],
  "nodes":[{{"name":"frame","children":[1,2]}},{{"name":"pane","mesh":0}},{{"name":"grille","mesh":1}}],
  "scenes":[{{"nodes":[0]}}]}}"#
    )
}

fn glb() -> Vec<u8> {
    let mut json = gltf_json("").into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let bin = quad_bin();
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = b"glTF".to_vec();
    for v in [2, total, json.len(), 0x4E4F_534A] {
        out.extend((v as u32).to_le_bytes());
    }
    out.extend(&json);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(0x004E_4942u32.to_le_bytes());
    out.extend(bin);
    out
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_glb_converts_to_prc_and_discloses_the_mask() {
    let s = Scratch::new("convert");
    std::fs::write(s.path("m.glb"), glb()).unwrap();
    let text = stdout(&run(&[
        "3d-convert",
        &s.path("m.glb"),
        "-o",
        &s.path("m.prc"),
    ]));
    assert!(text.contains("format=prc triangles=4 "), "{text}");
    assert!(
        text.contains("note: glTF converted to PRC: linear"),
        "{text}"
    );
    assert!(
        text.contains("inferred: grille: alpha mask drawn wholly opaque"),
        "{text}"
    );
    assert!(std::fs::read(s.path("m.prc")).unwrap().starts_with(b"PRC"));
}

#[test]
fn an_external_buffer_is_read_beside_the_model_and_never_outside_it() {
    let s = Scratch::new("external");
    std::fs::write(s.path("quad.bin"), quad_bin()).unwrap();
    std::fs::write(s.path("m.gltf"), gltf_json(r#","uri":"quad.bin""#)).unwrap();
    let text = stdout(&run(&[
        "3d-convert",
        &s.path("m.gltf"),
        "-o",
        &s.path("m.prc"),
    ]));
    assert!(text.contains("triangles=4"), "{text}");

    std::fs::write(s.path("up.gltf"), gltf_json(r#","uri":"../quad.bin""#)).unwrap();
    let out = run(&["3d-convert", &s.path("up.gltf"), "-o", &s.path("up.prc")]);
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("buffer 0"));
    assert!(!Path::new(&s.path("up.prc")).exists());
}

#[test]
fn a_glb_embeds_as_prc_with_its_names_in_the_model_tree() {
    let s = Scratch::new("embed");
    std::fs::write(s.path("m.glb"), glb()).unwrap();
    let pdf = format!(
        "{}/../../fixtures/synthetic/minimal.pdf",
        env!("CARGO_MANIFEST_DIR")
    );
    let embed = stdout(&run(&[
        "3d-embed",
        &pdf,
        "--model",
        &s.path("m.glb"),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "--apply",
        "-o",
        &s.path("out.pdf"),
    ]));
    assert!(embed.contains(" format=PRC "), "{embed}");
    assert!(
        embed.contains("inferred: format glTF from the file's signature or extension"),
        "{embed}"
    );
    assert!(embed.contains("note: glTF converted to PRC"), "{embed}");
    let tree = stdout(&run(&["3d-tree", &s.path("out.pdf"), "--index", "0"]));
    for name in ["frame", "pane", "grille"] {
        assert!(tree.contains(name), "{name} missing from\n{tree}");
    }
}

#[test]
fn a_non_gltf_file_stated_as_gltf_is_refused() {
    let s = Scratch::new("refused");
    std::fs::write(s.path("m.gltf"), b"PRC not json").unwrap();
    let out = run(&["3d-convert", &s.path("m.gltf"), "-o", &s.path("m.prc")]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!Path::new(&s.path("m.prc")).exists());
}
