//! What the WebGL measurement pages share: test scenes and `data.js`
//! encoding.

use pdfcer_3d::{AssembledModel, GltfOptions, ModelLight, ModelLightKind, TriangleMesh};

fn square(corners: [[f64; 3]; 4]) -> TriangleMesh {
    let mut m = TriangleMesh::default();
    m.positions = corners.to_vec();
    m.triangles = vec![[0, 1, 2], [0, 2, 3]];
    m.faces = std::iter::once(0..2).collect();
    m.normals_recalculated = true;
    m
}

pub fn room() -> AssembledModel {
    let wall = |x: f64| square([[x, 0., 0.], [x, 1., 0.], [x, 1., 1.], [x, 0., 1.]]);
    let mut m = AssembledModel::default();
    m.meshes = vec![
        square([[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]),
        wall(0.0),
        wall(1.0),
    ];
    m.colours = vec![
        Some([230, 230, 230, 255]),
        Some([220, 30, 30, 255]),
        Some([30, 220, 30, 255]),
    ];
    m.finishes = vec![None; 3];
    m.lights = vec![ModelLight::new(
        ModelLightKind::Point {
            position: [0.5, 0.5, 0.9],
            attenuation: [1.0, 0.0, 0.0],
        },
        [0.8; 3],
    )];
    m
}

pub fn load(arg: &str) -> AssembledModel {
    if arg == "room" {
        return room();
    }
    if arg == "roomglow" {
        let mut m = room();
        let mut glow = pdfcer_3d::MaterialFinish::new([0.0; 3], [0.0; 3], 0.0);
        glow.emissive = [0.9, 0.2, 0.1];
        m.finishes[1] = Some(glow);
        return m;
    }
    let data = std::fs::read(arg).expect("readable model");
    let import = pdfcer_3d::read_gltf(&data, &mut |_| None, GltfOptions::default()).expect("glTF");
    let prc = pdfcer_3d::write_prc(&import.scene).expect("PRC").prc;
    pdfcer_3d::assemble(&prc).expect("assembled")
}

pub fn b64(floats: &[f32]) -> String {
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
    b64_bytes(&bytes)
}

pub fn b64_bytes(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                s.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

pub fn js_str(s: &str) -> String {
    format!("{s:?}")
}

/// `t` as a `D.<name>` texture line of `data.js`.
pub fn texture_line(name: &str, t: &pdfcer_3d::FloatTexture) -> String {
    format!(
        "D.{name} = {{w:{}, h:{}, b:\"{}\"}};\n",
        t.width,
        t.height,
        b64(&t.texels)
    )
}

/// About `count` queries at triangle centres spread over every mesh.
pub fn surface_queries(
    model: &pdfcer_3d::AssembledModel,
    count: usize,
) -> Vec<pdfcer_3d::IlluminanceQuery> {
    let total: usize = model.meshes.iter().map(|m| m.triangles.len()).sum();
    let step = (total / count).max(1);
    let mut out = Vec::new();
    let mut k = 0usize;
    for (i, m) in model.meshes.iter().enumerate() {
        for t in &m.triangles {
            k += 1;
            if !k.is_multiple_of(step) {
                continue;
            }
            let [a, b, c] = t.map(|v| m.positions[v as usize]);
            let centre = std::array::from_fn(|j| (a[j] + b[j] + c[j]) / 3.0);
            let (u, v) = (
                std::array::from_fn::<f64, 3, _>(|j| b[j] - a[j]),
                std::array::from_fn::<f64, 3, _>(|j| c[j] - a[j]),
            );
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 {
                out.push(pdfcer_3d::IlluminanceQuery::new(
                    i,
                    centre,
                    n.map(|x| x / l),
                ));
            }
        }
    }
    out
}
