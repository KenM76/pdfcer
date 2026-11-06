//! Unit tests for `tess`: shared fixtures here, the cases in `tests/`.

use super::compressed::compressed_graphics;
use super::*;
use crate::prc::tess::face::NORMAL_SINGLE;
use crate::prc::tess::wire::{WIRE_IS_CLOSING, WIRE_IS_CONTINUOUS};
use crate::prc::testw::W;

const SQUARE: [f64; 12] = [0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.];

/// A 305 section holding `body` (already-written entities, `n` of them).
fn section(n: u32, body: &W) -> W {
    let mut w = W::default();
    w.uint(305).uint(0).bit(true).uint(n).append(body).uint(0);
    w
}

fn uints(w: &mut W, v: &[u32]) {
    w.uint(v.len() as u32);
    for &x in v {
        w.uint(x);
    }
}

fn square(w: &mut W, entity: u32) {
    coordinates(w, entity, &SQUARE);
}

/// An entity header over `coords` (any length, whole points or not).
fn coordinates(w: &mut W, entity: u32, coords: &[f64]) {
    w.uint(entity).bit(false).uint(coords.len() as u32);
    for &c in coords {
        w.double(c);
    }
}

/// Two normals: slot 0 is +z, slot 3 is -z.
const NORMALS: [f64; 6] = [0., 0., 1., 0., 0., -1.];

struct Mesh<'a> {
    recalc: bool,
    indices: &'a [u32],
    faces: &'a [(u32, u32, &'a [u32])],
    textures: u32,
    uvs: &'a [f64],
}

/// A TESS_3D over [`SQUARE`] at authoring version `v`.
fn tess_3d(w: &mut W, v: u32, m: &Mesh<'_>) {
    tess_3d_over(w, v, m, &SQUARE, &NORMALS);
}

/// [`tess_3d`] over the given positions and (unless recalculated)
/// normals.
fn tess_3d_over(w: &mut W, v: u32, m: &Mesh<'_>, coords: &[f64], normals: &[f64]) {
    coordinates(w, 172, coords);
    w.bit(true);
    if v >= HAS_LOOPS_FROM {
        w.bit(false);
    }
    if v >= RECALCULATE_FROM {
        w.bit(m.recalc);
        if m.recalc {
            w.put(0, 8).double(0.5);
        }
    }
    if m.recalc {
        w.uint(0);
    } else {
        w.uint(normals.len() as u32);
        for &c in normals {
            w.double(c);
        }
    }
    w.uint(0);
    uints(w, m.indices);
    w.uint(m.faces.len() as u32);
    for &(flags, start, data) in m.faces {
        w.uint(174).uint(0).uint(0).uint(0).uint(flags).uint(start);
        uints(w, data);
        w.uint(m.textures).bit(false);
    }
    w.uint(m.uvs.len() as u32);
    for &c in m.uvs {
        w.double(c);
    }
}

fn decode(w: &W, schema: &Schema, version: u32) -> Result<Vec<Tessellation>, PrcError> {
    let bytes = w.bytes();
    Ctx::new(BitReader::new(&bytes), schema, version).file_structure_tessellation()
}

fn mesh(t: &Tessellation) -> &TriangleMesh {
    match t {
        Tessellation::Mesh(m) => m,
        other => panic!("not a mesh: {other:?}"),
    }
}

fn one(indices: &[u32], faces: &[(u32, u32, &[u32])], textures: u32) -> TriangleMesh {
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: false,
            indices,
            faces,
            textures,
            uvs: &[],
        },
    );
    mesh(&decode(&section(1, &b), &Schema::default(), 8137).unwrap()[0]).clone()
}

/// The unit square as two textured triangles (one set; per vertex a
/// normal, a texture slot, a point), its corners at the picture's
/// corners.
pub(crate) fn textured_square_section() -> Vec<u8> {
    let mut body = W::default();
    tess_3d(
        &mut body,
        8137,
        &Mesh {
            recalc: false,
            indices: &[0, 0, 0, 0, 2, 3, 0, 4, 6, 0, 0, 0, 0, 4, 6, 0, 6, 9],
            faces: &[(0x200, 0, &[2])],
            textures: 1,
            uvs: &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        },
    );
    section(1, &body).bytes()
}

/// One triangle with normals, over `coords` and `normals`.
fn one_over(coords: &[f64], normals: &[f64]) -> Result<Vec<Tessellation>, PrcError> {
    let m = Mesh {
        recalc: false,
        indices: &[3, 0, 0, 3, 3, 6],
        faces: &[(0x2, 0, &[1])],
        textures: 0,
        uvs: &[],
    };
    let mut b = W::default();
    tess_3d_over(&mut b, 8137, &m, coords, normals);
    decode(&section(1, &b), &Schema::default(), 8137)
}

