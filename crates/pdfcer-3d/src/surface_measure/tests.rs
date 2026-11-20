use super::face::perimeter;
use super::*;
use crate::indirect::tests::quad;

/// A unit-wide strip at height `z`, tilted `degrees` about the X axis.
fn strip(z: f64, degrees: f64) -> TriangleMesh {
    let (s, c) = degrees.to_radians().sin_cos();
    let at = |x: f64, y: f64| [x, y * c, z + y * s];
    quad([at(0.0, 0.0), at(4.0, 0.0), at(4.0, 3.0), at(0.0, 3.0)])
}

fn pick(mesh: usize, triangle: usize, point: [f64; 3]) -> SurfacePick {
    SurfacePick {
        mesh,
        triangle,
        point,
    }
}

fn tolerance(parallel: f64) -> SurfaceTolerance {
    SurfaceTolerance {
        parallel_degrees: parallel,
        ..SurfaceTolerance::default()
    }
}

#[test]
fn parallel_surfaces_report_the_distance_between_their_planes() {
    let meshes = [strip(0.0, 0.0), strip(2.5, 0.0)];
    let m = measure_surfaces(
        &meshes,
        &pick(0, 0, [1.0, 1.0, 0.0]),
        &pick(1, 1, [3.0, 2.0, 2.5]),
        &SurfaceTolerance::default(),
    )
    .unwrap();
    assert_eq!(
        m.relation,
        SurfaceRelation::Parallel {
            distance: 2.5,
            forced: false
        }
    );
    assert!(m.angle_degrees.abs() < 1e-9);
    assert_eq!(m.delta, [2.0, 1.0, 2.5]);
    assert!((m.pick_distance - (4.0f64 + 1.0 + 6.25).sqrt()).abs() < 1e-12);
    assert_eq!((m.a.triangles, m.a.area), (2, 12.0));
    assert_eq!(m.a.centroid, [2.0, 1.5, 0.0]);
    assert_eq!(m.a.face, None);
    assert!(!m.a.whole_face);
}

#[test]
fn the_two_sides_of_a_plate_are_parallel() {
    let mut under = strip(0.0, 0.0);
    under.triangles = vec![[0, 2, 1], [0, 3, 2]];
    let meshes = [under, strip(1.0, 0.0)];
    let m = measure_surfaces(
        &meshes,
        &pick(0, 0, [1.0, 1.0, 0.0]),
        &pick(1, 0, [1.0, 1.0, 1.0]),
        &SurfaceTolerance::default(),
    )
    .unwrap();
    assert!((m.normals_degrees - 180.0).abs() < 1e-9);
    assert!(matches!(
        m.relation,
        SurfaceRelation::Parallel { distance, forced: false } if (distance - 1.0).abs() < 1e-12
    ));
}

#[test]
fn beyond_the_tolerance_the_angle_is_reported_and_within_it_the_distance() {
    let meshes = [strip(0.0, 0.0), strip(2.0, 30.0)];
    let (a, b) = (pick(0, 0, [1.0, 1.0, 0.0]), pick(1, 0, [1.0, 0.0, 2.0]));
    let angled = measure_surfaces(&meshes, &a, &b, &SurfaceTolerance::default()).unwrap();
    let SurfaceRelation::Angled { degrees } = angled.relation else {
        panic!("{:?}", angled.relation);
    };
    assert!((degrees - 30.0).abs() < 1e-9);

    let wide = measure_surfaces(&meshes, &a, &b, &tolerance(30.5)).unwrap();
    assert!(matches!(
        wide.relation,
        SurfaceRelation::Parallel { distance, forced: false } if (distance - 2.0).abs() < 1e-12
    ));
    assert!((wide.angle_degrees - 30.0).abs() < 1e-9);

    let narrow = measure_surfaces(&meshes, &a, &b, &tolerance(29.5)).unwrap();
    assert!(matches!(narrow.relation, SurfaceRelation::Angled { .. }));
}

