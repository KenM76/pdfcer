//! Fuzz target: mesh-shading stream decoding and rewriting
//! (`pdfcer_core::mesh_shading`).
//!
//! The first bytes choose the stream layout (type, field widths, colour
//! component count, padding); the rest is the stream. Decoding, patch
//! tessellation, rewriting as type 4 and re-expression must never panic,
//! and a written mesh must decode back to the same triangle count.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::mesh_shading::{
    MeshGeometry, MeshStreamFormat, MeshWriteOptions, Reexpress, Tessellation, decode_mesh,
    reexpress_mesh, tessellate_patches, write_free_form_mesh,
};
use pdfcer_core::object::{Dict, Name, Object};
use pdfcer_core::settings::MeshPatchPadding;

const SPACES: [&[u8]; 3] = [b"DeviceGray", b"DeviceRGB", b"DeviceCMYK"];
const COMPONENTS: [usize; 3] = [1, 3, 4];
const CAP: usize = 20_000;

fn dict_for(head: &[u8]) -> (Dict, usize, MeshPatchPadding) {
    let pick = |i: usize, opts: &[i64]| opts[usize::from(head[i]) % opts.len()];
    let space = usize::from(head[0]) % 3;
    let n = COMPONENTS[space];
    let mut d = Dict::new();
    d.insert(
        Name::from(b"ShadingType"),
        Object::Integer(pick(1, &[4, 5, 6, 7])),
    );
    d.insert(
        Name::from(b"ColorSpace"),
        Object::Name(Name::from(SPACES[space])),
    );
    d.insert(
        Name::from(b"BitsPerCoordinate"),
        Object::Integer(pick(2, &[1, 2, 4, 8, 12, 16, 24, 32])),
    );
    d.insert(
        Name::from(b"BitsPerComponent"),
        Object::Integer(pick(3, &[1, 2, 4, 8, 12, 16])),
    );
    d.insert(
        Name::from(b"BitsPerFlag"),
        Object::Integer(pick(4, &[2, 4, 8])),
    );
    d.insert(
        Name::from(b"VerticesPerRow"),
        Object::Integer(pick(5, &[2, 3, 7])),
    );
    let mut decode = vec![
        Object::Real(0.0),
        Object::Real(100.0),
        Object::Real(-5.0),
        Object::Real(5.0e6),
    ];
    for _ in 0..n {
        decode.extend([Object::Real(0.0), Object::Real(1.0)]);
    }
    d.insert(Name::from(b"Decode"), Object::Array(decode));
    let padding = if head[6] % 2 == 0 {
        MeshPatchPadding::PerRecord
    } else {
        MeshPatchPadding::None
    };
    (d, n, padding)
}

fuzz_target!(|data: &[u8]| {
    let Some((head, stream)) = data.split_first_chunk::<7>() else {
        return;
    };
    let (dict, n, padding) = dict_for(head);
    let Ok(format) = MeshStreamFormat::from_dict(&dict, n, padding) else {
        return;
    };
    let Ok(mesh) = decode_mesh(stream, &format) else {
        return;
    };
    let tris = match &mesh.geometry {
        MeshGeometry::Triangles(t) => t.clone(),
        MeshGeometry::Patches(p) => {
            match tessellate_patches(p, Tessellation::with_cell_size(10.0), CAP) {
                Ok(t) => t,
                Err(_) => return,
            }
        }
    };
    let opts = MeshWriteOptions::new(dict.get(b"ColorSpace").cloned().unwrap_or(Object::Null), n)
        .with_triangle_cap(CAP);
    if let Ok(w) = write_free_form_mesh(&tris, &opts) {
        let plain = pdfcer_core::filters::decode_stream(&w.dict, &w.data).expect("own Flate");
        let f =
            MeshStreamFormat::from_dict(&w.dict, n, MeshPatchPadding::PerRecord).expect("own dict");
        let back = decode_mesh(&plain, &f).expect("own stream");
        match back.geometry {
            MeshGeometry::Triangles(t) => assert_eq!(t.len(), tris.len()),
            MeshGeometry::Patches(_) => panic!("type 4 wrote patches"),
        }
    }
    let params = Reexpress::default()
        .with_triangle_cap(CAP)
        .with_max_depth(3);
    let _ = reexpress_mesh(
        &mesh,
        3,
        |c| Some(vec![c[0].sqrt(), 1.0 - c[0], c[0] * c[0]]),
        &params,
    );
});
