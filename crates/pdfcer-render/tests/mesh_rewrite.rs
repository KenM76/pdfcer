//! `X086` — a mesh shading decoded and rewritten by
//! `pdfcer_core::mesh_shading` paints, through this renderer, within one
//! 8-bit level of the source on every pixel.
//!
//! The oracle is the source shading itself, rendered by the same renderer:
//! two streams with different record layouts (and, for patches, a
//! different geometry type) that the standard says describe the same
//! picture. This also holds the core decoder and the renderer's own mesh
//! parser together: a disagreement between them shows here as pixels.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::mesh_shading::{
    MeshGeometry, MeshStreamFormat, MeshWriteOptions, Tessellation, decode_mesh,
    tessellate_patches, write_free_form_mesh,
};
use pdfcer_core::object::{Dict, Name, ObjId, Object};
use pdfcer_core::page_tree;
use pdfcer_core::settings::MeshPatchPadding;
use pdfcer_core::writer::IdentityEncoder;
use pdfcer_core::writer::serialize::write_object;
use pdfcer_render::RenderedPage;

const SCALE: f32 = 2.0;

/// A one-page, 120 pt square PDF that paints `dict` + `data` with `sh`.
fn pdf_with_shading(dict: &Dict, data: &[u8]) -> Vec<u8> {
    let mut sh = Vec::new();
    let mut d = dict.clone();
    d.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(data.len()).unwrap()),
    );
    let owner = ObjId {
        num: 5,
        generation: 0,
    };
    write_object(&mut sh, &Object::Dict(d), owner, &[], &IdentityEncoder);
    sh.extend_from_slice(b"\nstream\n");
    sh.extend_from_slice(data);
    sh.extend_from_slice(b"\nendstream");
    let content = b"q /Sh sh Q\n";
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 120] \
           /Resources << /Shading << /Sh 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content,
            b"endstream",
        ]
        .concat(),
        sh,
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn render_bytes(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], SCALE).expect("renders")
}

fn max_channel_delta(a: &RenderedPage, b: &RenderedPage) -> u8 {
    assert_eq!(a.pixmap.width(), b.pixmap.width());
    a.pixmap
        .pixels()
        .iter()
        .zip(b.pixmap.pixels())
        .map(|(p, q)| {
            p.red()
                .abs_diff(q.red())
                .max(p.green().abs_diff(q.green()))
                .max(p.blue().abs_diff(q.blue()))
        })
        .max()
        .unwrap_or(0)
}

/// Pixels that are not white: proof a render painted something.
fn painted(p: &RenderedPage) -> usize {
    p.pixmap
        .pixels()
        .iter()
        .filter(|q| q.red() < 250 || q.green() < 250 || q.blue() < 250)
        .count()
}

fn reals(v: &[f64]) -> Object {
    Object::Array(v.iter().map(|&x| Object::Real(x)).collect())
}

/// A hand-encoded CMYK type 4 source: 16-bit coordinates, 8-bit
/// components, 8-bit flags, one flag-0 triangle and three continuations.
fn cmyk_type4_source() -> (Dict, Vec<u8>) {
    let enc = |v: f64, hi: f64, max: f64| (v / hi * max).round() as u32;
    let mut data = Vec::new();
    let mut vertex = |flag: u8, xy: [f64; 2], cmyk: [f64; 4]| {
        data.push(flag);
        for c in xy {
            data.extend_from_slice(&u16::try_from(enc(c, 120.0, 65535.0)).unwrap().to_be_bytes());
        }
        for c in cmyk {
            data.push(u8::try_from(enc(c, 1.0, 255.0)).unwrap());
        }
    };
    vertex(0, [10.0, 10.0], [1.0, 0.0, 0.0, 0.0]);
    vertex(0, [110.0, 10.0], [0.0, 1.0, 0.0, 0.2]);
    vertex(0, [10.0, 110.0], [0.0, 0.0, 1.0, 0.0]);
    vertex(1, [110.0, 110.0], [0.3, 0.3, 0.0, 0.6]);
    vertex(1, [60.0, 115.0], [0.0, 0.8, 0.8, 0.0]);
    vertex(2, [115.0, 60.0], [0.5, 0.0, 0.5, 0.1]);
    let mut dict = Dict::new();
    dict.insert(Name::from(b"ShadingType"), Object::Integer(4));
    dict.insert(
        Name::from(b"ColorSpace"),
        Object::Name(Name::from(b"DeviceCMYK")),
    );
    dict.insert(Name::from(b"BitsPerCoordinate"), Object::Integer(16));
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::from(b"BitsPerFlag"), Object::Integer(8));
    dict.insert(
        Name::from(b"Decode"),
        reals(&[
            0.0, 120.0, 0.0, 120.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0,
        ]),
    );
    (dict, data)
}