#[test]
fn forcing_parallel_measures_a_distance_and_says_it_was_forced() {
    let meshes = [strip(0.0, 0.0), strip(2.0, 30.0)];
    let t = SurfaceTolerance {
        force_parallel: true,
        ..SurfaceTolerance::default()
    };
    let m = measure_surfaces(
        &meshes,
        &pick(0, 0, [1.0, 1.0, 0.0]),
        &pick(1, 0, [1.0, 0.0, 2.0]),
        &t,
    )
    .unwrap();
    assert!(matches!(
        m.relation,
        SurfaceRelation::Parallel { distance, forced: true } if (distance - 2.0).abs() < 1e-12
    ));
}

/// Two faces in one mesh: a flat floor, and a face bent along its middle.
fn floor_and_bent() -> TriangleMesh {
    TriangleMesh {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 5.0],
            [1.0, 0.0, 5.0],
            [1.0, 1.0, 5.0],
            [0.0, 1.0, 6.0],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
        faces: vec![0..2, 2..4],
        ..TriangleMesh::default()
    }
}

#[test]
fn a_surface_stays_inside_its_face_and_says_when_it_is_not_all_of_it() {
    let meshes = [floor_and_bent()];
    let flat = surface_at(&meshes, &pick(0, 1, [0.2, 0.8, 0.0]), 0.5).unwrap();
    assert_eq!(
        (flat.face, flat.whole_face, flat.triangles),
        (Some(0), true, 2)
    );
    assert_eq!(flat.flatness_degrees, 0.0);
    assert_eq!(flat.area, 1.0);

    let bent = surface_at(&meshes, &pick(0, 2, [0.8, 0.2, 5.0]), 0.5).unwrap();
    assert_eq!(
        (bent.face, bent.whole_face, bent.triangles),
        (Some(1), false, 1)
    );
    assert_eq!(bent.normal, [0.0, 0.0, 1.0]);
}

#[test]
fn coplanar_triangles_of_a_mesh_with_no_faces_are_one_surface() {
    let mut mesh = strip(0.0, 0.0);
    mesh.positions
        .extend([[9.0, 0.0, 0.0], [10.0, 0.0, 0.0], [9.0, 1.0, 0.0]]);
    mesh.positions
        .extend([[9.0, 0.0, 1.0], [10.0, 0.0, 1.0], [9.0, 1.0, 1.0]]);
    mesh.triangles.extend([[4, 5, 6], [7, 8, 9]]);
    let patch = surface_at(&[mesh], &pick(0, 0, [1.0, 1.0, 0.0]), 0.5).unwrap();
    assert_eq!(patch.triangles, 3);
    assert_eq!(patch.area, 12.5);
}

#[test]
fn unusable_questions_are_refused_by_name() {
    let mut flat = strip(0.0, 0.0);
    flat.positions.push([0.0, 0.0, 0.0]);
    flat.triangles.push([0, 4, 0]);
    let meshes = [flat];
    let good = pick(0, 0, [1.0, 1.0, 0.0]);
    let measure = |a: &SurfacePick, b: &SurfacePick, t: &SurfaceTolerance| {
        measure_surfaces(&meshes, a, b, t).unwrap_err()
    };
    let d = SurfaceTolerance::default();
    assert_eq!(
        measure(&good, &pick(3, 0, [0.0; 3]), &d),
        SurfaceMeasureError::NoSuchTriangle {
            mesh: 3,
            triangle: 0
        }
    );
    assert_eq!(
        measure(&good, &pick(0, 9, [0.0; 3]), &d),
        SurfaceMeasureError::NoSuchTriangle {
            mesh: 0,
            triangle: 9
        }
    );
    assert_eq!(
        measure(&good, &pick(0, 2, [0.0; 3]), &d),
        SurfaceMeasureError::Degenerate
    );
    assert_eq!(
        measure(&good, &pick(0, 0, [f64::NAN, 0.0, 0.0]), &d),
        SurfaceMeasureError::BadPoint
    );
    for bad in [-1.0, 90.5, f64::NAN] {
        assert_eq!(
            measure(&good, &good, &tolerance(bad)),
            SurfaceMeasureError::BadTolerance("parallel tolerance")
        );
        let t = SurfaceTolerance {
            flat_degrees: bad,
            ..SurfaceTolerance::default()
        };
        assert_eq!(
            measure(&good, &good, &t),
            SurfaceMeasureError::BadTolerance("flat tolerance")
        );
    }
    assert_eq!(
        SurfaceMeasureError::BadTolerance("parallel tolerance").to_string(),
        "the parallel tolerance must be from 0 to 90 degrees"
    );
}

