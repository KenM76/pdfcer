use super::*;

#[test]
fn a_compressed_mesh_is_read_to_its_end() {
    for (full, three_t) in [(false, false), (false, true), (true, true)] {
        for v in [ORIGIN_FROM - 1, 8137] {
            let mut b = W::default();
            compressed(&mut b, v, full, three_t);
            tess_3d(
                &mut b,
                v,
                &Mesh {
                    recalc: false,
                    indices: &[0, 0, 0, 3, 0, 6, 0, 0, 0, 3, 0, 6, 0, 9],
                    faces: &[(0x2 | 0x4, 0, &[1, 1, 4])],
                    textures: 0,
                    uvs: &[],
                },
            );
            let w = section(2, &b);
            let bytes = w.bytes();
            let schema = Schema::default();
            let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, v);
            let t = ctx.file_structure_tessellation().unwrap();
            assert!(
                matches!(
                    t[..],
                    [
                        Tessellation::Compressed { triangles: 1, .. },
                        Tessellation::Mesh(_)
                    ]
                ),
                "full={full} v={v}: {t:?}"
            );
            let Tessellation::Compressed { mesh, .. } = &t[0] else {
                unreachable!()
            };
            assert_eq!(
                mesh.as_ref().map(|m| (
                    m.positions.len(),
                    m.triangles.clone(),
                    m.normals_recalculated
                )),
                // This branch sets must_recalculate_normals.
                (!full && three_t).then(|| (3, vec![[0, 1, 2]], true)),
                "full={full} three_t={three_t} v={v}"
            );
            if let Some(m) = mesh {
                // One face, line attribute 1; `full` adds behaviour 1.
                let bits = u16::from(full);
                assert_eq!(
                    m.triangle_graphics,
                    vec![crate::prc::tree::Graphics {
                        style: 1,
                        bits,
                        fs: 0
                    }],
                    "full={full} v={v}"
                );
            }
            assert_eq!(ctx.r.position(), w.len(), "full={full} v={v}");
        }
    }
}

#[test]
fn compressed_line_attributes_style_each_face() {
    let g = |style, bits| crate::prc::tree::Graphics { style, bits, fs: 0 };
    let face_of = [0, 1, 1, 0];
    assert_eq!(
        compressed_graphics(&face_of, &[], &[2, 0], &[], 2),
        vec![g(2, 0), g(0, 0), g(0, 0), g(2, 0)]
    );
    assert_eq!(
        compressed_graphics(&face_of, &[false, false], &[2, 3], &[16, 8], 2),
        vec![g(2, 16), g(3, 8), g(3, 8), g(2, 16)]
    );
    // All inherit, a flagged face, or a count that is not one per face.
    assert!(compressed_graphics(&face_of, &[], &[0, 0], &[], 2).is_empty());
    assert!(compressed_graphics(&face_of, &[false, true], &[2, 3], &[], 2).is_empty());
    assert!(compressed_graphics(&face_of, &[], &[2], &[], 2).is_empty());
}

#[test]
fn refusals_and_damage_are_errors() {
    // Edge statuses neither T nor 3T long.
    let mut c = W::default();
    c.uint(173).bit(false).bit(true).double(0.001);
    c.put(0, 32).put(0, 32).put(0, 32);
    c.bit(false).uint(0).bit(false).uint(2).put(0, 16);
    c.bit(false).uint(1).put(1, 8).bit(false);
    assert!(matches!(
        decode(&section(1, &c), &Schema::default(), 8137),
        Err(PrcError::Malformed(_))
    ));
    // Optimised vertex colours.
    let mut o = W::default();
    square(&mut o, 175);
    o.uint(0).bit(true).bit(false).bit(false).bit(true);
    assert!(matches!(
        decode(&section(1, &o), &Schema::default(), 8137),
        Err(PrcError::Unsupported(_))
    ));
    // A point index that is not a multiple of 3, and one past the end.
    for bad in [4, 12] {
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0, 0, 0, bad, 0, 6],
                faces: &[(0x2, 0, &[1])],
                textures: 0,
                uvs: &[],
            },
        );
        assert!(matches!(
            decode(&section(1, &b), &Schema::default(), 8137),
            Err(PrcError::Malformed(_))
        ));
    }
    // A fan whose count claims a billion points.
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: false,
            indices: &[0, 0],
            faces: &[(0x4, 0, &[1, 0x3FFF_FFFF])],
            textures: 0,
            uvs: &[],
        },
    );
    assert!(decode(&section(1, &b), &Schema::default(), 8137).is_err());
    // A tessellation count far past the data.
    let mut w = W::default();
    w.uint(305).uint(0).bit(true).uint(u32::MAX);
    assert!(matches!(
        decode(&w, &Schema::default(), 8137),
        Err(PrcError::Truncated(_))
    ));
    // A wrong entity type where the section's own belongs.
    let mut w = W::default();
    w.uint(306);
    assert!(matches!(
        decode(&w, &Schema::default(), 8137),
        Err(PrcError::Malformed(_))
    ));
}

