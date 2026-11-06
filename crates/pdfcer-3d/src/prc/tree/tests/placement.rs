use super::*;

#[test]
fn sons_compose_locations_and_reference_systems() {
    let systems = fixture_globals();
    assert_eq!(systems.systems, [translate([0.0, 5.0, 0.0])]);
    let occs = [
        Occ {
            sons: &[1],
            location: Some([0.0, 0.0, 2.0]),
            ..OCC
        },
        Occ {
            part: 1,
            location: Some([10.0, 0.0, 0.0]),
            ..OCC
        },
    ];
    let out = place(&occs, 1, &systems).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tessellation, 0);
    assert_eq!(out[0].matrix, translate([10.0, 5.0, 2.0]));
}

#[test]
fn hidden_and_suppressed_occurrences_draw_nothing() {
    let shown = Occ { part: 1, ..OCC };
    for hide in [
        Occ {
            behaviour: Some((0, 0)),
            ..shown
        },
        Occ {
            behaviour: Some((0, SHOW | REMOVED)),
            ..shown
        },
        Occ {
            suppressed: true,
            ..shown
        },
    ] {
        let occs = [
            Occ {
                sons: &[1, 2],
                ..OCC
            },
            Occ {
                behaviour: Some((0, SHOW)),
                ..shown
            },
            hide,
        ];
        assert_eq!(place(&occs, 0, &Globals::default()).unwrap().len(), 1);
    }
}

#[test]
fn a_camera_is_read_past() {
    let occs = [Occ {
        part: 1,
        camera: true,
        ..OCC
    }];
    assert_eq!(place(&occs, 0, &Globals::default()).unwrap().len(), 1);
}

#[test]
fn a_schema_field_added_to_the_graphics_base_is_skipped() {
    let mut w = W::default();
    w.uint(1).uint(BASE_WITH_GRAPHICS).uint(1).uint(3);
    w.append(&tree(&[Occ { part: 1, ..OCC }], 0, true));
    let bytes = w.bytes();
    let mut r = BitReader::new(&bytes);
    let schema = Schema::read(&mut r).unwrap();
    let t = Ctx::new(r, &schema, 8137).file_structure_tree().unwrap();
    assert_eq!(t.parts[0][0].tessellation, 1);
    assert_eq!(t.root, 1);
}

#[test]
fn unreadable_globals_are_an_error_when_a_reference_names_a_system() {
    // The globals section holds only its schema, so it does not read.
    let tess = square_tess();
    let file = |cs| {
        let refs = [Er::Item {
            g: (0, SHOW),
            fs: None,
            uid: 7,
            cs,
        }];
        let occs = [Occ {
            part: 1,
            refs: &refs,
            ..OCC
        }];
        placed(&tess, &occs)
    };
    // Nothing needs the globals: default colours, the square placed.
    let bytes = file(0);
    let f = crate::PrcFile::parse(&bytes).unwrap();
    assert_eq!(f.placements().unwrap().len(), 1);
    // The occurrence's reference names system 1: the globals' own error,
    // whether or not the walk applies the reference.
    let bytes = file(1);
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let globals_error = |r: Result<_, PrcError>| matches!(r, Err(PrcError::Malformed(m)) if m.contains("where 303 belongs"));
    assert!(globals_error(f.placements()));
    let ignored = f.textured_placements(
        StyleAlpha::default(),
        TextureRules::default(),
        EntityOverrides::Ignore,
        crate::StoredVisibility::Honour,
    );
    assert!(globals_error(ignored.map(|p| p.0)));
}