#[test]
fn a_hit_converts_into_a_pick() {
    let hit = Hit {
        mesh: 2,
        triangle: 7,
        point: [1.0, 2.0, 3.0],
        normal: [0.0, 0.0, 1.0],
        distance: 4.0,
    };
    assert_eq!(SurfacePick::from(&hit), pick(2, 7, [1.0, 2.0, 3.0]));
}

/// A tube of radius `r` round the Z axis from z=0 to z=`h`, `n` facets
/// over `sweep` degrees, as one face.
fn tube(r: f64, h: f64, n: usize, sweep: f64) -> TriangleMesh {
    let full = (sweep - 360.0).abs() < 1e-9;
    let rim = if full { n } else { n + 1 };
    let mut positions = Vec::new();
    for i in 0..rim {
        let a = (sweep * i as f64 / n as f64).to_radians();
        positions.push([r * a.cos(), r * a.sin(), 0.0]);
        positions.push([r * a.cos(), r * a.sin(), h]);
    }
    let mut triangles = Vec::new();
    for i in 0..n {
        let j = (i + 1) % rim;
        let (a, b, c, d) = (
            2 * i as u32,
            2 * i as u32 + 1,
            2 * j as u32,
            2 * j as u32 + 1,
        );
        triangles.push([a, c, d]);
        triangles.push([a, d, b]);
    }
    let count = triangles.len();
    TriangleMesh {
        positions,
        triangles,
        faces: vec![0..count],
        ..TriangleMesh::default()
    }
}

#[test]
fn a_whole_tube_fits_a_cylinder() {
    let meshes = [tube(5.0, 8.0, 64, 360.0)];
    let f = face_figures(&meshes, &pick(0, 3, [0.0; 3]), 10.0).unwrap();
    let FaceShape::Cylinder {
        centre,
        axis,
        radius,
        residual,
        length,
        sweep_degrees,
    } = f.shape
    else {
        panic!("{:?}", f.shape)
    };
    assert!((radius - 5.0).abs() < 1e-9 && residual < 1e-9);
    assert!((axis[2].abs() - 1.0).abs() < 1e-9);
    assert!((length - 8.0).abs() < 1e-9);
    assert!(centre[0].abs() < 1e-9 && centre[1].abs() < 1e-9);
    assert!((centre[2] - 4.0).abs() < 1e-9);
    assert_eq!(sweep_degrees, 360.0);
    // Two rims of 64 chords each.
    let chord = 2.0 * 5.0 * (std::f64::consts::PI / 64.0).sin();
    assert!((f.perimeter - 128.0 * chord).abs() < 1e-9);
    assert_eq!((f.face, f.triangles), (Some(0), 128));
    assert_eq!(f.source, FaceSource::File);
}

#[test]
fn a_half_tube_reports_its_sweep() {
    let meshes = [tube(2.0, 1.0, 32, 180.0)];
    let f = face_figures(&meshes, &pick(0, 0, [0.0; 3]), 10.0).unwrap();
    let FaceShape::Cylinder {
        radius,
        sweep_degrees,
        ..
    } = f.shape
    else {
        panic!("{:?}", f.shape)
    };
    assert!((radius - 2.0).abs() < 1e-9);
    assert!((sweep_degrees - 180.0).abs() < 1e-9);
}

