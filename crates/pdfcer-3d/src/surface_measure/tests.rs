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