/// `fixtures/synthetic/prc/wired.prc` is [`wired_prc`], and draws its
/// wire red over the grey square.
#[test]
fn the_wired_fixture_is_current_and_its_wire_is_drawn() {
    let bytes = wired_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let t = f.file_structures[0].tessellations().unwrap();
    let Tessellation::Wire(w) = &t[1] else {
        panic!("not a wire")
    };
    assert_eq!(w.polylines, [vec![[0., 0., 0.], [1., 1., 0.]]]);
    assert_eq!(w.colours, [vec![[255, 0, 0, 255]; 2]]);
    crate::prc::testw::check_fixture("wired.prc", &bytes);
    let model = crate::assemble(&bytes).unwrap();
    assert_eq!((model.wires, model.lines.len()), (1, 1));
    #[cfg(feature = "render")]
    {
        let bounds = crate::Bounds::of(&model.meshes).unwrap();
        let camera = crate::Camera::fit(&bounds, [0., 0., -1.], [0., 1., 0.], false, 1.0).unwrap();
        let options = crate::RenderOptions {
            width: 50,
            height: 50,
            ..crate::RenderOptions::default()
        };
        let image = crate::render_model(&model, &camera, &options).unwrap();
        let px = |x: usize, y: usize| &image.rgba[(y * 50 + x) * 4..][..4];
        // The diagonal runs bottom left to top right through the centre.
        assert!((10..40).all(|x| px(x, 49 - x) == [255, 0, 0, 255]));
        assert_ne!(px(25, 20), [255, 0, 0, 255]);
    }
}

/// The CLI's `3d-mesh` fixtures under `fixtures/synthetic/prc/` are
/// exactly what [`square_prc`], [`compressed_prc`] and
/// [`compressed_prc_rebuilt`] build. Set
/// `PDFCER_WRITE_FIXTURES=1` to rewrite them.
#[test]
fn the_prc_fixtures_are_current_and_decode() {
    let bytes = square_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let t = f.file_structures[0].tessellations().unwrap();
    assert_eq!(mesh(&t[0]).triangles, [[0, 1, 2], [0, 2, 3]]);
    crate::prc::testw::check_fixture("square.prc", &bytes);
    let bytes = compressed_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let t = f.file_structures[0].tessellations().unwrap();
    assert!(matches!(
        t[..],
        [Tessellation::Compressed {
            triangles: 1,
            mesh: None,
            not_rebuilt: Some(_),
            best_fit: false,
            partial: None,
        }]
    ));
    crate::prc::testw::check_fixture("compressed.prc", &bytes);
    let bytes = compressed_prc_rebuilt();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let t = f.file_structures[0].tessellations().unwrap();
    assert!(matches!(
        t[..],
        [Tessellation::Compressed {
            triangles: 1,
            mesh: Some(_),
            not_rebuilt: None,
            best_fit: false,
            partial: None,
        }]
    ));
    crate::prc::testw::check_fixture("compressed_triangle.prc", &bytes);
}

/// `best_fit.prc` rebuilds only by best fit: drawn by default and
/// counted, left out with the strict-setting reason under
/// [`crate::MeshFit::Unique`].
#[test]
fn the_best_fit_fixture_is_current_and_rebuilt_only_by_best_fit() {
    let bytes = best_fit_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let t = f.file_structures[0].tessellations().unwrap();
    let [
        Tessellation::Compressed {
            triangles,
            mesh: Some(m),
            best_fit: true,
            ..
        },
    ] = &t[..]
    else {
        panic!("{t:?}");
    };
    assert_eq!((*triangles, m.triangles.len()), (28, 28));
    crate::prc::testw::check_fixture("best_fit.prc", &bytes);
    let model = crate::assemble(&bytes).unwrap();
    assert_eq!((model.rebuilt, model.best_fit, model.compressed), (1, 1, 0));
    let strict = crate::AssembleOptions {
        mesh_fit: crate::MeshFit::Unique,
        ..Default::default()
    };
    let err = crate::assemble_with_options(&bytes, &strict).unwrap_err();
    assert!(
        err.to_string()
            .contains("strict mesh-fit setting draws unique fits only"),
        "{err}"
    );
}