#[test]
fn a_tube_coarser_than_the_tolerance_is_not_called_a_plane() {
    // 8 facets turn 45 degrees each: no plane, and a cylinder only once
    // the tolerance lets a facet lean that far off square.
    let meshes = [tube(1.0, 1.0, 8, 360.0)];
    let f = face_figures(&meshes, &pick(0, 0, [0.0; 3]), 0.5).unwrap();
    assert!(
        matches!(f.shape, FaceShape::Cylinder { .. }),
        "{:?}",
        f.shape
    );
    let skew = [strip(0.0, 0.0), strip(0.0, 30.0)];
    let mut both = skew[0].clone();
    let base = both.positions.len() as u32;
    both.positions
        .extend(skew[1].positions.iter().map(|p| [p[0], p[1] + 9.0, p[2]]));
    both.triangles
        .extend(skew[1].triangles.iter().map(|t| t.map(|i| i + base)));
    let f = face_figures(&[both], &pick(0, 0, [0.0; 3]), 0.5).unwrap();
    assert!(matches!(
        f.shape,
        FaceShape::Cylinder { .. } | FaceShape::Unfitted
    ));
}

/// A unit box whose one file face covers all twelve triangles.
fn one_face_box() -> TriangleMesh {
    let mut positions = Vec::new();
    for i in 0..8_u32 {
        positions.push([f64::from(i & 1), f64::from((i >> 1) & 1), f64::from(i >> 2)]);
    }
    // Each side as two triangles wound outward.
    let sides: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let triangles: Vec<[u32; 3]> = sides
        .iter()
        .flat_map(|&[a, b, c, d]| [[a, b, c], [a, c, d]])
        .collect();
    TriangleMesh {
        positions,
        triangles,
        faces: vec![0..12],
        ..TriangleMesh::default()
    }
}

#[test]
fn a_part_given_as_one_face_measures_the_flat_face_at_the_pick() {
    // Triangle 2 is on the top, z = 1.
    let f = face_figures(&[one_face_box()], &pick(0, 2, [0.5, 0.5, 1.0]), 10.0).unwrap();
    assert_eq!(
        (f.source, f.face, f.triangles),
        (FaceSource::Grown, None, 2)
    );
    assert!((f.area - 1.0).abs() < 1e-12 && (f.perimeter - 4.0).abs() < 1e-12);
    let FaceShape::Plane { normal, .. } = f.shape else {
        panic!("{:?}", f.shape)
    };
    assert_eq!(normal, [0.0, 0.0, 1.0]);
}

#[test]
fn a_closed_part_with_no_faces_also_grows_the_flat_face() {
    let mut part = one_face_box();
    part.faces.clear();
    let f = face_figures(&[part], &pick(0, 2, [0.5, 0.5, 1.0]), 10.0).unwrap();
    assert_eq!(
        (f.source, f.face, f.triangles),
        (FaceSource::Grown, None, 2)
    );
    assert!((f.area - 1.0).abs() < 1e-12 && (f.perimeter - 4.0).abs() < 1e-12);
}

#[test]
fn a_closed_shell_never_reports_a_negative_zero_perimeter() {
    assert_eq!(perimeter(&[]).to_bits(), 0.0_f64.to_bits());
}

#[test]
fn a_plate_face_reports_its_area_and_outline() {
    // Stored as loose triangles: the shared diagonal still welds.
    let mut plate = strip(0.0, 0.0);
    let p = plate.positions.clone();
    plate.positions = vec![p[0], p[1], p[2], p[0], p[2], p[3]];
    plate.triangles = vec![[0, 1, 2], [3, 4, 5]];
    let f = face_figures(&[plate], &pick(0, 1, [0.0; 3]), 0.5).unwrap();
    assert_eq!((f.area, f.perimeter, f.face), (12.0, 14.0, None));
    assert_eq!(f.source, FaceSource::WholeMesh);
    let FaceShape::Plane {
        normal,
        flatness_degrees,
    } = f.shape
    else {
        panic!("{:?}", f.shape)
    };
    assert_eq!((normal, flatness_degrees), ([0.0, 0.0, 1.0], 0.0));
}

