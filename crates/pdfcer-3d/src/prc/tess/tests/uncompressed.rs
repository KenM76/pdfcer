use super::*;

#[test]
fn triangle_and_fan_with_normals() {
    // One triangle (n,p x3), then a fan of 4 vertices (n,p each).
    let m = one(
        &[3, 0, 0, 3, 3, 6, 0, 0, 3, 3, 0, 6, 3, 9],
        &[(0x2 | 0x4, 0, &[1, 1, 4])],
        0,
    );
    assert_eq!(m.positions.len(), 4);
    assert_eq!(m.positions[2], [1., 1., 0.]);
    assert_eq!(m.triangles, [[0, 1, 2], [0, 1, 2], [0, 2, 3]]);
    assert_eq!(m.faces, vec![std::ops::Range { start: 0, end: 3 }]);
    assert!(!m.normals_recalculated);
    assert_eq!(m.normals, [[0., 0., 1.], [0., 0., -1.]]);
    // Each corner keeps the normal stored beside its point.
    assert_eq!(m.triangle_normals, [[1, 0, 1], [0, 1, 0], [0, 0, 1]]);
}

#[test]
fn strip_normals_follow_the_alternating_winding() {
    let m = one(&[3, 0, 0, 3, 3, 6, 0, 9], &[(0x8, 0, &[1, 4])], 0);
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3]]);
    assert_eq!(m.triangle_normals, [[1, 0, 1], [1, 0, 0]]);
}

#[test]
fn a_normal_index_past_the_array_drops_every_normal_but_no_triangle() {
    let m = one(&[0, 0, 6, 3, 0, 6], &[(0x2, 0, &[1])], 0);
    assert_eq!(m.triangles, [[0, 1, 2]]);
    assert!(m.normals.is_empty());
    assert!(m.triangle_normals.is_empty());
}

#[test]
fn recalculated_normals_drop_the_normal_slots_and_strips_alternate() {
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: true,
            indices: &[0, 3, 6, 9],
            faces: &[(0x8, 0, &[1, 4])],
            textures: 0,
            uvs: &[],
        },
    );
    let t = decode(&section(1, &b), &Schema::default(), 8137).unwrap();
    let m = mesh(&t[0]);
    assert!(m.normals_recalculated);
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3]]);
    assert!(m.normals.is_empty() && m.triangle_normals.is_empty());
}

#[test]
fn version_gates_the_loop_and_recalculation_flags() {
    for v in [7000, 7040, 8137] {
        let mut b = W::default();
        tess_3d(
            &mut b,
            v,
            &Mesh {
                recalc: false,
                indices: &[0, 0, 0, 3, 0, 6],
                faces: &[(0x2, 0, &[1])],
                textures: 0,
                uvs: &[],
            },
        );
        // A trailing wire: a misread flag desyncs it.
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), v).unwrap();
        assert_eq!(mesh(&t[0]).triangles, [[0, 1, 2]], "version {v}");
        assert!(matches!(t[1], Tessellation::Wire(_)), "version {v}");
    }
}

#[test]
fn one_normal_blocks_and_textures() {
    // 0x20: n,p,p,p. 0x40 with NORMAL_Single: n then p per vertex.
    // 0x2000 with one texture set: n,t,p,t,p,t,p.
    let idx = [
        0, 0, 3, 6, // 0x20
        3, 0, 6, 9, // 0x40 single, 3 vertices
        0, 0, 9, 2, 0, 4, 3, // 0x2000
    ];
    let m = one(
        &idx,
        &[(0x20 | 0x40 | 0x2000, 0, &[1, 1, 3 | NORMAL_SINGLE, 1])],
        1,
    );
    assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3], [3, 0, 1]]);
    // A one-normal entity gives every corner its single normal.
    assert_eq!(m.triangle_normals, [[0, 0, 0], [1, 1, 1], [0, 0, 0]]);
}

#[test]
fn one_normal_fan_without_single_keeps_a_normal_per_vertex() {
    let m = one(&[0, 0, 0, 3, 0, 6, 0, 9], &[(0x40, 0, &[1, 4])], 0);
    assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
}

#[test]
fn textured_fan_and_strip_carry_texture_slots_per_vertex() {
    // 0x400 then 0x800, two texture sets: n,t,t,p per vertex.
    let mut idx = Vec::new();
    for p in [0, 3, 6, 0, 3, 6, 9] {
        idx.extend([0, 0, 2, p]);
    }
    let m = one(&idx, &[(0x400 | 0x800, 0, &[1, 3, 1, 4])], 2);
    assert_eq!(m.triangles, [[0, 1, 2], [0, 1, 2], [2, 1, 3]]);
}

