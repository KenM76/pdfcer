use super::*;

/// Seed `[0 1 2]`, then the apex across `[2 1]`: `X` runs from vertex
/// 2 to vertex 1, and each of `d`'s three components lands on its own
/// axis with its own sign. Tolerance 0.5 halves every step.
#[test]
fn a_seed_and_one_apex_rebuild() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2];
    let m = run(&pts, &[2, 0, 0, 0, 0, 0], 2, &[false; 4], &[]).unwrap();
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3]]);
    let p: Vec<V> = m
        .positions
        .iter()
        .map(|v| [v[0] - 10.0, v[1], v[2]])
        .collect();
    assert!(close(p[0], [0.0, 0.0, 0.0]) && close(p[1], [2.0, 0.0, 0.0]));
    assert!(close(p[2], [0.0, 2.0, 0.0]), "{:?}", p[2]);
    assert!(close(p[3], [1.0 + 2.0 * S, 1.0 + S, -1.0]), "{:?}", p[3]);
}

/// Bit 0 alone continues across `[A C]`, keeping `W = B`.
#[test]
fn the_right_edge_is_entered_from_a_to_c() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -2, 0];
    let m = run(&pts, &[1, 0, 0, 0, 0, 0], 2, &[false; 4], &[]).unwrap();
    assert_eq!(m.triangles[1], [0, 2, 3]);
}

/// A reference slot reuses a vertex; a closed pushed edge is skipped
/// and the next component starts from a fresh seed.
#[test]
fn references_and_components() {
    let pts = [0; 9 + 9];
    let is_ref = [false, false, false, true, false, false, false];
    let m = run(&pts, &[3, 0, 0, 0, 0, 0, 0, 0, 0], 3, &is_ref, &[0]).unwrap();
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 0], [3, 4, 5]]);
}

/// A double-sided panel: four coplanar vertices, four triangles. The
/// second triangle's apex folds back onto the seed's side, so its own
/// continuation takes `[A C]` first; the last two apexes are the seed's
/// first two vertices.
#[test]
fn a_folded_apex_swaps_its_continuation() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 1, 3, 0];
    let is_ref = [false, false, false, false, true, true];
    let m = run(
        &pts,
        &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        4,
        &is_ref,
        &[0, 1],
    )
    .unwrap();
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3], [2, 3, 0], [0, 3, 1]]);
    // The same panel unfolded is rescued by the component retry.
    let unfolded = [0, 0, 0, 4, 0, 0, -2, 4, 0, 1, -3, 0];
    let m = run(
        &unfolded,
        &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        4,
        &is_ref,
        &[0, 1],
    )
    .unwrap();
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3], [2, 3, 0], [0, 3, 1]]);
}

#[test]
fn a_fold_past_the_retry_point_is_detected() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -3, 1, 1, 3, 0];
    let is_ref = [false, false, false, false, false, true];
    let m = run(
        &pts,
        &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        4,
        &is_ref,
        &[0],
    )
    .unwrap();
    assert_eq!(m.triangles[3], [3, 4, 0]);
}

/// A panel whose fold apex is a reference carries no fold signal: the
/// unfolded walk closes an edge it still needs, so the component is
/// rolled back and retried folded at its second triangle.
#[test]
fn a_reference_fold_is_found_by_retrying_the_component() {
    let pts: Vec<i64> = (1..=30).collect();
    let mut is_ref = vec![false; 9];
    is_ref.extend([true, true, false, true, true, true]);
    let mut status = vec![0; 21];
    status[3..7].copy_from_slice(&[3, 3, 2, 0]);
    let m = run(&pts, &status, 7, &is_ref, &[0, 3, 6, 0, 3]).unwrap();
    assert_eq!(
        m.triangles[3..],
        [[0, 3, 9], [9, 3, 6], [9, 6, 0], [0, 6, 3]]
    );
}

/// A record read at a triangle orients it exactly: the third
/// triangle's fresh record at vertex 4 has `reversed` clear while its
/// winding runs `max` to `min`, so it folds [WD 7.8.9.1]; vertex 3's
/// set bit, read at `[2 1 3]`, keeps the second unfolded.
#[test]
fn a_reversed_bit_orients_the_walk() {
    let mut bits = Vec::new();
    for rev in [0, 0, 0, 1, 0] {
        bits.extend([0, rev, 0, 0]);
    }
    let angles: Vec<i32> = [0, 1023].repeat(5);
    let m = oriented(&bits, &angles, false);
    assert_eq!(m.triangles, FOLDED);
    assert_eq!(m.normals.len(), 5);
    // Without the normals the default rule leaves it unfolded.
    let plain = run(
        &[0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -3, 1, 1, 3, 1],
        &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        4,
        &[false, false, false, false, false, true],
        &[0],
    )
    .unwrap();
    assert_eq!(plain.triangles[3], [4, 1, 0]);
}