#[test]
fn a_face_measure_refuses_a_bad_pick_or_tolerance() {
    let meshes = [strip(0.0, 0.0)];
    assert!(matches!(
        face_figures(&meshes, &pick(0, 9, [0.0; 3]), 0.5),
        Err(SurfaceMeasureError::NoSuchTriangle { .. })
    ));
    assert!(matches!(
        face_figures(&meshes, &pick(0, 0, [0.0; 3]), 91.0),
        Err(SurfaceMeasureError::BadTolerance(_))
    ));
}

#[test]
fn a_tube_rim_is_a_closed_circle() {
    let meshes = [tube(5.0, 8.0, 64, 360.0)];
    let a = (360.0_f64 * 3.5 / 64.0).to_radians();
    let at = [5.0 * a.cos(), 5.0 * a.sin(), 0.01];
    let e = edge_at(&meshes, &pick(0, 6, at), 10.0, 45.0).unwrap();
    assert!(e.closed);
    assert_eq!(e.points.len(), 64);
    assert_eq!(e.faces, [Some(0), None]);
    assert_eq!(e.chord, 0.0);
    let EdgeShape::Circle {
        centre,
        radius,
        sweep_degrees,
        arc_length,
        ..
    } = e.shape
    else {
        panic!("{:?}", e.shape)
    };
    assert!((radius - 5.0).abs() < 1e-9, "{radius}");
    assert!((sweep_degrees - 360.0).abs() < 1e-9);
    assert!((arc_length - 10.0 * std::f64::consts::PI).abs() < 1e-9);
    assert!(centre.iter().all(|c| c.abs() < 1e-9), "{centre:?}");
    assert!(e.length < arc_length && e.length > 0.99 * arc_length);
}

#[test]
fn a_half_tube_has_a_straight_side_and_a_half_circle_rim() {
    let meshes = [tube(5.0, 8.0, 32, 180.0)];
    let side = edge_at(&meshes, &pick(0, 1, [5.0, 0.0, 4.0]), 10.0, 45.0).unwrap();
    assert!(!side.closed);
    assert!((side.length - 8.0).abs() < 1e-12);
    assert!((side.chord - 8.0).abs() < 1e-12);
    let EdgeShape::Line { direction } = side.shape else {
        panic!("{:?}", side.shape)
    };
    assert!((direction[2].abs() - 1.0).abs() < 1e-12);
    let a = (180.0_f64 * 0.5 / 32.0).to_radians();
    let rim = edge_at(
        &meshes,
        &pick(0, 0, [5.0 * a.cos(), 5.0 * a.sin(), 0.0]),
        10.0,
        45.0,
    )
    .unwrap();
    assert_eq!(rim.points.len(), 33);
    assert!((rim.chord - 10.0).abs() < 1e-9);
    let EdgeShape::Circle {
        radius,
        sweep_degrees,
        ..
    } = rim.shape
    else {
        panic!("{:?}", rim.shape)
    };
    assert!((radius - 5.0).abs() < 1e-9);
    assert!((sweep_degrees - 180.0).abs() < 1e-9, "{sweep_degrees}");
}

#[test]
fn a_vertex_is_the_picked_triangles_nearest_corner() {
    let meshes = [tube(5.0, 8.0, 32, 180.0)];
    let v = vertex_at(&meshes, &pick(0, 1, [4.9, 0.1, 7.5])).unwrap();
    assert_eq!(v, [5.0, 0.0, 8.0]);
}