/// An empty textured fan declaring `textures` texture sets.
fn empty_fan(textures: u32) -> Result<Vec<Tessellation>, PrcError> {
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: false,
            indices: &[0],
            faces: &[(0x400, 0, &[1, 0])],
            textures,
            uvs: &[],
        },
    );
    decode(&section(1, &b), &Schema::default(), 8137)
}

/// A wire over [`SQUARE`]: an open two-point wire continued by a closing
/// two-point wire, then optionally per-segment RGB colours.
fn wire(w: &mut W, colours: bool) {
    square(w, 175);
    uints(
        w,
        &[2, 0, 3, WIRE_IS_CONTINUOUS | WIRE_IS_CLOSING | 2, 6, 9],
    );
    w.bit(colours);
    if colours {
        // 5 points (4 + the implicit closing one) -> 2 segment colours.
        w.bit(false).bit(true).bit(false).put(0xff0000, 24);
        w.bit(true);
    }
}

/// A square as one triangle plus one 4-vertex fan (two entities, three
/// triangles), the face carrying `attrs`; behaviour bits 0x30.
fn styled_face(attrs: &[u32]) -> TriangleMesh {
    let t = styled_face_with(attrs, 0x30).unwrap();
    assert!(matches!(t[1], Tessellation::Wire(_)));
    mesh(&t[0]).clone()
}

/// [`styled_face`]'s section with the face behaviour written as `bits`.
fn styled_face_with(attrs: &[u32], bits: u32) -> Result<Vec<Tessellation>, PrcError> {
    let mut b = W::default();
    square(&mut b, 172);
    b.bit(true).bit(false).bit(false).uint(0).uint(0);
    uints(&mut b, &[0, 0, 0, 3, 0, 6, 0, 0, 0, 3, 0, 6, 0, 9]);
    b.uint(1).uint(174);
    uints(&mut b, attrs);
    b.uint(0).uint(0).uint(0x2 | 0x4).uint(0);
    uints(&mut b, &[1, 1, 4]);
    b.uint(0).bit(false);
    if !attrs.is_empty() {
        b.uint(bits);
    }
    b.uint(0);
    wire(&mut b, false);
    decode(&section(2, &b), &Schema::default(), 8137)
}

fn styles(m: &TriangleMesh) -> Vec<(u32, u16)> {
    m.triangle_graphics
        .iter()
        .map(|g| (g.style, g.bits))
        .collect()
}

/// A one-triangle TESS_3D_Compressed. `full` takes every optional
/// branch: stored normals, colours, line attributes, texture, behaviours.
/// Otherwise `three_t` picks the 3T edge form, and the triangle rebuilds.
fn compressed(w: &mut W, v: u32, full: bool, three_t: bool) {
    w.uint(173).bit(false).bit(true).double(0.001);
    if v >= ORIGIN_FROM {
        w.put(0, 32).put(0, 32).put(0, 32);
    }
    // point_array: nine 2-bit values.
    w.bit(false).uint(9);
    for _ in 0..9 {
        w.put(2, 8);
    }
    for _ in 0..9 {
        w.bit(false).bit(true);
    }
    // edge_status_array: the 3T form (Huffman) or the T form.
    if full {
        w.bit(true).huffman(2, 2, &[(0, 0b10, 2)], &[0, 0, 0]);
    } else if three_t {
        w.bit(false).uint(3).put(0, 8).put(0, 8).put(0, 8);
    } else {
        w.bit(false).uint(1).put(0, 8);
    }
    // triangle_face_array [0]. `full`: four references, so the
    // flagless reference array is Huffman; else none.
    w.bit(false).uint(1).put(1, 8).bit(false);
    if full {
        w.uint(4).put(0b1111, 4);
        w.huffman(6, 2, &[(1, 0b10, 2), (0, 0b11, 2)], &[1, 0, 0, 0]);
        w.put(0, 4);
    } else {
        w.uint(3).put(0, 3).uint(0);
    }
    if full {
        w.bit(false).put(10, 8).uint(3).put(0b101, 3);
        w.bit(true).huffman(10, 2, &[(5, 0b10, 2)], &[5, 5]);
        w.bit(true); // is_face_planar
        w.bit(true).bit(true).bit(false).uint(5);
        for c in [1, 255, 0, 0, 128] {
            w.put(c, 8);
        }
        w.bit(true).bit(false);
    } else {
        w.bit(true).bit(false).double(0.5).put(0, 8);
        w.bit(false).bit(false);
    }
    w.bit(false).uint(1).put(1, 8).put(0, 8); // line_attribute_array
    if full {
        w.bit(false).uint(1).put(0, 32).uint(32);
        w.uint(1).put(1, 5).bit(true).double(0.5).uint(2);
        w.put(0, 32).put(0, 32).bit(false).bit(true);
        w.bit(true).bit(false).uint(1).put(1, 8);
    } else {
        w.bit(true).bit(false);
    }
}