#[test]
fn a_cmyk_type4_mesh_rewritten_renders_within_one_level_of_its_source() {
    let (dict, data) = cmyk_type4_source();
    let format = MeshStreamFormat::from_dict(&dict, 4, MeshPatchPadding::PerRecord).unwrap();
    let mesh = decode_mesh(&data, &format).unwrap();
    let MeshGeometry::Triangles(tris) = &mesh.geometry else {
        panic!("type 4 decodes to triangles");
    };
    assert_eq!(tris.len(), 4);
    let opts = MeshWriteOptions::new(dict.get(b"ColorSpace").unwrap().clone(), 4);
    let written = write_free_form_mesh(tris, &opts).unwrap();
    assert_eq!(written.report.shared_edges, 3, "continuations are kept");

    let source = render_bytes(pdf_with_shading(&dict, &data));
    let rewritten = render_bytes(pdf_with_shading(&written.dict, &written.data));
    assert!(painted(&source) > 10_000, "the source paints");
    let d = max_channel_delta(&source, &rewritten);
    assert!(d <= 1, "rewritten type 4 differs by {d} levels");
}

/// Read fixture object 5 (every `sh`-route mesh fixture's shading).
fn fixture_shading(name: &str) -> (Vec<u8>, Dict, Vec<u8>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/mesh")
        .join(name);
    let bytes = std::fs::read(&path).unwrap();
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let obj = doc
        .get(ObjId {
            num: 5,
            generation: 0,
        })
        .unwrap();
    let Object::Stream(st) = &obj.value else {
        panic!("{name}: object 5 is the shading stream");
    };
    let raw = pdfcer_core::writer::serialize::stream_data(st, doc.bytes()).unwrap();
    let data = pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap();
    (bytes, st.dict.clone(), data)
}

/// The renderer cuts a patch into cells of 4 device pixels; at the same
/// density the two lattices coincide, so the curved silhouette is the same
/// chord polygon in both and every pixel is comparable. (A finer cell
/// follows the curve more closely and moves boundary pixels, which is a
/// better approximation of the patch, not a decoding error.)
fn renderer_density() -> Tessellation {
    Tessellation::with_cell_size(4.0 / f64::from(SCALE))
}

#[test]
fn coons_and_tensor_patches_tessellated_and_rewritten_render_within_one_level() {
    for name in [
        "type6_coons.pdf",
        "type6_flag1.pdf",
        "type6_flag2.pdf",
        "type6_flag3.pdf",
        "type7_tensor.pdf",
        "type7_flag1.pdf",
    ] {
        let (bytes, dict, data) = fixture_shading(name);
        let format = MeshStreamFormat::from_dict(&dict, 3, MeshPatchPadding::PerRecord).unwrap();
        let mesh = decode_mesh(&data, &format).unwrap();
        let MeshGeometry::Patches(patches) = &mesh.geometry else {
            panic!("{name}: patches");
        };
        let tris = tessellate_patches(patches, renderer_density(), 1_000_000).unwrap();
        let opts = MeshWriteOptions::new(dict.get(b"ColorSpace").unwrap().clone(), 3);
        let written = write_free_form_mesh(&tris, &opts).unwrap();

        let source = render_bytes(bytes);
        let rewritten = render_bytes(pdf_with_shading(&written.dict, &written.data));
        assert!(painted(&source) > 10_000, "{name}: the source paints");
        let d = max_channel_delta(&source, &rewritten);
        assert!(d <= 1, "{name}: rewritten mesh differs by {d} levels");
    }
}