/// On a planar face only the seed reads a record; the third triangle
/// winds against that face normal, so it folds [WD 7.8.9].
#[test]
fn a_planar_face_normal_orients_the_walk() {
    let m = oriented(&[0, 0, 0, 0], &[0, 1023], true);
    assert_eq!(m.triangles, FOLDED);
    assert_eq!(m.triangle_normals, [[0; 3]; 4]);
}

/// Normals the oriented walk cannot use up send the mesh back to the
/// default rule, which keeps its triangles and drops the normals.
#[test]
fn normals_that_do_not_fit_fall_back_to_the_default_fold() {
    let m = oriented(&[0, 0, 0, 0, 1], &[0, 1023], true);
    assert_eq!(m.triangles[3], [4, 1, 0]);
    assert!(m.normals.is_empty());
}

/// One planar face. The third triangle's apex `(4, 0, 0)` lies on its
/// entry edge's line, a sliver its face normal cannot orient, so it
/// takes the default (unfolded). The fourth triangle's reference then
/// reuses an edge a third time, and inverting the second triangle does
/// not help: only the sliver, folded, fits.
#[test]
fn a_sliver_fold_is_found_by_retrying_at_the_sliver() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, 3, 0, 4, 0, 0, 0, 3, 0];
    let mut status = [0; 15];
    status[..3].copy_from_slice(&[1, 1, 3]);
    let mut is_ref = [false; 7];
    is_ref[5] = true;
    let m = reconstruct(&Arrays {
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
            angles: &[0, 1023],
            planar: &[true],
            face_of: &[0; 5],
        }),
    })
    .unwrap();
    assert_eq!(
        m.triangles,
        [[0, 1, 2], [0, 2, 3], [3, 2, 4], [3, 4, 0], [4, 2, 5]]
    );
    assert_eq!(m.normals.len(), 1);
}

/// Corners whose normals were all read at an earlier triangle still
/// orient it [WD 7.8.9]; one at most a tolerance thick, or a legacy
/// walk, keeps the default fold, as does a thin one's fresh record.
#[test]
fn reused_normals_orient_unless_thin_or_legacy() {
    let na = NormalArrays {
        bits: 10,
        binary: &[false; 16],
        angles: &[0, 1023, 0, 1023, 0, 1023, 0, 1023],
        planar: &[false],
        face_of: &[0; 8],
    };
    let mut w = Walk::new(3, Some(NormalReader::new(&na)));
    w.pos = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
    assert!(!w.fold([0, 1, 2], false, false).unwrap());
    let both = |w: &mut Walk<'_>, legacy| {
        [[0, 1, 2], [0, 2, 1]].map(|t| w.fold(t, false, legacy).unwrap())
    };
    assert_eq!(both(&mut w, false), [false, true]);
    assert_eq!(both(&mut w, true), [false, false]);
    w.thin = true;
    assert_eq!(both(&mut w, false), [false, false]);
    // A fresh record's reversed bit, which would fold it, too.
    assert!(!w.fold([1, 0, 3], false, false).unwrap());
}

/// A fresh record whose normal lies in its triangle's plane carries a
/// `triangle_normal_reversed` bit the encoder set from a rounding
/// residue ([`BLIND_NORMAL`]): the default fold stands, unsignalled, and
/// the triangle is marked for the sliver retry. One whose normal
/// leaves the plane decides as before.
#[test]
fn a_record_normal_in_the_plane_is_blind() {
    let na = NormalArrays {
        bits: 10,
        binary: &[false; 16],
        angles: &[0, 0, 0, 0, 0, 0, 0, 1023],
        planar: &[false],
        face_of: &[0; 8],
    };
    let mut w = Walk::new(3, Some(NormalReader::new(&na)));
    w.pos = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
    assert!(!w.fold([0, 1, 2], false, false).unwrap());
    assert!(!w.signalled);
    assert_eq!(w.sliver, Some(0));
    w.sliver = None;
    assert!(w.fold([1, 0, 3], false, false).unwrap());
    assert!(w.signalled);
    assert_eq!(w.sliver, None);
}