#[test]
fn one_tree_walk_shares_one_schema_step_allowance() {
    // A schema program for the tree entity: For 1_500_000 { {} }, about
    // 3M steps, under the 2^22 ceiling once, over it twice.
    let mut sw = W::default();
    sw.uint(1).uint(FILE_STRUCTURE_TREE).uint(5);
    for t in [15, 26, 1_500_000, 19, 21] {
        sw.uint(t);
    }
    let schema = sw.bytes();
    let tess = square_tess();
    let tree = tree(&[Occ { part: 1, ..OCC }], 0, false).bytes();
    let one = [&schema[..], &tree[..], &tess[..]];
    let other = [&schema[..], &tree[..], &[][..]];
    let bytes = crate::prc::testw::prc_container_n(&[one], &placed_model());
    let f = crate::PrcFile::parse(&bytes).unwrap();
    assert_eq!(f.placements().unwrap().len(), 1);
    let bytes = crate::prc::testw::prc_container_n(&[one, other], &placed_model());
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let r = f.placements();
    assert!(
        matches!(&r, Err(PrcError::Malformed(m)) if m.contains("step ceiling")),
        "not refused: {r:?}"
    );
}

#[test]
fn a_cycle_is_refused() {
    let occs = [Occ { sons: &[0], ..OCC }];
    assert!(matches!(
        place(&occs, 0, &Globals::default()),
        Err(PrcError::Malformed(_))
    ));
}

#[test]
fn a_missing_reference_system_is_refused() {
    let occs = [Occ { part: 1, ..OCC }];
    assert!(matches!(
        place(&occs, 1, &Globals::default()),
        Err(PrcError::Malformed(_))
    ));
}

#[test]
fn the_tree_lists_hidden_and_suppressed_subtrees_without_drawing_them() {
    let occs = [
        Occ {
            sons: &[1, 2, 4],
            name: Some(Some("Assembly")),
            ..OCC
        },
        Occ {
            part: 1,
            name: Some(Some("Bolt")),
            ..OCC
        },
        Occ {
            part: 1,
            sons: &[3],
            suppressed: true,
            name: Some(Some("Nut")),
            ..OCC
        },
        Occ {
            part: 1,
            name: Some(Some("Washer")),
            ..OCC
        },
        Occ {
            part: 1,
            behaviour: Some((0, 0)),
            name: Some(Some("Pin")),
            ..OCC
        },
    ];
    let (drawn, nodes) = listed(&occs, None);
    assert_eq!(drawn, 1);
    let row = |n: &ModelNode| {
        (
            n.name.clone().unwrap_or_default(),
            n.parent,
            n.depth,
            n.hidden,
            n.suppressed,
            n.drawn,
            n.placements.clone(),
        )
    };
    let rows: Vec<_> = nodes.iter().map(row).collect();
    let s = String::from;
    assert_eq!(
        rows,
        [
            (s("Assembly"), None, 0, false, false, true, 0..1),
            (s("Bolt"), Some(0), 1, false, false, true, 0..1),
            (s("Nut"), Some(0), 1, false, true, false, 1..1),
            (s("Washer"), Some(2), 2, false, false, false, 1..1),
            (s("Pin"), Some(0), 1, true, false, false, 1..1),
        ]
    );
    assert!(!nodes[0].has_part && nodes[1].has_part);
    assert_eq!(nodes[3].occurrence, 3);
}

#[test]
fn a_name_falls_back_to_the_prototype_then_the_part() {
    let occs = [
        Occ {
            sons: &[1, 2],
            name: Some(None),
            ..OCC
        },
        Occ {
            prototype: Some((4, None)),
            name: Some(None),
            ..OCC
        },
        Occ {
            part: 1,
            name: Some(None),
            ..OCC
        },
        Occ {
            part: 1,
            name: Some(Some("Bracket")),
            ..OCC
        },
    ];
    let from = |nodes: &[ModelNode]| -> Vec<_> {
        nodes
            .iter()
            .map(|n| (n.name.clone(), n.name_from))
            .collect()
    };
    let named = |n: &str| Some(n.to_owned());
    let (_, nodes) = listed(&occs, Some(Some("Plate")));
    assert_eq!(
        from(&nodes),
        [
            (None, NameSource::Unnamed),
            (named("Bracket"), NameSource::Prototype),
            (named("Plate"), NameSource::Part),
        ]
    );
    let (_, nodes) = listed(&occs, Some(None));
    assert_eq!(from(&nodes)[2], (None, NameSource::Unnamed));
}