#[test]
fn edges_between_faces_and_where_none_is() {
    let p = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let triangles = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    let one = [TriangleMesh {
        positions: p.clone(),
        triangles: triangles.clone(),
        faces: vec![0..4],
        ..TriangleMesh::default()
    }];
    let at = [0.3, 0.3, 0.0];
    assert_eq!(
        edge_at(&one, &pick(0, 0, at), 10.0, 45.0),
        Err(SurfaceMeasureError::NoEdge)
    );
    let split = [TriangleMesh {
        positions: p.clone(),
        triangles: triangles.clone(),
        faces: vec![0..1, 1..4],
        ..TriangleMesh::default()
    }];
    let e = edge_at(&split, &pick(0, 0, [0.5, 0.01, 0.0]), 10.0, 45.0).unwrap();
    assert_eq!(e.faces, [Some(0), Some(1)]);
    assert!((e.length - 1.0).abs() < 1e-12);
    assert!(matches!(e.shape, EdgeShape::Line { .. }));
    let faceless = [TriangleMesh {
        positions: p,
        triangles,
        ..TriangleMesh::default()
    }];
    let e = edge_at(&faceless, &pick(0, 0, [0.5, 0.01, 0.0]), 10.0, 45.0).unwrap();
    assert_eq!(e.faces, [None, None]);
    assert_eq!(e.points.len(), 2);
    assert_eq!(
        edge_at(&faceless, &pick(0, 0, at), 91.0, 45.0),
        Err(SurfaceMeasureError::BadTolerance("flat tolerance"))
    );
}