#[test]
fn arrays_that_do_not_fit_are_refused() {
    let pts = [0; 9];
    let ok = [false; 3];
    assert!(run(&pts, &[0, 0, 0], 1, &ok, &[]).is_ok());
    assert!(run(&pts, &[0], 1, &ok, &[]).is_err(), "T form");
    assert!(
        run(&[0; 12], &[0, 0, 0], 1, &ok, &[]).is_err(),
        "points left"
    );
    assert!(
        run(&[0; 6], &[0, 0, 0], 1, &ok, &[]).is_err(),
        "points short"
    );
    assert!(
        run(&pts, &[0, 0, 0], 1, &[false; 4], &[]).is_err(),
        "slots left"
    );
    let fwd = [false, false, true];
    assert!(
        run(&[0; 6], &[0, 0, 0], 1, &fwd, &[7]).is_err(),
        "forward ref"
    );
}

/// `PrcPt::Length` rounds the squared length to `f32` and takes
/// Newton's root from a halved exponent, which can settle an ulp off
/// the correctly rounded root; `Unitize` refuses lengths below
/// `FLT_EPSILON` [WD 12.3].
#[test]
fn length_and_unitize_follow_the_wd_pseudocode() {
    assert_eq!(wd_length([3.0, 4.0, 0.0]), Some(5.0));
    let newton = f64::from_bits(S.to_bits() - 1);
    assert_eq!(wd_length([1.0, 1.0, 0.0]), Some(newton));
    let long = 1.0 + 2f64.powi(-30);
    assert_eq!(wd_length([long, 0.0, 0.0]), Some(1.0));
    assert_eq!(unitize([0.0, long, 0.0]), Some([0.0, long, 0.0]));
    assert_eq!(unitize([1e-8, 0.0, 0.0]), None);
    assert_eq!(unitize([0.0; 3]), None);
}

/// A `W` on the edge's line leaves `Z` null, so the frame comes from
/// `MakeOrthoRep(X)`: `Z` from `X x (0, 1, 0)`, or from `X x (1, 0, 0)`
/// when `X` is along `Y` [WD 7.8.9.2, 12.3].
#[test]
fn a_null_z_takes_the_ortho_rep_of_x() {
    let d = [1.0, 2.0, 3.0];
    let (p, _) = apex([0.0; 3], [-2.0, 0.0, 0.0], [5.0, 0.0, 0.0], d, false, false);
    assert_eq!(p, [0.0, -2.0, -3.0]);
    let (p, _) = apex([0.0; 3], [0.0, -2.0, 0.0], [0.0, 5.0, 0.0], d, false, false);
    assert_eq!(p, [-2.0, 0.0, 3.0]);
}

/// A degenerate frame's `Y` comes from the frame's own `X`, whose
/// length `unitize` left off by a few 1e-8, not from the copy
/// `MakeOrthoRep` unitized again: the two `Y`s differ by that much, and
/// the walk turns such a difference into a wrong fold (m23 and m39 of
/// the door sample fit with the former only).
#[test]
fn a_degenerate_frame_takes_y_from_its_own_x() {
    let (lo, hi) = ([0.1, 0.2, 0.3], [0.7, -0.4, 0.9]);
    let o = scale(add(lo, hi), 0.5);
    let x = unitize(sub(lo, hi)).unwrap();
    let w = add(o, scale(x, 0.37));
    let d = [1.0, 2.0, 3.0];
    let z = make_ortho_rep_z(x).unwrap();
    let (p, degenerate) = apex(lo, hi, w, d, false, false);
    assert!(degenerate);
    let own = unitize(cross(z, x)).unwrap();
    let again = unitize(cross(z, unitize(x).unwrap())).unwrap();
    assert_ne!(own, again, "the probe must tell the two Ys apart");
    let sign = if dot(cross(sub(w, o), x), z) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let expect = |y: V| {
        sub(
            sub(add(o, scale(x, d[0])), scale(y, sign * d[1])),
            scale(z, sign * d[2]),
        )
    };
    assert_eq!(p, expect(own));
    assert_ne!(p, expect(again));
}

