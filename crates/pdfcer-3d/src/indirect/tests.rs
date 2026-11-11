#![allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests index fixed fixtures.

use super::*;
use crate::lighting::shade::Local;

/// Few probes, so a debug test run stays quick.
const TEST_PROBES: usize = 512;

/// Per-mesh albedo; everything opaque except meshes listed in `holes`.
struct Paint {
    albedo: Vec<[f64; 3]>,
    holes: Vec<usize>,
}

impl Surfaces for Paint {
    fn albedo(&self, hit: &Hit, _: [f64; 3]) -> [f64; 3] {
        self.albedo.get(hit.mesh).copied().unwrap_or([0.0; 3])
    }
    fn passes(&self, hit: &Hit, _: [f64; 3]) -> f64 {
        if self.holes.contains(&hit.mesh) {
            1.0
        } else {
            0.0
        }
    }
}

fn quad(corners: [[f64; 3]; 4]) -> TriangleMesh {
    TriangleMesh {
        positions: corners.to_vec(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        ..TriangleMesh::default()
    }
}

/// A closed box (shared corners, so it welds watertight).
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> TriangleMesh {
    let p = |i: usize| {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    let faces = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    TriangleMesh {
        positions: (0..8).map(p).collect(),
        triangles: faces
            .iter()
            .flat_map(|f| [[f[0], f[1], f[2]], [f[0], f[2], f[3]]])
            .collect(),
        ..TriangleMesh::default()
    }
}

fn point_lamp(position: [f64; 3], colour: [f64; 3]) -> Lamp {
    Lamp::Near(Local {
        position,
        colour,
        attenuation: [1.0, 0.0, 0.0],
        cone: None,
        reach: f64::INFINITY,
        model: 0,
        slot: None,
    })
}

/// A unit Cornell box, open at the front (z = 1): red wall at x = 0, green
/// at x = 1, the rest white, a lamp under the ceiling.
fn cornell() -> (Vec<TriangleMesh>, Vec<[f64; 3]>) {
    let meshes = vec![
        quad([[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]]),
        quad([[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]]),
        quad([[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]]),
        quad([[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]]),
        quad([[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]]),
    ];
    let w = [0.8; 3];
    (meshes, vec![w, w, w, [0.8, 0.05, 0.05], [0.05, 0.8, 0.05]])
}

fn built(meshes: &[TriangleMesh], paint: &Paint, lamps: &[Lamp], threads: usize) -> Volume {
    let index = PickIndex::new(meshes);
    let scene = Scene {
        meshes,
        index: &index,
        surfaces: paint,
        lamps,
        sky: [0.0; 3],
    };
    let mut v = Volume::with_ceiling(meshes, TEST_PROBES).unwrap();
    v.build(&scene, threads);
    v
}

const UP: [f64; 3] = [0.0, 1.0, 0.0];

#[test]
fn walls_bleed_their_colour_onto_the_floor_beside_them() {
    let (meshes, albedo) = cornell();
    let paint = Paint {
        albedo,
        holes: vec![],
    };
    let v = built(&meshes, &paint, &[point_lamp([0.5, 0.9, 0.5], [1.0; 3])], 1);
    assert!(v.is_done() && v.bounces() == BOUNCES);
    let by_red = v.irradiance([0.1, 0.0, 0.5], UP);
    let by_green = v.irradiance([0.9, 0.0, 0.5], UP);
    assert!(by_red[0] > 1.3 * by_red[1], "{by_red:?}");
    assert!(by_green[1] > 1.3 * by_green[0], "{by_green:?}");
    assert!(by_red[2] > 0.0 && by_red[2] < 1.0, "{by_red:?}");
}

#[test]
fn black_surfaces_bounce_nothing() {
    let (meshes, _) = cornell();
    let paint = Paint {
        albedo: vec![[0.0; 3]; 5],
        holes: vec![],
    };
    let v = built(&meshes, &paint, &[point_lamp([0.5, 0.9, 0.5], [1.0; 3])], 1);
    for p in [[0.1, 0.0, 0.5], [0.5, 0.5, 0.0], [0.5, 1.0, 0.5]] {
        assert_eq!(v.irradiance(p, UP), [0.0; 3]);
    }
}

#[test]
fn bounce_is_linear_in_lamp_strength() {
    let (meshes, albedo) = cornell();
    let paint = Paint {
        albedo,
        holes: vec![],
    };
    let one = built(&meshes, &paint, &[point_lamp([0.5, 0.9, 0.5], [0.5; 3])], 1);
    let two = built(&meshes, &paint, &[point_lamp([0.5, 0.9, 0.5], [1.0; 3])], 1);
    for p in [[0.1, 0.0, 0.5], [0.9, 0.3, 0.2], [0.5, 0.0, 0.9]] {
        let (a, b) = (one.irradiance(p, UP), two.irradiance(p, UP));
        assert_eq!(a.map(|v| v * 2.0), b, "at {p:?}");
    }
}

#[test]
fn thread_count_and_step_budget_do_not_change_the_result() {
    let (meshes, albedo) = cornell();
    let paint = Paint {
        albedo,
        holes: vec![],
    };
    let lamps = [point_lamp([0.5, 0.9, 0.5], [1.0; 3])];
    let serial = built(&meshes, &paint, &lamps, 1);
    assert_eq!(built(&meshes, &paint, &lamps, 4), serial);
    let index = PickIndex::new(&meshes);
    let scene = Scene {
        meshes: &meshes,
        index: &index,
        surfaces: &paint,
        lamps: &lamps,
        sky: [0.0; 3],
    };
    let mut stepped = Volume::with_ceiling(&meshes, TEST_PROBES).unwrap();
    let mut steps = 0;
    while !stepped.step(&scene, 3_000, 3) {
        steps += 1;
        assert!(stepped.bounces() < BOUNCES);
    }
    assert!(steps > 10, "{steps} steps");
    assert_eq!(stepped, serial);
}

#[test]
fn a_sealed_room_stays_dark_under_a_sunlit_roof() {
    // Six closed slabs round a room [0.2, 0.8]^3, a white ground outside.
    let (lo, hi, t) = (0.2, 0.8, 0.1);
    let mut meshes = vec![
        cuboid([lo - t, lo - t, lo - t], [hi + t, lo, hi + t]),
        cuboid([lo - t, hi, lo - t], [hi + t, hi + t, hi + t]),
        cuboid([lo - t, lo, lo - t], [lo, hi, hi + t]),
        cuboid([hi, lo, lo - t], [hi + t, hi, hi + t]),
        cuboid([lo, lo, lo - t], [hi, hi, lo]),
        cuboid([lo, lo, hi], [hi, hi, hi + t]),
    ];
    meshes.push(quad([
        [-1., 0., -1.],
        [-1., 0., 2.],
        [2., 0., 2.],
        [2., 0., -1.],
    ]));
    let paint = Paint {
        albedo: vec![[0.9; 3]; meshes.len()],
        holes: vec![],
    };
    let sun = Lamp::Far {
        to_light: crate::model::light::unit([0.3, 1.0, 0.2]).unwrap(),
        colour: [1.0; 3],
    };
    let v = built(&meshes, &paint, &[sun], 2);
    let outside = v.irradiance([1.5, 0.0, 0.5], UP);
    assert!(
        outside[0] > 0.02,
        "the ground outside gets bounce: {outside:?}"
    );
    for p in [[0.5, lo, 0.5], [0.25, lo, 0.25], [0.75, lo, 0.75]] {
        let e = v.irradiance(p, UP);
        assert!(
            e.iter().all(|&c| 0.9 * c <= 2.0 / 255.0),
            "inside at {p:?}: {e:?}"
        );
    }
}

#[test]
fn a_thin_wall_keeps_bounce_on_its_lit_side() {
    let meshes = vec![
        quad([[0., 0., 0.], [0., 0., 1.], [1., 0., 1.], [1., 0., 0.]]),
        quad([[0.5, 0., 0.], [0.5, 1., 0.], [0.5, 1., 1.], [0.5, 0., 1.]]),
    ];
    let paint = Paint {
        albedo: vec![[0.9; 3]; 2],
        holes: vec![],
    };
    let sun = Lamp::Far {
        to_light: crate::model::light::unit([1.0, 1.0, 0.0]).unwrap(),
        colour: [1.0; 3],
    };
    let v = built(&meshes, &paint, &[sun], 2);
    let lit = v.irradiance([0.55, 0.0, 0.5], UP)[0];
    let dark = v.irradiance([0.45, 0.0, 0.5], UP)[0];
    assert!(lit > 0.05, "{lit}");
    assert!(dark <= 0.02 * lit, "dark {dark} lit {lit}");
}

#[test]
fn a_cut_out_wall_lets_light_through() {
    // Sun from +x over a floor in two halves; a middle wall at x = 0.5, tall
    // enough to shade the near half and the receiving wall at x = 0 behind
    // it, unless it is cut out.
    let meshes = vec![
        quad([[0., 0., 0.], [0., 0., 1.], [0.5, 0., 1.], [0.5, 0., 0.]]),
        quad([[0.5, 0., 0.], [0.5, 0., 1.], [1., 0., 1.], [1., 0., 0.]]),
        quad([[0.5, 0., 0.], [0.5, 2., 0.], [0.5, 2., 1.], [0.5, 0., 1.]]),
        quad([[0., 0., 0.], [0., 1., 0.], [0., 1., 1.], [0., 0., 1.]]),
    ];
    let sun = Lamp::Far {
        to_light: crate::model::light::unit([1.0, 1.0, 0.0]).unwrap(),
        colour: [1.0; 3],
    };
    let white = vec![[0.9; 3]; 4];
    let mut far_black = white.clone();
    far_black[1] = [0.0; 3];
    let solid = Paint {
        albedo: white.clone(),
        holes: vec![],
    };
    let holed = Paint {
        albedo: white,
        holes: vec![2],
    };
    let hidden = Paint {
        albedo: far_black,
        holes: vec![2],
    };
    let at = ([0.0, 0.3, 0.5], [1.0, 0.0, 0.0]);
    let dark = built(&meshes, &solid, &[sun], 1).irradiance(at.0, at.1)[0];
    let open = built(&meshes, &holed, &[sun], 1).irradiance(at.0, at.1)[0];
    let without_far = built(&meshes, &hidden, &[sun], 1).irradiance(at.0, at.1)[0];
    assert!(open > 2.0 * dark + 0.05, "open {open} dark {dark}");
    // The probes see the far half through the hole, not only its light.
    assert!(
        open > without_far + 0.04,
        "open {open} without far {without_far}"
    );
}

#[test]
fn a_lamp_behind_a_surface_lights_none_of_it() {
    // In open space, so no shadow ray can stand in for the far-side rule.
    let meshes = vec![quad([
        [0., 0., 0.],
        [0., 0., 1.],
        [1., 0., 1.],
        [1., 0., 0.],
    ])];
    let paint = Paint {
        albedo: vec![[0.9; 3]],
        holes: vec![],
    };
    let index = PickIndex::new(&meshes);
    let lamps = [
        Lamp::Far {
            to_light: UP,
            colour: [1.0; 3],
        },
        point_lamp([0.5, 3.0, 0.5], [1.0; 3]),
    ];
    let scene = Scene {
        meshes: &meshes,
        index: &index,
        surfaces: &paint,
        lamps: &lamps,
        sky: [0.0; 3],
    };
    let p = [0.5, 2.0, 0.5];
    assert!(trace::direct(&scene, p, UP, 10.0)[0] > 1.0);
    assert_eq!(trace::direct(&scene, p, [0.0, -1.0, 0.0], 10.0), [0.0; 3]);
}

#[test]
fn no_lamps_build_a_dark_volume() {
    let (meshes, albedo) = cornell();
    let v = built(
        &meshes,
        &Paint {
            albedo,
            holes: vec![],
        },
        &[],
        1,
    );
    assert_eq!(v.irradiance([0.5, 0.0, 0.5], UP), [0.0; 3]);
}

#[test]
fn a_probe_inside_a_closed_mesh_is_not_used() {
    let solid = vec![cuboid([0.0; 3], [1.0; 3])];
    let mut open = solid.clone();
    open[0].triangles.truncate(10);
    assert_eq!(inside::closed_meshes(&solid), vec![true]);
    assert_eq!(inside::closed_meshes(&open), vec![false]);
    let index = PickIndex::new(&solid);
    assert!(!inside::usable(&index, &[true], [0.5; 3], 0.1, 10.0));
    assert!(inside::usable(&index, &[true], [1.5, 0.5, 0.5], 0.1, 10.0));
    let open_index = PickIndex::new(&open);
    assert!(inside::usable(&open_index, &[false], [0.5; 3], 0.1, 10.0));
}

#[test]
fn sphere_directions_are_unit_and_spread() {
    let d = sphere_directions(RAYS, false);
    assert_eq!(d.len(), RAYS);
    let mean = d
        .iter()
        .fold([0.0; 3], |m, v| [m[0] + v[0], m[1] + v[1], m[2] + v[2]]);
    for v in &d {
        assert!((crate::vec3::length(*v) - 1.0).abs() < 1e-9);
    }
    assert!(crate::vec3::length(mean) < 0.05 * RAYS as f64, "{mean:?}");
}

#[test]
fn a_lone_sheet_lit_from_above_leaves_its_underside_dark() {
    let meshes = vec![quad([
        [-3., -3., 0.],
        [3., -3., 0.],
        [3., 3., 0.],
        [-3., 3., 0.],
    ])];
    let paint = Paint {
        albedo: vec![[0.5; 3]],
        holes: vec![],
    };
    let sun = Lamp::Far {
        to_light: [0.0, 0.0, 1.0],
        colour: [1.0; 3],
    };
    for probes in [TEST_PROBES, MAX_PROBES] {
        let index = PickIndex::new(&meshes);
        let scene = Scene {
            meshes: &meshes,
            index: &index,
            surfaces: &paint,
            lamps: &[sun],
            sky: [0.0; 3],
        };
        let mut v = Volume::with_ceiling(&meshes, probes).unwrap();
        v.build(&scene, 2);
        let under = v.irradiance([0.1, 0.1, 0.0], [0.0, 0.0, -1.0]);
        let over = v.irradiance([0.1, 0.1, 0.0], [0.0, 0.0, 1.0]);
        assert!(under[0] < 1e-6, "{probes}: under {under:?}");
        // The top faces an empty sky; what it gets is the blur of the
        // probe maps' 8 by 8 texels, under 1% of the light on the sheet.
        assert!(over[0] < 0.01 * 0.5, "{probes}: over {over:?}");
    }
}
