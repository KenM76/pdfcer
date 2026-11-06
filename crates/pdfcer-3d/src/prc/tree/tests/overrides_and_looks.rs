use super::*;

#[test]
fn a_part_under_markups_and_a_view_is_placed_and_coloured() {
    let tess = crate::PrcFile::parse(include_bytes!(
        "../../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let tess = tess.file_structures[0].section(crate::SectionKind::Tessellation);
    let occs = [
        Occ {
            sons: &[1],
            markups: true,
            ..OCC
        },
        Occ {
            part: 1,
            behaviour: Some((1, SHOW)),
            markups: true,
            ..OCC
        },
    ];
    let bytes = crate::prc::testw::prc_container(
        &coloured_globals(false).bytes(),
        &tree(&occs, 0, false).bytes(),
        tess,
        &placed_model(),
    );
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let p = f.placements_with(StyleAlpha::StyleWins).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].colour, Some([1.0, 0.0, 0.0, 1.0]));
    let nodes = f.model_tree().unwrap();
    let counts: Vec<_> = nodes.iter().map(|n| (n.markups, n.views)).collect();
    assert_eq!(counts, [(2, 1), (2, 1)]);
    assert!(nodes[1].has_part && nodes[1].drawn);
    let views = f.view_lights().unwrap();
    assert_eq!(views.len(), 2, "each occurrence's view");
    assert!(
        views
            .iter()
            .all(|v| v.default && v.active && v.lights.len() == 4)
    );
    assert!(f.lights().unwrap().is_empty(), "no occurrence lights");
    let mut options = crate::AssembleOptions::default();
    let m = crate::assemble_with_options(&bytes, &options).unwrap();
    assert_eq!(m.lights_view, Some(0));
    assert_eq!(m.lights, m.view_lights[0].lights);
    options.view_lights = crate::ViewLightUse::Ignore;
    let m = crate::assemble_with_options(&bytes, &options).unwrap();
    assert_eq!(
        (m.lights_view, m.lights.len(), m.view_lights.len()),
        (None, 0, 2)
    );
}

#[test]
fn a_prototype_takes_the_palette_of_the_structure_that_styled_it() {
    let bytes = coloured_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let blue = 128.0 / 255.0;
    let colours = |rule| -> Vec<_> {
        let p = f.placements_with(rule).unwrap();
        assert!(p.iter().all(|p| p.file_structure == 0));
        let xs: Vec<_> = p.iter().map(|p| p.matrix[0][3]).collect();
        assert_eq!(xs, [0.0, 2.0, 4.0]);
        p.iter().map(|p| p.colour).collect()
    };
    assert_eq!(
        colours(StyleAlpha::StyleWins),
        [
            Some([1.0, 0.0, 0.0, 1.0]),
            Some([0.0, 0.0, 1.0, blue]),
            None
        ]
    );
    assert_eq!(
        colours(StyleAlpha::Multiply),
        [Some([1.0, 0.0, 0.0, 0.0]), Some([0.0, 0.0, 1.0, 0.0]), None]
    );
    let m = crate::assemble(&bytes).unwrap();
    assert_eq!(
        m.colours,
        [Some([255, 0, 0, 255]), Some([0, 0, 255, 128]), None]
    );
    crate::prc::testw::check_fixture("coloured.prc", &bytes);
}

/// An item naming tessellation 1 (0-based) of a section holding one is
/// malformed: a strict fit refuses it; a lenient one leaves that
/// placement out, counts it, and refuses only when nothing else draws.
#[test]
fn a_placement_naming_a_missing_tessellation_is_refused() {
    let tess = square_tess();
    let occs = [Occ { part: 1, ..OCC }];
    let file = |tess_index| {
        placed_tree(
            &tess,
            &tree_on(&occs, (0, tess_index), false, None, None).bytes(),
        )
    };
    let strict = crate::AssembleOptions {
        mesh_fit: crate::MeshFit::Unique,
        ..Default::default()
    };
    assert!(crate::assemble_with_options(&file(1), &strict).is_ok());
    let err = crate::assemble_with_options(&file(2), &strict).unwrap_err();
    assert!(
        matches!(&err, crate::AssembleError::Prc(PrcError::Malformed(why))
            if why.contains("tessellation 1")),
        "{err:?}"
    );
    let err = crate::assemble(&file(2)).unwrap_err();
    assert!(
        matches!(&err, crate::AssembleError::Damaged { count: 1, .. }),
        "{err:?}"
    );
}