/// A strip of thin, non-planar triangles compressed by an encoder
/// written out from the WD: each point is expressed in the frame of the
/// working mesh, rounded to the tolerance and reinjected [WD 7.8.9,
/// 7.8.9.2], with `PrcPt::Length` and `Unitize` transcribed from the
/// pseudocode [WD 12.3]. The decoder must land on that working mesh bit
/// for bit, which keeps every vertex within the rounding bound of its
/// source.
#[test]
fn a_strip_decodes_onto_the_encoders_working_mesh() {
    fn length(v: V) -> f64 {
        let f = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f32;
        let mut bits = f.to_bits();
        let exponent = (bits >> 23) & 0xff;
        if exponent > 127 {
            bits = (bits & 0x807f_ffff) | (((exponent - 127) / 2 + 127) << 23);
        }
        let (squared, mut x0) = (f64::from(f), f64::from(f32::from_bits(bits)));
        for _ in 0..100 {
            let xi = 0.5 * (x0 + squared / x0);
            if xi == x0 {
                return xi;
            }
            x0 = xi;
        }
        -1.0
    }
    fn unitized(v: V) -> V {
        let l = length(v);
        assert!(l >= f64::from(f32::EPSILON), "null axis");
        [v[0] / l, v[1] / l, v[2] / l]
    }
    let (tol, n) = (1e-4, 48);
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut jitter = |scale: f64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * scale
    };
    // Two rails 4 apart, 0.05 between a rail's points, 0.02 of relief.
    let source: Vec<V> = (0..n)
        .map(|i| {
            let (along, rail) = (i as f64 * 0.025, (i % 2) as f64 * 4.0);
            [along + jitter(0.01), rail + jitter(0.01), jitter(0.02)]
        })
        .collect();
    let round = |v: f64| (v / tol).round() as i64;
    let (mut points, mut work) = (Vec::new(), Vec::<V>::new());
    for (i, target) in source.iter().take(3).enumerate() {
        let base = match i {
            0 => [0.0; 3],
            1 => work[0],
            _ => scale(add(work[0], work[1]), 0.5),
        };
        let d = [0, 1, 2].map(|c| round(target[c] - base[c]));
        points.extend(d);
        work.push(add(base, d.map(|c| c as f64 * tol)));
    }
    // Triangle k enters across vertices k and k + 1, opposite k - 1,
    // and continues right on odd k, left on even k, unless it folds.
    let mut status = vec![0; 3 * (n - 2)];
    status[0] = 2;
    for k in 1..n - 2 {
        let (lo, hi, w) = (work[k], work[k + 1], work[k - 1]);
        let o = scale(add(hi, lo), 0.5);
        let x = unitized(sub(lo, hi));
        let z = unitized(cross(sub(w, o), x));
        let y = unitized(cross(z, x));
        let r = sub(source[k + 2], o);
        let d = [round(dot(r, x)), round(-dot(r, y)), round(-dot(r, z))];
        points.extend(d);
        let [dx, dy, dz] = d.map(|c| c as f64 * tol);
        work.push(sub(sub(add(o, scale(x, dx)), scale(y, dy)), scale(z, dz)));
        if k < n - 3 {
            let fold = d[2] == 0 && d[1] > 0;
            status[k] = if (k % 2 == 1) != fold { 1 } else { 2 };
        }
    }
    let m = reconstruct(&Arrays {
        tolerance: tol,
        origin: [0.0; 3],
        points: &points,
        edge_status: &status,
        triangles: n - 2,
        is_reference: &vec![false; n],
        references: &[],
        ortho_turned: false,
        legacy_orient: false,
        normals: None,
    })
    .unwrap();
    assert_eq!(m.triangles[1..4], [[2, 1, 3], [2, 3, 4], [4, 3, 5]]);
    assert_eq!(m.positions, work);
    for (p, s) in m.positions.iter().zip(&source) {
        let off = dot(sub(*p, *s), sub(*p, *s)).sqrt() / tol;
        assert!(off < 0.87, "{off} tolerances off");
    }
}

/// A tetrahedron closed by two references: after `[3 1 0]` the
/// continuation edge `[0 1]` is already shared twice, so the walk
/// takes the open `[3 0]` instead.
#[test]
fn a_closed_continuation_edge_gives_way_to_the_open_one() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, 0, 4];
    let mut status = [0; 12];
    status[..3].copy_from_slice(&[2, 2, 2]);
    let mut is_ref = [false; 6];
    is_ref[4..].copy_from_slice(&[true, true]);
    let m = run(&pts, &status, 4, &is_ref, &[0, 2]).unwrap();
    assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3], [3, 1, 0], [3, 0, 2]]);
}

/// Two triangles of one planar face folded about 70 degrees along their
/// shared edge, far enough that no plane holds all four corners within
/// `MAX_BEND` tolerances: the arrays fit, the face does not.
#[test]
fn a_planar_face_that_bends_is_refused() {
    let bent = [0, 0, 0, 80, 0, 0, -40, 80, 0, 0, -80, 240];
    assert!(one_face(&bent, true).is_err());
    assert!(one_face(&[0, 0, 0, 80, 0, 0, -40, 80, 0, 0, -80, 0], true).is_ok());
}