/// A whole PRC stream: one file structure whose tessellation section
/// holds the unit square as two triangles, and an empty schema.
fn square_prc() -> Vec<u8> {
    let mut body = W::default();
    tess_3d(
        &mut body,
        8137,
        &Mesh {
            recalc: false,
            indices: &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9],
            faces: &[(0x2, 0, &[2])],
            textures: 0,
            uvs: &[],
        },
    );
    prc_file(&section(1, &body))
}

/// [`square_prc`] plus a red wire along its diagonal, (0,0,0)–(1,1,0):
/// a `TESS_3D_Wire` of two indexed points and one segment colour.
fn wired_prc() -> Vec<u8> {
    let mut body = W::default();
    tess_3d(
        &mut body,
        8137,
        &Mesh {
            recalc: false,
            indices: &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9],
            faces: &[(0x2, 0, &[2])],
            textures: 0,
            uvs: &[],
        },
    );
    square(&mut body, 175);
    uints(&mut body, &[2, 0, 6]);
    body.bit(true)
        .bit(false)
        .bit(true)
        .bit(false)
        .put(0xff0000, 24);
    prc_file(&section(2, &body))
}

/// A one-file-structure PRC whose tessellation section is `tess`.
fn prc_file(tess: &W) -> Vec<u8> {
    let schema = W::default().uint(0).bytes();
    crate::prc::testw::prc_container(&schema, &[0], &tess.bytes(), &[0])
}

/// One compressed-tessellation mesh, in a form not rebuilt.
fn compressed_prc() -> Vec<u8> {
    let mut body = W::default();
    compressed(&mut body, 8137, true, true);
    prc_file(&section(1, &body))
}

/// One compressed-tessellation triangle that rebuilds.
fn compressed_prc_rebuilt() -> Vec<u8> {
    let mut body = W::default();
    compressed(&mut body, 8137, false, true);
    prc_file(&section(1, &body))
}

/// Copies of the one-turn mesh in `best_fit.prc`. Each needs two
/// inverted choices, so seven need more than the unique search's
/// deepest stage combines, and no walk retry repairs one.
const BEST_FIT_COPIES: u32 = 7;

/// `best_fit.prc`: one compressed mesh, [`BEST_FIT_COPIES`] separate
/// copies of the four-triangle planar mesh whose apex needs its frame
/// turned, that only [`crate::prc::compressed::rebuild`]'s best-fit search
/// rebuilds.
fn best_fit_prc() -> Vec<u8> {
    let one = [0i64, 0, 0, 4, 0, 0, 2, 4, 0, 0, 0, 0, -4, -4, 2];
    let (mut pts, mut st, mut face_of, mut is_ref, mut refs) =
        (vec![], vec![], vec![], vec![], vec![]);
    let (mut angles, mut planar) = (vec![], vec![]);
    for k in 0..BEST_FIT_COPIES {
        pts.extend_from_slice(&one);
        st.extend_from_slice(&[1, 3, 1, 0]);
        face_of.extend([k; 4]);
        is_ref.extend([false, false, false, false, false, true]);
        refs.push(2 + 5 * k);
        angles.extend([470, 724]);
        planar.push(true);
    }
    st.resize(3 * face_of.len(), 0);
    let mut w = W::default();
    w.uint(173).bit(false).bit(true).double(0.5);
    w.put(0, 32).put(0, 32).put(0, 32);
    vbn_array(&mut w, &pts, 4);
    w.bit(false).uint(st.len() as u32);
    for &x in &st {
        w.put(x as u64, 8);
    }
    w.bit(false);
    indices(&mut w, &face_of, 2, None);
    w.uint(is_ref.len() as u32);
    for &x in &is_ref {
        w.bit(x);
    }
    indices(&mut w, &refs, 4, Some(&[(4, 0b10, 2), (0, 0b11, 2)]));
    w.bit(false).put(10, 8).uint(2 * angles.len() as u32);
    for _ in 0..2 * angles.len() {
        w.bit(false);
    }
    w.bit(false).uint(angles.len() as u32);
    for &a in &angles {
        w.put(a & 0xff, 8).put(a >> 8, 8);
    }
    for &x in &planar {
        w.bit(x);
    }
    w.bit(false).bit(false);
    w.bit(false).uint(1).put(1, 8).put(0, 8); // line_attribute_array
    w.bit(true).bit(false);
    prc_file(&section(1, &w))
}