/// Of two parts, the one naming a tessellation the section does not
/// hold is left out under the default fit and counted with its label;
/// the other draws.
#[test]
fn a_placement_naming_a_missing_tessellation_is_left_out_when_lenient() {
    let tess = square_tess();
    let mut w = W::default();
    w.uint(FILE_STRUCTURE_TREE);
    base(&mut w);
    w.uint(2);
    part(&mut w, (0, 1), false, None, None);
    part(&mut w, (0, 2), false, None, None);
    let occs = [
        Occ {
            sons: &[1, 2],
            ..OCC
        },
        Occ {
            part: 1,
            name: Some(Some("Kept")),
            ..OCC
        },
        Occ {
            part: 2,
            name: Some(Some("Broken")),
            ..OCC
        },
    ];
    w.uint(occs.len() as u32);
    for o in &occs {
        occurrence(&mut w, o, false);
    }
    w.uint(FILE_STRUCTURE);
    base(&mut w);
    w.uint(100).uint(1).uint(0);
    let m = crate::assemble(&placed_tree(&tess, &w.bytes())).unwrap();
    assert_eq!(m.triangles, 2);
    assert_eq!(m.damaged, 1);
    assert_eq!(
        m.damaged_why,
        [(crate::prc::mesh::MISSING_TESSELLATION.to_owned(), 1)]
    );
    assert_eq!(m.damaged_parts, ["Broken"]);
}

/// A topology reference to face 0 of a two-face B-rep with Show clear,
/// or Removed set, removes face 0's triangles whatever its style and
/// keeps face 1's; with Show set and no style it changes nothing
/// [WD 7.2.4.2, 7.4.7].
#[test]
fn a_topology_reference_that_hides_a_face_removes_its_triangles() {
    let tess = square_tess();
    let assemble = |g: (u32, u16)| {
        let refs = [Er::Faces { g, faces: &[0] }];
        let occs = [Occ {
            part: 1,
            refs: &refs,
            ..OCC
        }];
        let tree = tree_on(&occs, (0, 1), false, None, Some((1, 1)));
        let bytes = placed_tree(&tess, &tree.bytes());
        crate::prc::testw::assemble_two_faced(&bytes).unwrap()
    };
    let all = |m: &crate::AssembledModel| -> Vec<[u32; 3]> {
        m.meshes.iter().flat_map(|m| m.triangles.clone()).collect()
    };
    let shown = assemble((0, SHOW));
    let both = all(&shown);
    let n = both.len() / 2;
    assert!(n > 0 && both.len() == 2 * n);
    assert_ne!(both[..n], both[n..], "Show set keeps both faces");
    for g in [(0, 0), (0, REMOVED | SHOW), (1, 0)] {
        let m = assemble(g);
        assert_eq!(all(&m), both[n..], "{g:?} keeps only face 1");
        assert_eq!(m.mesh_placements.len(), m.meshes.len());
    }
}