/// `partial.prc` rebuilds by no fit: left out by default, drawn in
/// part under [`crate::MeshFit::Partial`] (the sound prefix) and
/// [`crate::MeshFit::Continue`] (the whole decode), each disclosed.
#[test]
fn the_partial_fixture_is_current_and_drawn_only_in_part() {
    let bytes = partial_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let t = f.file_structures[0].tessellations().unwrap();
    let [
        Tessellation::Compressed {
            triangles: 4,
            mesh: None,
            best_fit: false,
            partial: Some(p),
            ..
        },
    ] = &t[..]
    else {
        panic!("{t:?}");
    };
    assert_eq!((p.sound, p.total, p.mesh.triangles.len()), (3, 4, 4));
    assert_eq!(p.mesh.triangle_faces, vec![0; 4]);
    assert_eq!(p.sound_mesh().triangle_faces, vec![0; 3]);
    crate::prc::testw::check_fixture("partial.prc", &bytes);
    assert!(matches!(
        crate::assemble(&bytes),
        Err(crate::AssembleError::CompressedOnly { .. })
    ));
    for (fit, n) in [(crate::MeshFit::Partial, 3), (crate::MeshFit::Continue, 4)] {
        let options = crate::AssembleOptions {
            mesh_fit: fit,
            ..Default::default()
        };
        let model = crate::assemble_with_options(&bytes, &options).unwrap();
        assert_eq!(model.meshes.len(), 1, "{fit:?}");
        assert_eq!(model.meshes[0].triangles.len(), n, "{fit:?}");
        assert_eq!(
            (model.partial, model.partial_triangles, model.compressed),
            (1, (n, 4), 0),
            "{fit:?}"
        );
        assert_eq!(
            model.partial_why,
            vec![("an edge is shared by more than two triangles".to_owned(), 1)]
        );
        assert!(model.skipped_why.is_empty() && model.left_out_parts.is_empty());
    }
}

#[test]
fn a_partial_mesh_point_leaves_out_the_triangles_using_it() {
    let coords: Vec<f64> = SQUARE.iter().copied().chain([0.0]).collect();
    let m = Mesh {
        recalc: false,
        indices: &[3, 0, 0, 3, 3, 6, 3, 0, 3, 6, 0, 12],
        faces: &[(0x2, 0, &[2])],
        textures: 0,
        uvs: &[],
    };
    let mut b = W::default();
    tess_3d_over(&mut b, 8137, &m, &coords, &NORMALS);
    wire(&mut b, false);
    let model = salvaged(&prc_file(&section(2, &b)), PARTIAL_POINT);
    assert_eq!(model.triangles, 1);
    assert_eq!(model.wires, 1);
}

#[test]
fn a_partial_wire_point_leaves_out_the_polyline_using_it() {
    let coords: Vec<f64> = SQUARE.iter().copied().chain([0.0; 2]).collect();
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: false,
            indices: &[3, 0, 0, 3, 3, 6],
            faces: &[(0x2, 0, &[1])],
            textures: 0,
            uvs: &[],
        },
    );
    coordinates(&mut b, 175, &coords);
    uints(&mut b, &[2, 0, 3, 2, 6, 12]);
    b.bit(false);
    let model = salvaged(&prc_file(&section(2, &b)), PARTIAL_POINT);
    assert_eq!((model.triangles, model.wires), (1, 1));
    assert_eq!(model.lines.len(), 1);
    assert_eq!(model.lines[0].points.len(), 2);
}

#[test]
fn a_face_behaviour_wider_than_16_bits_leaves_out_that_face() {
    let whole = crate::assemble(&two_faces(0x30)).unwrap();
    assert_eq!((whole.triangles, whole.damaged), (2, 0));
    let model = salvaged(&two_faces(0x1_0000 | 0x0008), WIDE_BEHAVIOUR);
    assert_eq!(model.triangles, 1);
}

#[test]
fn a_huffman_prefix_pair_leaves_out_its_tessellation() {
    let mut b = W::default();
    tess_3d(
        &mut b,
        8137,
        &Mesh {
            recalc: false,
            indices: &[3, 0, 0, 3, 3, 6],
            faces: &[(0x2, 0, &[1])],
            textures: 0,
            uvs: &[],
        },
    );
    // A compressed tessellation whose edge-status table stores `10`
    // and `100`: the reader loses its place there.
    b.uint(173).bit(false).bit(true).double(0.001);
    b.put(0, 32).put(0, 32).put(0, 32);
    b.bit(false).uint(9);
    for _ in 0..9 {
        b.put(2, 8);
    }
    for _ in 0..9 {
        b.bit(false).bit(true);
    }
    b.bit(true)
        .huffman(2, 2, &[(0, 0b10, 2), (1, 0b100, 3)], &[0, 1, 0]);
    let model = salvaged(&prc_file(&section(2, &b)), "prefix of another");
    assert_eq!(model.triangles, 1);
    assert_eq!(model.compressed, 0);
}

#[test]
fn a_model_file_end_past_the_data_is_read_to_the_data_end() {
    let mut bytes = square_prc();
    let len = bytes.len() as u32;
    let at = (0..120)
        .find(|&i| bytes[i..i + 4] == len.to_le_bytes())
        .unwrap();
    bytes[at..at + 4].copy_from_slice(&(len + 1).to_le_bytes());
    let model = salvaged(&bytes, crate::prc::mesh::SHORT_FILE);
    assert_eq!(model.triangles, 2);
}