/// A `CompressedIntegerArray` of `v`, every value `width` bits wide.
fn vbn_array(w: &mut W, v: &[i64], width: u64) {
    w.bit(false).uint(v.len() as u32);
    for _ in v {
        w.put(width, 8);
    }
    for &x in v {
        w.bit(x < 0).put(x.unsigned_abs(), width as u32 - 1);
    }
}

/// A `CompressedIndiceArray` of `v`, every delta `width` bits wide: its
/// width differences uncompressed, or Huffman-coded by `leaves`.
fn indices(w: &mut W, v: &[u32], width: i32, leaves: Option<&[(u32, u32, u32)]>) {
    let diffs: Vec<i32> = (0..v.len())
        .map(|i| if i == 0 { width } else { 0 })
        .collect();
    match leaves {
        Some(l) => {
            w.huffman(6, 2, l, &diffs);
        }
        None => {
            w.uint(v.len() as u32);
            for &d in &diffs {
                w.put(d as u64, 8);
            }
        }
    }
    let mut prev = 0;
    for &x in v {
        w.bit(false).put(u64::from(x - prev), width as u32 - 1);
        prev = x;
    }
}

/// `partial.prc`: one compressed mesh of four triangles on one face;
/// the fourth puts a third triangle on an edge, so no fit rebuilds it,
/// and three decode before that.
fn partial_prc() -> Vec<u8> {
    let pts = [0i64, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2];
    let mut st = vec![3u8, 3];
    st.resize(12, 0);
    let is_ref = [false, false, false, false, true, true];
    let mut w = W::default();
    w.uint(173).bit(false).bit(true).double(0.5);
    w.put(0, 32).put(0, 32).put(0, 32);
    vbn_array(&mut w, &pts, 4);
    w.bit(false).uint(st.len() as u32);
    for &x in &st {
        w.put(u64::from(x), 8);
    }
    w.bit(false);
    indices(&mut w, &[0, 0, 0, 0], 2, None);
    w.uint(is_ref.len() as u32);
    for &x in &is_ref {
        w.bit(x);
    }
    indices(&mut w, &[0, 1], 2, None);
    w.bit(true); // must_recalculate_normals
    for _ in 0..4 {
        w.bit(false); // normal_is_reversed
    }
    w.double(0.5).put(0, 8);
    w.bit(false).bit(false);
    w.bit(false).uint(1).put(1, 8).put(0, 8); // line_attribute_array
    w.bit(true).bit(false);
    prc_file(&section(1, &w))
}

/// `bytes` refused under a strict fit with `Malformed`, and assembled
/// under the default (lenient) fit with one damaged unit for `why`.
fn salvaged(bytes: &[u8], why: &str) -> crate::AssembledModel {
    let strict = crate::AssembleOptions {
        mesh_fit: crate::MeshFit::Unique,
        ..Default::default()
    };
    let err = crate::assemble_with_options(bytes, &strict).unwrap_err();
    assert!(
        matches!(err, crate::AssembleError::Prc(PrcError::Malformed(_))),
        "{err:?}"
    );
    let m = crate::assemble(bytes).unwrap();
    assert_eq!(m.damaged, 1, "{:?}", m.damaged_why);
    assert_eq!(m.damaged_why.len(), 1);
    assert!(m.damaged_why[0].0.contains(why), "{:?}", m.damaged_why);
    assert!(m.partial_why.is_empty() && m.skipped_why.is_empty());
    m
}

/// Two one-triangle faces over [`SQUARE`]; the second's behaviour is
/// written as `bits`.
fn two_faces(bits: u32) -> Vec<u8> {
    let mut b = W::default();
    square(&mut b, 172);
    b.bit(true).bit(false).bit(false).uint(0).uint(0);
    uints(&mut b, &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9]);
    b.uint(2);
    for (start, bits) in [(0, 0x30), (6, bits)] {
        b.uint(174);
        uints(&mut b, &[5]);
        b.uint(0).uint(0).uint(0x2).uint(start);
        uints(&mut b, &[1]);
        b.uint(0).bit(false).uint(bits);
    }
    b.uint(0);
    prc_file(&section(1, &b))
}

mod compressed_and_fixtures;
mod uncompressed;