#[test]
fn a_textured_mesh_keeps_each_corners_coordinates() {
    let bytes = textured_square_section();
    let t = Ctx::new(BitReader::new(&bytes), &Schema::default(), 8137)
        .file_structure_tessellation()
        .unwrap();
    let m = mesh(&t[0]);
    assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
    assert_eq!(m.uvs, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    assert_eq!(m.triangle_uvs, [vec![Some([0, 1, 2]), Some([0, 2, 3])]]);
    // An odd slot names no pair: that triangle has no coordinates.
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: false,
            indices: &[0, 1, 0, 0, 2, 3, 0, 4, 6],
            faces: &[(0x200, 0, &[1])],
            textures: 1,
            uvs: &[0.0; 6],
        },
    );
    let t = decode(&section(1, &b), &Schema::default(), 8137).unwrap();
    let m = mesh(&t[0]);
    assert_eq!(m.triangles, [[0, 1, 2]]);
    assert_eq!(m.triangle_uvs, [vec![None]]);
}

#[test]
fn two_faces_partition_the_triangles() {
    let m = one(
        &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9],
        &[(0x2, 0, &[1]), (0x2, 6, &[1])],
        0,
    );
    assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
    assert_eq!(m.faces, [0..1, 1..2]);
}

#[test]
fn partial_position_and_wire_points_are_malformed() {
    for extra in [1, 2] {
        let coords: Vec<f64> = SQUARE
            .iter()
            .copied()
            .chain([0.0; 2])
            .take(12 + extra)
            .collect();
        let got = one_over(&coords, &NORMALS);
        assert!(matches!(got, Err(PrcError::Malformed(_))), "{got:?}");
        let mut b = W::default();
        coordinates(&mut b, 175, &coords);
        b.uint(0).bit(false);
        let got = decode(&section(1, &b), &Schema::default(), 8137);
        assert!(matches!(got, Err(PrcError::Malformed(_))), "{got:?}");
    }
    assert!(one_over(&SQUARE, &NORMALS).is_ok());
}

#[test]
fn a_partial_normal_triple_drops_every_stored_normal() {
    let whole = one_over(&SQUARE, &NORMALS).unwrap();
    assert_eq!(mesh(&whole[0]).normals.len(), 2);
    let normals: Vec<f64> = NORMALS.iter().copied().chain([0.0]).collect();
    let t = one_over(&SQUARE, &normals).unwrap();
    let m = mesh(&t[0]);
    assert_eq!(m.triangles, [[0, 1, 2]]);
    assert!(m.normals.is_empty());
    assert!(m.triangle_normals.is_empty());
}

#[test]
fn faces_reusing_one_index_range_are_charged_per_face() {
    // F faces, each a run of k triangles over the same k-triangle range.
    const F: u64 = 5;
    const K: u64 = 40;
    let indices: Vec<u32> = [3, 0, 0, 3, 3, 6].repeat(K as usize);
    let sizes = [K as u32];
    let faces = vec![(0x2, 0, &sizes[..]); F as usize];
    let m = Mesh {
        recalc: false,
        indices: &indices,
        faces: &faces,
        textures: 0,
        uvs: &[],
    };
    let mut b = W::default();
    tess_3d(&mut b, 8137, &m);
    let bytes = section(1, &b).bytes();
    let schema = Schema::default();
    let run = |already: u64| {
        let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
        ctx.elements = already;
        let got = ctx.file_structure_tessellation();
        (got, ctx.elements)
    };
    let (got, spent) = run(0);
    assert_eq!(mesh(&got.unwrap()[0]).triangles.len() as u64, F * K);
    // Each triangle holds its corners and its normal-slot corners.
    let cost = F * K * 4;
    assert_eq!(spent, cost);
    // Room for exactly that fits; one element less is refused.
    let room = crate::MAX_DECODED_ELEMENTS - cost;
    assert!(run(room).0.is_ok());
    assert_eq!(
        run(room + 1).0.unwrap_err(),
        PrcError::TooManyElements {
            limit: crate::MAX_DECODED_ELEMENTS
        }
    );
    // A lenient read contains damage, never a ceiling.
    let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
    ctx.elements = room + 1;
    ctx.lenient = true;
    let n = ctx.tessellation_count().unwrap();
    let mut model = crate::AssembledModel::default();
    let mut flagged = crate::prc::mesh::Flagged::default();
    let got = crate::prc::mesh::read_entries(&mut ctx, (0, n), &mut model, &mut flagged);
    assert_eq!(
        got.unwrap_err(),
        PrcError::TooManyElements {
            limit: crate::MAX_DECODED_ELEMENTS
        }
    );
    assert_eq!(model.damaged, 0);
}