#[test]
fn an_occurrence_overrides_the_entities_its_subtree_places() {
    let bytes = overridden_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let red = Some([1.0, 0.0, 0.0, 1.0]);
    let blue = Some([0.0, 0.0, 1.0, 128.0 / 255.0]);
    let walk = |scope| {
        f.textured_placements(
            StyleAlpha::default(),
            TextureRules::default(),
            scope,
            crate::StoredVisibility::Honour,
        )
        .unwrap()
        .0
    };
    let p = walk(EntityOverrides::Subtree);
    let xs: Vec<_> = p.iter().map(|p| p.matrix[0][3]).collect();
    assert_eq!(xs, [0.0, 2.0], "the third occurrence hides its square");
    let colours: Vec<_> = p.iter().map(|p| p.colour).collect();
    assert_eq!(colours, [blue, red]);
    assert_eq!(p[0].face_overrides, []);
    let faces: Vec<_> = p[1]
        .face_overrides
        .iter()
        .map(|(f, g)| (*f, g.style))
        .collect();
    assert_eq!(faces, [(0, 2)]);

    let p = walk(EntityOverrides::Everywhere);
    let colours: Vec<_> = p.iter().map(|p| p.colour).collect();
    assert_eq!(colours, [blue, blue, blue], "the first occurrence's wins");
    assert!(p.iter().all(|p| p.face_overrides.len() == 1));

    let p = walk(EntityOverrides::Ignore);
    let colours: Vec<_> = p.iter().map(|p| p.colour).collect();
    assert_eq!(colours, [red, red, None]);
    assert!(
        p.iter()
            .all(|p| p.face_overrides.is_empty() && p.item_override.is_none())
    );
    crate::prc::testw::check_fixture("overridden.prc", &bytes);
}

/// Each mesh names the placement that drew it, and the tree read with
/// the same overrides indexes those placements: hiding the third
/// occurrence's square shifts nothing before it, and a placement whose
/// faces differ in colour draws two meshes.
#[test]
fn an_assembled_mesh_names_its_placement_under_the_same_overrides() {
    let bytes = overridden_prc();
    let read = |scope| {
        let options = crate::AssembleOptions {
            entity_overrides: scope,
            ..crate::AssembleOptions::default()
        };
        let m = crate::assemble_with_options(&bytes, &options).unwrap();
        let ranges: Vec<_> = m.tree.iter().map(|n| n.placements.clone()).collect();
        (m.mesh_placements, ranges)
    };
    assert_eq!(
        read(EntityOverrides::Subtree),
        (vec![0, 1], vec![0..2, 0..1, 1..2, 2..2])
    );
    assert_eq!(
        read(EntityOverrides::Ignore),
        (vec![0, 1, 2], vec![0..3, 0..1, 1..2, 2..3])
    );
}

#[test]
fn a_shiny_square_reads_its_highlight_from_the_file() {
    let bytes = shiny_prc();
    crate::prc::testw::check_fixture("shiny.prc", &bytes);
    let m = crate::assemble(&bytes).unwrap();
    let f = m.finishes[0].unwrap();
    assert_eq!(
        (f.specular, f.emissive, f.shininess),
        ([0.75; 3], [0.0; 3], 0.5)
    );
    assert!(f.is_specular() && !f.is_emissive());
}

#[test]
fn a_zero_material_alpha_under_a_bare_style_is_unset_by_default() {
    let bytes = alpha_unset_prc();
    crate::prc::testw::check_fixture("alpha-unset.prc", &bytes);
    let grey = 191;
    let read = |rule| {
        let options = crate::AssembleOptions {
            style_alpha: rule,
            ..crate::AssembleOptions::default()
        };
        let m = crate::assemble_with_options(&bytes, &options).unwrap();
        (m.colours, m.alpha_unset)
    };
    assert_eq!(
        read(StyleAlpha::default()),
        (vec![Some([grey, grey, grey, 255])], 1)
    );
    for rule in [StyleAlpha::StyleWins, StyleAlpha::Multiply] {
        assert_eq!(
            read(rule),
            (vec![Some([grey, grey, grey, 0])], 0),
            "{rule:?}"
        );
    }
}

#[test]
fn a_textured_square_draws_its_picture() {
    let bytes = textured_prc(1);
    let m = crate::assemble(&bytes).unwrap();
    assert_eq!(m.unplaced, None);
    assert_eq!((m.textured, m.textures.len()), (1, 1));
    assert_eq!(m.mesh_textures, [Some(0)]);
    assert!(m.texture_notes.is_empty(), "{:?}", m.texture_notes);
    let t = &m.textures[0];
    assert_eq!((t.width, t.height, t.uv_set), (2, 2, 0));
    assert_eq!(t.function, crate::TextureFunction::Replace);
    assert_eq!(t.rgba[..4], [255, 0, 0, 255]);
    crate::prc::testw::check_fixture("textured.prc", &bytes);
}

