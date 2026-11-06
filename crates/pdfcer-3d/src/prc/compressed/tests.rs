//! Unit tests for `compressed`: shared fixtures here, the cases in `tests/`.

use super::*;

const S: f64 = std::f64::consts::SQRT_2;

fn run(
    points: &[i64],
    status: &[i32],
    t: usize,
    is_ref: &[bool],
    refs: &[u32],
) -> Result<TriangleMesh, String> {
    reconstruct(&Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points,
        edge_status: status,
        triangles: t,
        is_reference: is_ref,
        references: refs,
        ortho_turned: false,
        legacy_orient: false,
        normals: None,
    })
}

fn close(a: V, b: V) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

/// Four triangles, the third's apex `(1, 3, 1)` off its parent's plane
/// so the default rule sees no fold; `bits`/`angles` are the stored
/// normals, `planar` the one face's flag.
fn oriented(bits: &[u8], angles: &[i32], planar: bool) -> TriangleMesh {
    let binary: Vec<bool> = bits.iter().map(|&b| b == 1).collect();
    let pts = [
        0,
        0,
        0,
        4,
        0,
        0,
        -2,
        4,
        0,
        0,
        -3,
        i64::from(!planar),
        1,
        3,
        1,
    ];
    let mut is_ref = [false; 6];
    is_ref[5] = true;
    reconstruct(&Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: &pts,
        edge_status: &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        triangles: 4,
        is_reference: &is_ref,
        references: &[0],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &binary,
            angles,
            planar: &[planar],
            face_of: &[0; 4],
        }),
    })
    .unwrap()
}

const FOLDED: [[u32; 3]; 4] = [[0, 1, 2], [2, 1, 3], [3, 1, 4], [3, 4, 0]];

fn one_face(pts: &[i64], planar: bool) -> Result<TriangleMesh, String> {
    let mut status = [0; 6];
    status[0] = 2;
    reconstruct(&Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: pts,
        edge_status: &status,
        triangles: 2,
        is_reference: &[false; 4],
        references: &[],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[false; 4],
            angles: &[0, 1023],
            planar: &[planar],
            face_of: &[0; 2],
        }),
    })
}

/// Rebuilds a one-face mesh whose sixth slot is its one reference, with
/// the WD frame and with the frame turned; the WD frame must not orient
/// it to the end.
fn with_turned_frame(
    pts: &[i64],
    status: &[i32],
    slots: usize,
    refs: &[u32],
    angles: &[i32],
    walks: bool,
    expected: fn(&Arrays<'_>) -> Option<TriangleMesh>,
) {
    let t = status.len() / 3;
    let mut is_ref = vec![false; slots];
    is_ref[5] = true;
    let a = Arrays {
        tolerance: 0.5,
        // An origin that keeps the arithmetic exact, so the degenerate
        // apex's residue is zero and the variant decides its turn.
        origin: [0.0, 0.0, 0.0],
        points: pts,
        edge_status: status,
        triangles: t,
        is_reference: &is_ref,
        references: refs,
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[false; 4],
            angles,
            planar: &[true],
            face_of: &vec![0; t],
        }),
    };
    assert!(oriented_fit(&a).is_none());
    assert_eq!(walk(&a, None).is_ok(), walks);
    // The search met the apex the turn moves, so the turned reading runs.
    assert!(walks || search::unique_fit_reading(&a).1);
    let want = expected(&Arrays {
        ortho_turned: true,
        ..a
    })
    .unwrap();
    let m = reconstruct(&a).unwrap();
    assert_eq!(m.triangles, want.triangles);
    assert_eq!(m.positions, want.positions);
}

mod fit_search;
mod walk;