#[test]
fn too_many_texture_sets_are_refused_before_anything_is_allocated() {
    match empty_fan(u32::MAX) {
        Err(PrcError::Malformed(why)) => {
            assert!(why.contains("4294967295 texture-coordinate sets"), "{why}");
        }
        other => panic!("not refused: {other:?}"),
    }
    let at_cap = crate::prc::tess::face::MAX_TEXTURE_SETS as u32;
    assert!(empty_fan(at_cap).is_ok());
    assert!(matches!(empty_fan(at_cap + 1), Err(PrcError::Malformed(_))));
}

#[test]
fn reused_textured_faces_are_charged_for_their_corner_arrays() {
    // F faces, each one triangle with 8 texture sets, over one range.
    const F: u64 = 6;
    let sets = crate::prc::tess::face::MAX_TEXTURE_SETS;
    let vertex = |p: u32| {
        let mut v = vec![0];
        v.extend(std::iter::repeat_n(0, sets));
        v.push(p);
        v
    };
    let indices: Vec<u32> = [0, 3, 6].into_iter().flat_map(vertex).collect();
    let faces = vec![(0x200, 0, &[1][..]); F as usize];
    let m = Mesh {
        recalc: false,
        indices: &indices,
        faces: &faces,
        textures: sets as u32,
        uvs: &[0.0, 0.0],
    };
    let mut b = W::default();
    tess_3d(&mut b, 8137, &m);
    let bytes = section(1, &b).bytes();
    let schema = Schema::default();
    let run = |already: u64| {
        let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
        ctx.elements = already;
        let got = ctx.file_structure_tessellation();
        (got, ctx.elements)
    };
    let (got, spent) = run(0);
    let t = got.unwrap();
    assert_eq!(mesh(&t[0]).triangles.len() as u64, F);
    assert_eq!(mesh(&t[0]).triangle_uvs.len(), sets);
    // Corners 2, normal slots 2, 2 per texture set: 20 per triangle.
    let cost = F * (2 + 2 + 2 * sets as u64);
    assert_eq!(spent, cost);
    let room = crate::MAX_DECODED_ELEMENTS - cost;
    assert!(run(room).0.is_ok());
    assert_eq!(
        run(room + 1).0.unwrap_err(),
        PrcError::TooManyElements {
            limit: crate::MAX_DECODED_ELEMENTS
        }
    );
    // A real textured model still opens from an empty allowance.
    let square = textured_square_section();
    assert!(
        Ctx::new(BitReader::new(&square), &Schema::default(), 8137)
            .file_structure_tessellation()
            .is_ok()
    );
}

#[test]
fn wires_close_and_continue_and_colours_are_counted() {
    for colours in [false, true] {
        let mut b = W::default();
        wire(&mut b, colours);
        // A second entity: a wrong colour count would desync it.
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        for (i, e) in t.iter().enumerate() {
            let Tessellation::Wire(w) = e else {
                panic!("not a wire")
            };
            // Two red segments cover four points; the odd fifth takes
            // the last segment's colour.
            let red = vec![[255, 0, 0, 255]; 5];
            let want: Vec<Vec<[u8; 4]>> = if colours && i == 0 {
                vec![red]
            } else {
                Vec::new()
            };
            assert_eq!(w.colours, want);
            assert_eq!(
                w.polylines,
                [vec![
                    [0., 0., 0.],
                    [1., 0., 0.],
                    [1., 1., 0.],
                    [0., 1., 0.],
                    [1., 1., 0.]
                ]]
            );
        }
    }
}

#[test]
fn a_closing_wire_colours_its_implicit_point() {
    // Per-point RGB: 4 indexed points + 1 implicit = 5 colours.
    let mut b = W::default();
    square(&mut b, 175);
    uints(
        &mut b,
        &[2, 0, 3, WIRE_IS_CONTINUOUS | WIRE_IS_CLOSING | 2, 6, 9],
    );
    b.bit(true).bit(false).bit(false).bit(false).put(1, 24);
    for _ in 1..5 {
        b.bit(true);
    }
    wire(&mut b, false);
    let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
    let (Tessellation::Wire(a), Tessellation::Wire(b)) = (&t[0], &t[1]) else {
        panic!("not wires")
    };
    assert_eq!(a.polylines, b.polylines);
    assert_eq!(a.colours, [vec![[0, 0, 1, 255]; 5]]);
}