#[test]
fn same_name_reuses_the_current_name() {
    let occs = [
        Occ {
            sons: &[1],
            name: Some(Some("Top")),
            ..OCC
        },
        Occ { part: 1, ..OCC },
    ];
    let (_, nodes) = listed(&occs, None);
    assert_eq!(nodes[1].name.as_deref(), Some("Top"));
    assert_eq!(nodes[1].name_from, NameSource::Occurrence);
}

#[test]
fn the_model_tree_indexes_the_placements() {
    let f = crate::PrcFile::parse(&assembly_prc()).unwrap();
    let ranges: Vec<_> = f
        .model_tree()
        .unwrap()
        .iter()
        .map(|n| (n.depth, n.placements.clone()))
        .collect();
    assert_eq!(ranges, [(0, 0..2), (1, 0..1), (1, 1..2)]);
}

#[test]
fn the_parts_drawing_a_left_out_mesh_are_named_in_tree_order() {
    let occs = [
        Occ {
            sons: &[1, 2, 3],
            name: Some(Some("Frame")),
            ..OCC
        },
        Occ {
            part: 1,
            name: Some(Some("Bolt")),
            ..OCC
        },
        Occ { part: 1, ..OCC },
        Occ {
            part: 1,
            name: Some(None),
            ..OCC
        },
    ];
    let compressed = crate::PrcFile::parse(include_bytes!(
        "../../../../../../fixtures/synthetic/prc/compressed.prc"
    ))
    .unwrap();
    let tess = compressed.file_structures[0].section(crate::SectionKind::Tessellation);
    let prc = crate::PrcFile::parse(&placed(tess, &occs)).unwrap();
    let labels = crate::prc::mesh::part_labels(&prc, &[(0, 0)]);
    assert_eq!(labels, ["Bolt x2", "occurrence 0:3"]);
    assert!(crate::prc::mesh::part_labels(&prc, &[(0, 1)]).is_empty());
    let square = crate::assemble(&assembly_prc()).unwrap();
    assert!(square.left_out_parts.is_empty());
}

#[test]
fn a_unit_not_from_the_cad_file_reads_as_assumed() {
    let prc = crate::PrcFile::parse(&assembly_prc()).unwrap();
    let want = Some(crate::ModelUnits {
        mm_per_unit: 1.0,
        from_cad_file: false,
    });
    assert_eq!(prc.units().unwrap(), want);
    assert_eq!(crate::assemble(&assembly_prc()).unwrap().units, want);
}

#[test]
fn the_named_tree_fixture_shows_every_listed_state() {
    let bytes = named_tree_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let rows: Vec<_> = f
        .model_tree()
        .unwrap()
        .iter()
        .map(|n| (n.label(), n.name_from, n.hidden, n.suppressed, n.drawn))
        .collect();
    let s = String::from;
    use NameSource::{Occurrence as O, Part, Prototype};
    assert_eq!(
        rows,
        [
            (s("Assembly"), O, false, false, true),
            (s("Bolt"), O, false, false, true),
            (s("Bracket"), Prototype, false, false, true),
            (s("Plate"), Part, false, false, true),
            (s("Pin"), O, true, false, false),
            (s("Nut"), O, false, true, false),
        ]
    );
    let m = crate::assemble(&bytes).unwrap();
    assert_eq!(m.mesh_placements, [0, 1, 2]);
    crate::prc::testw::check_fixture("named-tree.prc", &bytes);
}