/// One face of two triangles over the four corners in order.
fn square(c: [[f64; 3]; 4]) -> TriangleMesh {
    TriangleMesh {
        positions: c.to_vec(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        faces: vec![0..2],
        ..TriangleMesh::default()
    }
}

fn select(mesh: usize, triangle: usize, point: [f64; 3], entity: MeasureEntity) -> Selection {
    Selection {
        pick: pick(mesh, triangle, point),
        entity,
    }
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn two_faces_report_their_nearest_and_farthest_points() {
    let meshes = [
        square([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        square([
            [2.0, 0.0, 3.0],
            [3.0, 0.0, 3.0],
            [3.0, 1.0, 3.0],
            [2.0, 1.0, 3.0],
        ]),
    ];
    let a = select(0, 0, [0.5, 0.2, 0.0], MeasureEntity::Face);
    let b = select(1, 0, [2.5, 0.2, 3.0], MeasureEntity::Face);
    let d = entity_distance(&meshes, &a, &b, 10.0, 45.0).unwrap();
    assert!((d.minimum.length - 10.0_f64.sqrt()).abs() < 1e-12);
    assert!((d.minimum.from[0] - 1.0).abs() < 1e-12 && d.minimum.from[2] == 0.0);
    assert!((d.minimum.to[0] - 2.0).abs() < 1e-12 && d.minimum.to[2] == 3.0);
    assert!((d.minimum.delta()[1]).abs() < 1e-12);
    assert!((d.maximum.length - 19.0_f64.sqrt()).abs() < 1e-12);
    assert_eq!(d.centres, None);
}

#[test]
fn a_vertex_over_a_face_measures_straight_down() {
    let meshes = [
        square([
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
        ]),
        square([
            [1.0, 2.0, 5.0],
            [2.0, 2.0, 5.0],
            [2.0, 3.0, 5.0],
            [1.0, 3.0, 5.0],
        ]),
    ];
    let a = select(1, 0, [1.01, 2.01, 5.0], MeasureEntity::Vertex);
    let b = select(0, 0, [3.0, 1.0, 0.0], MeasureEntity::Face);
    let d = entity_distance(&meshes, &a, &b, 10.0, 45.0).unwrap();
    assert!((d.minimum.length - 5.0).abs() < 1e-12);
    assert!(near(d.minimum.to, [1.0, 2.0, 0.0]), "{:?}", d.minimum);
    assert_eq!(d.centres, None);
}

#[test]
fn two_rims_of_a_tube_measure_centre_to_centre() {
    let meshes = [tube(5.0, 8.0, 64, 360.0)];
    let a = (360.0_f64 * 3.5 / 64.0).to_radians();
    let (x, y) = (5.0 * a.cos(), 5.0 * a.sin());
    let bottom = select(0, 6, [x, y, 0.01], MeasureEntity::Edge);
    let top = select(0, 7, [x, y, 7.99], MeasureEntity::Edge);
    let d = entity_distance(&meshes, &bottom, &top, 10.0, 45.0).unwrap();
    let c = d.centres.unwrap();
    assert!((c.length - 8.0).abs() < 1e-9, "{c:?}");
    assert!(near(c.delta(), [0.0, 0.0, 8.0]) || near(c.delta(), [0.0, 0.0, -8.0]));
    assert!((d.minimum.length - 8.0).abs() < 1e-9);
    assert!((d.maximum.length - 164.0_f64.sqrt()).abs() < 1e-9);
}

#[test]
fn crossing_faces_are_no_distance_apart() {
    let meshes = [
        square([
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ]),
        square([
            [0.0, 0.2, -1.0],
            [0.0, 0.8, -1.0],
            [0.0, 0.8, 1.0],
            [0.0, 0.2, 1.0],
        ]),
    ];
    let a = select(0, 0, [0.5, -0.5, 0.0], MeasureEntity::Face);
    let b = select(1, 0, [0.0, 0.6, -0.5], MeasureEntity::Face);
    let d = entity_distance(&meshes, &a, &b, 10.0, 45.0).unwrap();
    assert!(d.minimum.length < 1e-12, "{:?}", d.minimum);
    assert!(d.minimum.from[2].abs() < 1e-12);
    let missing = select(0, 9, [0.0; 3], MeasureEntity::Face);
    assert!(matches!(
        entity_distance(&meshes, &a, &missing, 10.0, 45.0),
        Err(SurfaceMeasureError::NoSuchTriangle { .. })
    ));
}

#[test]
fn a_span_projects_onto_a_plane_and_keeps_its_normal_part() {
    let meshes = [
        square([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        square([
            [4.0, 0.0, 3.0],
            [5.0, 0.0, 3.0],
            [5.0, 1.0, 3.0],
            [4.0, 1.0, 3.0],
        ]),
    ];
    let a = select(0, 0, [0.01, 0.01, 0.0], MeasureEntity::Vertex);
    let b = select(1, 0, [4.01, 0.01, 3.0], MeasureEntity::Vertex);
    let span = entity_distance(&meshes, &a, &b, 10.0, 45.0)
        .unwrap()
        .minimum;
    assert!((span.length - 5.0).abs() < 1e-12);
    let flat = span.projected([0.0, 0.0, 2.0]).unwrap();
    assert!((flat.length - 4.0).abs() < 1e-12);
    assert!(near(flat.from, span.from) && near(flat.to, [4.0, 0.0, 0.0]));
    assert!((span.along([0.0, 0.0, -7.0]).unwrap() + 3.0).abs() < 1e-12);
    let side = span.projected([1.0, 0.0, 0.0]).unwrap();
    assert!((side.length - 3.0).abs() < 1e-12);
    assert_eq!(span.projected([0.0; 3]), None);
    assert_eq!(span.along([0.0; 3]), None);
}

#[test]
fn a_coordinate_frame_reads_points_and_deltas_in_its_own_axes() {
    // Origin (1, 2, 3), X along model Y, Y along model -X, so Z is model Z.
    let f = CoordinateFrame::new([1.0, 2.0, 3.0], [0.0, 5.0, 0.0], [-3.0, 7.0, 0.0]).unwrap();
    let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12);
    assert!(close(f.axes()[1], [-1.0, 0.0, 0.0]));
    assert!(close(f.axes()[2], [0.0, 0.0, 1.0]));
    assert!(close(f.point_to_local([1.0, 2.0, 3.0]), [0.0; 3]));
    assert!(close(f.point_to_local([0.0, 6.0, 5.0]), [4.0, 1.0, 2.0]));
    // A delta ignores the origin.
    assert!(close(f.vector_to_local([0.0, 6.0, 5.0]), [6.0, 0.0, 5.0]));
    // A tilted frame keeps lengths.
    let g = CoordinateFrame::new([0.0; 3], [1.0, 1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
    let v = g.vector_to_local([3.0, -4.0, 12.0]);
    assert!((crate::vec3::length(v) - 13.0).abs() < 1e-12);
    // Degenerate frames are refused.
    assert!(CoordinateFrame::new([0.0; 3], [0.0; 3], [0.0, 1.0, 0.0]).is_none());
    assert!(CoordinateFrame::new([0.0; 3], [1.0, 0.0, 0.0], [-2.0, 0.0, 0.0]).is_none());
    assert!(CoordinateFrame::new([f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).is_none());
}

#[test]
fn a_projected_area_is_the_faces_shadow_on_each_side() {
    // A 4 x 3 plate tilted 60 degrees: its shadow on XY is half its area.
    let plate = [strip(0.0, 60.0)];
    let up = face_projected_area(&plate, &pick(0, 0, [0.0; 3]), [0.0, 0.0, 7.0]).unwrap();
    assert!((up.toward - 6.0).abs() < 1e-9 && up.away == 0.0, "{up:?}");
    let down = face_projected_area(&plate, &pick(0, 0, [0.0; 3]), [0.0, 0.0, -1.0]).unwrap();
    assert!((down.away - 6.0).abs() < 1e-9 && down.toward == 0.0);
    // A whole tube seen side-on: each side covers the 10 x 8 silhouette.
    let tube = [tube(5.0, 8.0, 64, 360.0)];
    let side = face_projected_area(&tube, &pick(0, 3, [0.0; 3]), [1.0, 0.0, 0.0]).unwrap();
    assert!((side.toward - 80.0).abs() < 1e-9 && (side.away - 80.0).abs() < 1e-9);
    let end = face_projected_area(&tube, &pick(0, 3, [0.0; 3]), [0.0, 0.0, 1.0]).unwrap();
    assert!(end.toward < 1e-9 && end.away < 1e-9);
    for bad in [[0.0; 3], [f64::NAN, 0.0, 1.0]] {
        let e = face_projected_area(&plate, &pick(0, 0, [0.0; 3]), bad);
        assert_eq!(e, Err(SurfaceMeasureError::BadDirection));
    }
    let e = face_projected_area(&plate, &pick(0, 9, [0.0; 3]), [0.0, 0.0, 1.0]);
    assert!(matches!(e, Err(SurfaceMeasureError::NoSuchTriangle { .. })));
}

#[test]
fn a_part_given_as_one_face_measures_distance_from_the_flat_face_at_the_pick() {
    // The box top (z = 1) against a point above it: the whole box would put
    // the farthest corner at the bottom, z = 0.
    let mut meshes = vec![one_face_box()];
    meshes.push(TriangleMesh {
        positions: vec![[0.5, 0.5, 3.0]; 3],
        triangles: vec![[0, 1, 2]],
        ..TriangleMesh::default()
    });
    let top = select(0, 2, [0.5, 0.5, 1.0], MeasureEntity::Face);
    let above = select(1, 0, [0.5, 0.5, 3.0], MeasureEntity::Vertex);
    let d = entity_distance(&meshes, &top, &above, 10.0, 45.0).unwrap();
    assert_eq!((d.a_source, d.b_source), (Some(FaceSource::Grown), None));
    assert!((d.minimum.length - 2.0).abs() < 1e-12, "{:?}", d.minimum);
    assert!(
        (d.maximum.length - 4.5_f64.sqrt()).abs() < 1e-12,
        "{:?}",
        d.maximum
    );
    assert!(d.maximum.from[2] == 1.0, "{:?}", d.maximum);
}