#[test]
fn a_wire_index_must_name_a_point() {
    let mut b = W::default();
    square(&mut b, 175);
    uints(&mut b, &[2, 0, 4]);
    b.bit(false);
    assert!(matches!(
        decode(&section(1, &b), &Schema::default(), 8137),
        Err(PrcError::Malformed(_))
    ));
}

#[test]
fn a_wire_without_indices_is_the_coordinate_polyline() {
    let mut b = W::default();
    square(&mut b, 175);
    b.uint(0).bit(true);
    // 4 implicit points -> 4 RGB colours, all "same" after the first.
    b.bit(false).bit(false).bit(false).put(1, 24);
    for _ in 1..4 {
        b.bit(true);
    }
    wire(&mut b, false);
    let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
    let Tessellation::Wire(w) = &t[0] else {
        panic!("not a wire")
    };
    assert_eq!(w.polylines.len(), 1);
    assert_eq!(w.polylines[0].len(), 4);
    assert!(matches!(t[1], Tessellation::Wire(_)));
}

#[test]
fn a_tessellation_ending_in_reused_vertex_colours_reads() {
    // A four-point RGBA polyline, the last entity: one colour stored in
    // full, three reusing it, then only the section's user data.
    let mut b = W::default();
    square(&mut b, 175);
    b.uint(0).bit(true);
    b.bit(true).bit(false).bit(false).put(0x1020_3040, 32);
    for _ in 1..4 {
        b.bit(true);
    }
    let t = decode(&section(1, &b), &Schema::default(), 8137).unwrap();
    let Tessellation::Wire(w) = &t[0] else {
        panic!("not a wire")
    };
    assert_eq!(w.polylines[0].len(), 4);

    // A first colour cut short still cannot fit.
    let mut b = W::default();
    square(&mut b, 175);
    b.uint(0).bit(true);
    b.bit(true).bit(false).bit(false).put(0x10_2030, 24);
    assert!(matches!(
        decode(&section(1, &b), &Schema::default(), 8137),
        Err(PrcError::Truncated(_))
    ));
}