#[cfg(feature = "render")]
#[test]
fn a_textured_square_renders_its_picture_the_right_way_up() {
    let m = crate::assemble(&textured_prc(1)).unwrap();
    // The fixture's material names no highlight and no glow.
    assert!(m.finishes.iter().all(Option::is_none), "{:?}", m.finishes);
    let camera = crate::Camera {
        eye: [0.5, 0.5, 10.0],
        target: [0.5, 0.5, 0.0],
        up: [0.0, 1.0, 0.0],
        projection: crate::Projection::Orthographic { height: 1.0 },
    };
    let options = crate::RenderOptions {
        width: 2,
        height: 2,
        ..crate::RenderOptions::default()
    };
    let image = crate::render_model(&m, &camera, &options).unwrap();
    // Pixel centres fall on texel centres, so filtering mixes nothing.
    let px = |x: usize, y: usize| image.rgba[(y * 2 + x) * 4..][..4].to_vec();
    // v = 0 is the picture's bottom row, the square's bottom edge.
    assert_eq!(px(0, 0), [255, 0, 0, 255], "top left");
    assert_eq!(px(1, 0), [0, 255, 0, 255], "top right");
    assert_eq!(px(0, 1), [0, 0, 255, 255], "bottom left");
    assert_eq!(px(1, 1), [255, 255, 255, 255], "bottom right");
}

#[test]
fn an_undrawable_texture_draws_the_base_colour_and_says_why() {
    let m = crate::assemble(&textured_prc(TEXTURE_MAPPING_OPERATOR)).unwrap();
    assert_eq!(m.textured, 0);
    assert!(m.textures.is_empty());
    assert_eq!(
        m.texture_notes,
        [(super::super::why::MAPPING.to_owned(), 1)]
    );
    assert_eq!(m.colours, [Some([128, 128, 128, 255])]);
}

#[test]
fn a_mesh_without_coordinates_or_a_later_level_is_disclosed() {
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let bare = square.file_structures[0].section(crate::SectionKind::Tessellation);
    let m = crate::assemble(&textured_prc_with(&textured_globals(1, 0), bare)).unwrap();
    assert_eq!(m.textured, 0);
    assert_eq!(m.texture_notes, [(super::super::why::NO_UVS.to_owned(), 1)]);
    let uvs = crate::prc::tess::tests::textured_square_section();
    let m = crate::assemble(&textured_prc_with(&textured_globals(1, 1), &uvs)).unwrap();
    assert_eq!(m.textured, 1, "the first level still draws");
    assert_eq!(
        m.texture_notes,
        [(super::super::why::MORE_LEVELS.to_owned(), 1)]
    );
}

#[test]
fn a_material_keeps_its_specular_emissive_and_ambient_terms() {
    let (grey, white, orange) = ([0.5; 3], [1.0; 3], [1.0, 0.5, 0.0]);
    let gl = lit_globals(&[
        (0, 0, 4, 0.75),
        (1, 7, 0, 0.0),
        (7, 0, 0, 0.0),
        (1, 0, 0, 0.5),
        (2, 2, 2, 9.0),
    ]);
    let finish = |style| gl.style_paint(style, StyleAlpha::default()).unwrap().finish;
    let shiny = finish(1).unwrap();
    assert_eq!(
        (shiny.ambient, shiny.specular, shiny.shininess),
        (grey, white, 0.75),
        "no ambient index reads as the diffuse colour"
    );
    assert_eq!(shiny.emissive, [0.0; 3]);
    assert_eq!(finish(2).unwrap().emissive, orange);
    assert_eq!(finish(3).unwrap().ambient, orange);
    assert_eq!(finish(4), None, "diffuse-coloured ambient, no light terms");
    assert_eq!(finish(5), None, "indices that name nothing are unset");
}
