use super::*;

/// Where only one geometry fits, the best-fit search returns it; where
/// none does, it returns nothing rather than a partial mesh.
#[test]
fn a_best_fit_is_the_unique_fit_when_one_exists() {
    let tri0 = [0, 0, 0, 4, 0, 0, 2, 4, 0];
    let st = [1, 3, 1, 1, 1, 3, 0, 0, 1, 3, 2, 0];
    let pts = [&tri0[..], &[0, 0, 0, -4, -4, 2]].concat();
    let mut is_ref = vec![false; 6];
    is_ref[5] = true;
    let a = Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: &pts,
        edge_status: &st,
        triangles: 4,
        is_reference: &is_ref,
        references: &[2],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[false; 4],
            angles: &[470, 724],
            planar: &[true],
            face_of: &[0; 4],
        }),
    };
    let best = search::best_fit(&a);
    assert!(best.is_some());
    assert_eq!(best, search::unique_fit(&a));
    let extra = [&pts[..], &[1, 1, 1]].concat();
    let left_over = Arrays {
        points: &extra,
        ..a
    };
    assert!(search::best_fit(&left_over).is_none(), "points left");
}

/// The search is bounded by its depth: the one-turn mesh above has no
/// fit within one inverted choice and one within two,
/// and the last stage searches deeper than the first two.
#[test]
fn the_search_depth_bounds_its_choices() {
    let tri0 = [0, 0, 0, 4, 0, 0, 2, 4, 0];
    let st = [1, 3, 1, 1, 1, 3, 0, 0, 1, 3, 2, 0];
    let pts = [&tri0[..], &[0, 0, 0, -4, -4, 2]].concat();
    let mut is_ref = vec![false; 6];
    is_ref[5] = true;
    let a = Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: &pts,
        edge_status: &st,
        triangles: 4,
        is_reference: &is_ref,
        references: &[2],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[false; 4],
            angles: &[470, 724],
            planar: &[true],
            face_of: &[0; 4],
        }),
    };
    assert!(!search::fits_within(&a, 1));
    assert!(search::fits_within(&a, 2));
    let [.., (_, shallow), (turns, deep)] = search::STAGES;
    assert!(turns && deep > shallow);
}

/// A sliver, at most a tolerance from collinear, whose stored
/// decision orients its fold the wrong way: the search still tries
/// inverting it, and that is the only fit.
#[test]
fn a_signalled_sliver_is_a_search_candidate() {
    let pts = [0, 0, 0, 4, 0, 0, 2, 4, 0, -4, -1, 1, -1, -1, 0, 4, 0, -3];
    let status = [1, 3, 3, 1, 2, 1, 1, 0, 1, 2, 2, 1, 3, 1, 3];
    let mut is_ref = [false; 7];
    is_ref[5] = true;
    let a = Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: &pts,
        edge_status: &status,
        triangles: 5,
        is_reference: &is_ref,
        references: &[0],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[false; 4],
            angles: &[60, 476],
            planar: &[true],
            face_of: &[0; 5],
        }),
    };
    assert!(walk(&a, None).is_err());
    let m = reconstruct(&a).unwrap();
    assert_eq!(
        m.triangles,
        [[0, 1, 2], [0, 2, 3], [3, 2, 4], [3, 4, 0], [0, 4, 5]]
    );
}

/// A mesh whose `MakeOrthoRep` frames fit, after one inverted fold, the
/// WD's way round: the search tries the WD frame before the turned one,
/// and the turned one fits nothing else.
#[test]
fn the_search_tries_the_wd_frame_first() {
    let pts = [0, 0, 0, 4, 0, 0, 2, 4, 0, 1, 0, 0, -3, 1, -1, -2, 3, 1];
    let status = [3, 3, 3, 2, 2, 1, 1, 2, 3, 3, 1, 3, 1, 1, 0];
    let mut is_ref = [false; 7];
    is_ref[5] = true;
    let a = Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: &pts,
        edge_status: &status,
        triangles: 5,
        is_reference: &is_ref,
        references: &[2],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[true, false, false, true],
            angles: &[302, 340],
            planar: &[true],
            face_of: &[0; 5],
        }),
    };
    assert!(walk(&a, None).is_err());
    let turned = Arrays {
        ortho_turned: true,
        ..a
    };
    let wd = [[0, 1, 2], [2, 1, 3], [3, 1, 4], [3, 4, 2], [2, 4, 5]];
    let other = search::unique_fit(&turned).map(|m| m.triangles);
    assert!(other.is_none() || other == Some(wd.to_vec()));
    assert_eq!(reconstruct(&a).unwrap().triangles, wd);
}

/// A planar face's normal tilted within its own quantum cannot orient
/// a triangle that is tall in space but nearly flat along the face:
/// the fourth triangle's apex frame leans out of the face after the
/// sliver before it, so its in-face height is a twelfth of a tolerance
/// and the sign of its winding against the normal is noise. It takes
/// the default fold and the mesh matches the untilted control
/// ([`MIN_PLANE_HEIGHT`]); oriented by the tilted normal it would
/// close a different fifth triangle, `[3, 0, 5]`.
#[test]
fn a_triangle_flat_along_its_face_is_a_sliver() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -3, 0, 1, 3, 1, 0, 3, 1];
    let status = [3, 3, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut is_ref = [false; 7];
    is_ref[5] = true;
    let mesh = |angles: &[i32]| {
        reconstruct(&Arrays {
            tolerance: 0.5,
            origin: [10.0, 0.0, 0.0],
            points: &pts,
            edge_status: &status,
            triangles: 5,
            is_reference: &is_ref,
            references: &[0],
            ortho_turned: false,
            legacy_orient: false,
            normals: Some(NormalArrays {
                bits: 10,
                binary: &[false; 4],
                angles,
                planar: &[true],
                face_of: &[0; 5],
            }),
        })
        .unwrap()
        .triangles
    };
    let expected = [[0, 1, 2], [2, 1, 3], [3, 1, 4], [3, 4, 0], [0, 4, 5]];
    assert_eq!(mesh(&[341, 909]), expected);
    assert_eq!(mesh(&[0, 1023]), expected);
}