#[test]
fn programs_on_one_stream_share_one_step_allowance() {
    // For 1_500_000 { {} } after each wire: about 3M steps, under the
    // 2^22 ceiling once, over it twice.
    let mut sw = W::default();
    sw.uint(1).uint(TESS_3D_WIRE).uint(5);
    for t in [15, 26, 1_500_000, 19, 21] {
        sw.uint(t);
    }
    let sb = sw.bytes();
    let schema = Schema::read(&mut BitReader::new(&sb)).unwrap();
    let wires = |n| {
        let mut b = W::default();
        for _ in 0..n {
            wire(&mut b, false);
        }
        decode(&section(n, &b), &schema, 8137)
    };
    assert_eq!(wires(1).unwrap().len(), 1);
    match wires(2) {
        Err(PrcError::Malformed(m)) => assert!(m.contains("step ceiling"), "{m}"),
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn face_vertex_colours_count_points_not_data() {
    // A triangle plus a fan of 4 vertices = 7 points -> 7 colours.
    let mut b = W::default();
    square(&mut b, 172);
    b.bit(true).bit(false).bit(false).uint(0).uint(0);
    uints(&mut b, &[0, 0, 0, 3, 0, 6, 0, 0, 0, 3, 0, 6, 0, 9]);
    b.uint(1)
        .uint(174)
        .uint(0)
        .uint(0)
        .uint(0)
        .uint(0x2 | 0x4)
        .uint(0);
    uints(&mut b, &[1, 1, 4]);
    b.uint(0).bit(true);
    b.bit(false).bit(false).put(0x10_2030, 24);
    for _ in 1..7 {
        b.bit(true);
    }
    b.uint(0);
    wire(&mut b, false);
    let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
    assert_eq!(mesh(&t[0]).triangles.len(), 3);
    assert!(matches!(t[1], Tessellation::Wire(_)));
}

#[test]
fn face_behaviour_follows_line_attributes() {
    let mut b = W::default();
    square(&mut b, 172);
    b.bit(true).bit(false).bit(false).uint(0).uint(0);
    uints(&mut b, &[0, 0, 0, 3, 0, 6]);
    b.uint(1).uint(174);
    uints(&mut b, &[5]);
    b.uint(0).uint(0).uint(0x2).uint(0);
    uints(&mut b, &[1]);
    b.uint(0).bit(false).uint(1).uint(0);
    wire(&mut b, false);
    let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
    assert!(matches!(t[1], Tessellation::Wire(_)));
}

#[test]
fn a_face_behaviour_wider_than_16_bits_is_malformed() {
    // SonHeritColor (0x0008) with bit 16 set: cut to u16 it would read
    // as a plain inherit flag.
    assert!(matches!(
        styled_face_with(&[5], 0x1_0000 | 0x0008),
        Err(PrcError::Malformed(_))
    ));
    assert!(styled_face_with(&[5], 0xffff).is_ok());
}

#[test]
fn face_line_attributes_style_its_triangles() {
    assert!(styled_face(&[]).triangle_graphics.is_empty());
    let whole = styled_face(&[5]);
    assert_eq!(whole.triangles.len(), 3);
    assert_eq!(styles(&whole), [(5, 0x30); 3]);
    // One per entity: the triangle, then both triangles of the fan.
    let each = styled_face(&[4, 7]);
    assert_eq!(styles(&each), [(4, 0x30), (7, 0x30), (7, 0x30)]);
    // An entity styled 0 inherits.
    let short = styled_face(&[4, 0, 9]);
    assert_eq!(styles(&short), [(4, 0x30), (0, 0x30), (0, 0x30)]);
}

#[test]
fn markup_is_read_past() {
    let mut b = W::default();
    b.uint(176).bit(false).uint(3);
    for c in [1., 2., 3.] {
        b.double(c);
    }
    uints(&mut b, &[6, 0]);
    b.uint(1).string(Some("R1")).string(Some("label")).put(7, 8);
    wire(&mut b, false);
    let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
    assert_eq!(t[0], Tessellation::Markup);
    assert!(matches!(t[1], Tessellation::Wire(_)));
}

#[test]
fn schema_fields_are_skipped_after_each_entity() {
    // A newer producer added one UInt to TESS_3D and one to TESS_Face.
    let mut s = W::default();
    s.uint(2)
        .uint(172)
        .uint(1)
        .uint(3)
        .uint(174)
        .uint(1)
        .uint(3);
    let sb = s.bytes();
    let schema = Schema::read(&mut BitReader::new(&sb)).unwrap();

    let mut b = W::default();
    square(&mut b, 172);
    b.bit(true).bit(false).bit(false).uint(0).uint(0);
    uints(&mut b, &[0, 0, 0, 3, 0, 6]);
    b.uint(1)
        .uint(174)
        .uint(0)
        .uint(0)
        .uint(0)
        .uint(0x2)
        .uint(0);
    uints(&mut b, &[1]);
    b.uint(0).bit(false).uint(99); // the face's added field
    b.uint(0).uint(77); // texture count, then TESS_3D's added field
    wire(&mut b, false);
    let t = decode(&section(2, &b), &schema, 8137).unwrap();
    assert_eq!(mesh(&t[0]).triangles, [[0, 1, 2]]);
    assert!(matches!(t[1], Tessellation::Wire(_)));
    // Without the schema the same bytes do not decode to the same thing.
    assert_ne!(
        decode(&section(2, &b), &Schema::default(), 8137).ok(),
        Some(t)
    );
}

#[test]
fn attributes_names_and_user_data_are_read_past() {
    let mut w = W::default();
    w.uint(305).uint(1).uint(201);
    w.bit(true).uint(2); // predefined title
    w.uint(2);
    w.bit(false).string(Some("k")).uint(4).string(Some("v"));
    w.bit(true).uint(1).uint(5).int(-1).uint(9);
    w.uint(42); // the attribute's schema-added field
    w.bit(false).string(Some("tess")).uint(1);
    wire(&mut w, false);
    w.uint(5).put(0b10110, 5);
    let mut sw = W::default();
    sw.uint(1).uint(ATTRIBUTE).uint(1).uint(3);
    let sb = sw.bytes();
    let schema = Schema::read(&mut BitReader::new(&sb)).unwrap();
    let bytes = w.bytes();
    let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
    let t = ctx.file_structure_tessellation().unwrap();
    assert!(matches!(t[..], [Tessellation::Wire(_)]));
    assert_eq!(ctx.r.position(), w.len());
}