#[test]
fn draw_all_places_the_stored_hidden_and_suppressed_parts_too() {
    let bytes = named_tree_prc();
    let options = crate::AssembleOptions {
        stored_visibility: crate::StoredVisibility::DrawAll,
        ..Default::default()
    };
    let m = crate::assemble_with_options(&bytes, &options).unwrap();
    assert_eq!(m.mesh_placements, [0, 1, 2, 3, 4]);
    let rows: Vec<_> = m
        .tree
        .iter()
        .map(|n| (n.label(), n.drawn, n.placements.clone()))
        .collect();
    let s = String::from;
    assert_eq!(
        rows,
        [
            (s("Assembly"), true, 0..5),
            (s("Bolt"), true, 0..1),
            (s("Bracket"), true, 1..2),
            (s("Plate"), true, 2..3),
            (s("Pin"), false, 3..4),
            (s("Nut"), false, 4..5),
        ]
    );
}

#[test]
fn the_assembly_fixture_places_and_mirrors() {
    let bytes = assembly_prc();
    let f = crate::PrcFile::parse(&bytes).unwrap();
    let p = f.placements().unwrap();
    let mut mirror = translate([5.0, 0.0, 0.0]);
    mirror[0][0] = -1.0;
    assert_eq!(p.len(), 2);
    assert_eq!((p[0].tessellation, p[0].matrix), (0, IDENTITY));
    assert_eq!((p[1].tessellation, p[1].matrix), (0, mirror));
    crate::prc::testw::check_fixture("assembly.prc", &bytes);
}

#[test]
fn normals_take_the_inverse_transpose_and_mirror_with_their_triangle() {
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let mesh = crate::TriangleMesh {
        positions: vec![[0.0; 3]; 3],
        triangles: vec![[0, 1, 2]],
        normals: vec![[r, r, 0.0], [0.0, 0.0, 3.0]],
        triangle_normals: vec![[0, 0, 1]],
        ..Default::default()
    };
    // Stretching x by 2 turns the plane x + y = 0 into x + 2y = 0.
    let mut stretch = IDENTITY;
    stretch[0][0] = 2.0;
    let s = mesh.transformed(&stretch);
    let k = 1.0 / 5f64.sqrt();
    for (got, want) in s.normals[0].iter().zip([k, 2.0 * k, 0.0]) {
        assert!((got - want).abs() < 1e-12, "{:?}", s.normals[0]);
    }
    assert_eq!(s.normals[1], [0.0, 0.0, 1.0]);
    let mut mirror = IDENTITY;
    mirror[0][0] = -1.0;
    let m = mesh.transformed(&mirror);
    assert!((m.normals[0][0] + r).abs() < 1e-12 && (m.normals[0][1] - r).abs() < 1e-12);
    assert_eq!(m.triangle_normals, [[0, 1, 0]]);
    assert_eq!(m.triangles, [[0, 2, 1]]);
}

#[test]
fn a_son_style_wins_unless_a_father_forces_his() {
    let father = FATHER_HERIT_COLOR;
    assert_eq!(resolve_style(&[g(1, 0), g(2, 0)]).style, 2, "own wins");
    assert_eq!(resolve_style(&[g(1, 0), g(0, 0)]).style, 1, "none inherits");
    assert_eq!(
        resolve_style(&[g(1, father), g(2, 0)]).style,
        1,
        "father forces"
    );
    assert_eq!(
        resolve_style(&[g(1, father), g(3, father), g(2, 0)]).style,
        1,
        "the oldest father wins"
    );
    assert_eq!(
        resolve_style(&[g(1, father), g(2, SON_HERIT_COLOR)]).style,
        2,
        "a son's heritage beats the father's"
    );
    assert_eq!(resolve_style(&[]).style, 0);
}