/// No retry fits: the unsignalled reference triangles need folds the
/// retries do not try. The search finds exactly one fit.
#[test]
fn a_mesh_no_retry_fits_is_found_by_the_search() {
    let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 1, 2, 3, 2, 3, 5, 3, 4, 7];
    let mut status = [0; 18];
    status[..6].copy_from_slice(&[3, 0, 1, 3, 0, 0]);
    let mut is_ref = [false; 8];
    is_ref[5] = true;
    is_ref[7] = true;
    let a = Arrays {
        tolerance: 0.5,
        origin: [10.0, 0.0, 0.0],
        points: &pts,
        edge_status: &status,
        triangles: 6,
        is_reference: &is_ref,
        references: &[3, 2],
        ortho_turned: false,
        legacy_orient: false,
        normals: Some(NormalArrays {
            bits: 10,
            binary: &[false; 24],
            angles: &[0; 12],
            planar: &[false],
            face_of: &[0; 6],
        }),
    };
    assert!(walk(&a, None).is_err());
    let m = reconstruct(&a).unwrap();
    assert_eq!(
        m.triangles,
        [
            [0, 1, 2],
            [2, 1, 3],
            [0, 2, 4],
            [0, 4, 3],
            [0, 3, 5],
            [3, 4, 2]
        ]
    );
}

/// An apex whose `W` lies on the edge's line takes its frame from
/// `MakeOrthoRep`. In each mesh the WD's frame folds a later triangle
/// the wrong way, and only the frame turned a half-turn about `X`
/// rebuilds it: oriented by the stored normals after the plain walk
/// fits, or found by the search after it fails.
#[test]
fn a_frame_only_turned_fits_is_rebuilt_turned() {
    let tri0 = [0, 0, 0, 4, 0, 0, 2, 4, 0];
    let st = [1, 1, 3, 3, 3, 2, 3, 1, 3, 2, 3, 0];
    let pts = [&tri0[..], &[0, 0, 0, 3, 2, -1]].concat();
    with_turned_frame(&pts, &st, 6, &[2], &[414, 420], true, oriented_fit);
    let st = [1, 3, 1, 1, 1, 3, 0, 0, 1, 3, 2, 0];
    let pts = [&tri0[..], &[0, 0, 0, -4, -4, 2]].concat();
    with_turned_frame(&pts, &st, 6, &[2], &[470, 724], false, search::unique_fit);
    let st = [1, 3, 2, 1, 2, 2, 2, 0, 1, 1, 2, 0, 2, 3, 3];
    let pts = [&tri0[..], &[-4, 0, 0, 3, 3, 3, -1, 0, 0]].concat();
    with_turned_frame(&pts, &st, 7, &[0], &[837, 402], false, search::unique_fit);
    let d = [1.0, 2.0, 3.0];
    let (p, degenerate) = apex([0.0; 3], [-2.0, 0.0, 0.0], [5.0, 0.0, 0.0], d, true, false);
    assert_eq!((p, degenerate), ([0.0, 2.0, 3.0], true));
}

/// A degenerate frame follows the sign of `(W - O) x X`'s sub-epsilon
/// residue against `MakeOrthoRep`'s `Z`, whatever the variant;
/// `invert` reverses it.
#[test]
fn a_degenerate_frame_follows_its_residue() {
    let (lo, hi, d) = ([0.0; 3], [-2.0, 0.0, 0.0], [1.0, 2.0, 3.0]);
    let against = [5.0, 1e-9, 0.0];
    let along = [5.0, -1e-9, 0.0];
    assert_eq!(
        apex(lo, hi, against, d, false, false),
        ([0.0, 2.0, 3.0], true)
    );
    assert_eq!(
        apex(lo, hi, along, d, true, false),
        ([0.0, -2.0, -3.0], true)
    );
    assert_eq!(apex(lo, hi, along, d, true, true), ([0.0, 2.0, 3.0], true));
}

/// A mesh whose degenerate apex's residue turns its frame the wrong
/// way for any fold-only search: the search's turn choice finds the
/// one fit.
#[test]
fn a_turn_only_the_search_tries_is_rebuilt() {
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
    assert!(!search::fits_without_turns(&a));
    assert!(search::unique_fit(&a).is_some());
    // With the fourth point moved, every degenerate apex the search
    // meets keeps a residue: it still fits, no walk reads the turn, and
    // the turned reading is skipped.
    let mut moved = pts.clone();
    moved[9] = 1;
    let moved = Arrays {
        points: &moved,
        ..a
    };
    let (fit, reads) = search::unique_fit_reading(&moved);
    assert!(fit.is_some() && !reads);
}