#[test]
fn face_styles_take_part_in_inheritance() {
    let red = Some([1.0, 0.0, 0.0, 1.0]);
    let green = Some([0.0, 1.0, 0.0, 1.0]);
    let placement = |chain| Placement {
        file_structure: 0,
        tessellation: 0,
        matrix: IDENTITY,
        colour: red,
        chain,
        palettes: std::sync::Arc::from(vec![std::sync::Arc::from([None, red, green].map(|c| {
            c.map(|rgba| Paint {
                rgba,
                alpha_unset: false,
                finish: None,
            })
        }))]),
        skins: std::sync::Arc::from(Vec::new()),
        item_override: None,
        face_overrides: Vec::new(),
        hidden_faces: Vec::new(),
    };
    let mut mesh = crate::TriangleMesh::default();
    let item = placement(vec![g(1, 0)]);
    assert_eq!(item.triangle_colours(&mesh), None);
    mesh.triangle_graphics = vec![g(0, 0), g(2, 0)];
    assert_eq!(item.triangle_colours(&mesh), Some(vec![red, green]));
    // A parent that forces its colour wins unless the face claims it.
    let forced = placement(vec![g(1, FATHER_HERIT_COLOR)]);
    assert_eq!(forced.triangle_colours(&mesh), Some(vec![red, red]));
    mesh.triangle_graphics = vec![g(2, SON_HERIT_COLOR)];
    assert_eq!(forced.triangle_colours(&mesh), Some(vec![green]));
}

#[test]
fn colour_indices_are_double_scaled() {
    let style = |is_material, index, transparency| Style {
        is_material,
        index,
        transparency,
    };
    let gl = Globals {
        colours: vec![RED, GREEN],
        materials: vec![
            Material::Plain {
                diffuse: 4,
                alpha: 0.5,
                terms: Terms::default(),
            },
            Material::Textured {
                base: 1,
                texture: 0,
                next: 0,
                uv: 0,
            },
        ],
        styles: vec![
            style(false, 4, None),
            style(false, 2, None),
            style(true, 1, Some(51)),
            style(true, 2, None),
        ],
        ..Globals::default()
    };
    let mul = StyleAlpha::Multiply;
    assert_eq!(
        gl.style_colour(1, mul),
        Some([0.0, 1.0, 0.0, 1.0]),
        "4 names entry 1"
    );
    assert_eq!(gl.style_colour(2, mul), None, "2 is not double-scaled");
    assert_eq!(
        gl.style_colour(3, mul),
        Some([0.0, 1.0, 0.0, 0.1]),
        "diffuse alpha x transparency"
    );
    assert_eq!(
        gl.style_colour(3, StyleAlpha::StyleWins),
        Some([0.0, 1.0, 0.0, 0.2]),
        "the style's transparency replaces the diffuse alpha"
    );
    assert_eq!(
        gl.style_colour(4, StyleAlpha::StyleWins),
        Some([0.0, 1.0, 0.0, 0.5]),
        "with no transparency the material alpha stands"
    );
    assert_eq!(
        gl.style_colour(4, mul),
        Some([0.0, 1.0, 0.0, 0.5]),
        "a texture's base material"
    );
    assert_eq!(
        gl.style_colour(4, StyleAlpha::ZeroUnset),
        Some([0.0, 1.0, 0.0, 0.5]),
        "a non-zero material alpha stands"
    );
    assert_eq!(gl.style_colour(0, mul), None);
    assert_eq!(gl.style_colour(9, mul), None);
}

/// The globals fixture's style 1 is colour 0 at transparency 128;
/// an occurrence reusing the current graphics takes the one read last.
#[test]
fn placements_carry_the_resolved_colour() {
    let gl = fixture_globals();
    let want = Some([1.0, 0.5, 0.0, 128.0 / 255.0]);
    let occs = [
        Occ {
            sons: &[1, 2, 3],
            ..OCC
        },
        Occ {
            part: 1,
            behaviour: Some((1, SHOW)),
            ..OCC
        },
        Occ { part: 1, ..OCC },
        Occ {
            part: 1,
            behaviour: Some((0, SHOW)),
            ..OCC
        },
    ];
    let out = place(&occs, 0, &gl).unwrap();
    let colours: Vec<_> = out.iter().map(|p| p.colour).collect();
    assert_eq!(colours, [want, want, None]);
}
