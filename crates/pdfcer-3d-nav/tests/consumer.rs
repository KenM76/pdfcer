//! What a shell outside the crate builds and drives: the plain structs it
//! constructs from its own state, and one orbit-then-walk round trip.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pdfcer_3d::{Bounds, NamedView, Ray, TriangleMesh};
use pdfcer_3d_nav::{
    Anchor, Axis, Intent, Lens, LensKind, NotInside, Orbit, Rules, SavedAim, Solids, Upright,
    ViewFrame, Walk, radius, reach, shaped,
};

fn floor() -> TriangleMesh {
    let mut m = TriangleMesh::default();
    m.positions = vec![
        [-10.0, -10.0, 0.0],
        [10.0, -10.0, 0.0],
        [10.0, 10.0, 0.0],
        [-10.0, 10.0, 0.0],
    ];
    m.triangles = vec![[0, 1, 2], [0, 2, 3]];
    m
}

#[test]
fn a_shell_builds_every_plain_struct_and_drives_an_orbit_and_a_walk() {
    let meshes = vec![floor()];
    let bounds = Bounds {
        min: [-10.0, -10.0, 0.0],
        max: [10.0, 10.0, 0.0],
    };
    let upright = Upright {
        up: Axis::PosZ,
        front: Axis::PosY,
    };
    let lens = Lens {
        kind: LensKind::Auto,
        ..Lens::default()
    };
    let aim = SavedAim {
        direction: [0.0, 1.0, -1.0],
        up: [0.0, 0.0, 1.0],
        position: [0.0, -5.0, 1.0],
        orthographic: true,
        framed: true,
    };
    let frame = ViewFrame {
        position: aim.position,
        height: 12.0,
    };
    assert_eq!(aim.eye_inside(&bounds), Err(NotInside::NoStandingPoint));
    assert!(!NotInside::Outside.to_string().is_empty());

    let reach = reach(&meshes, &bounds);
    let mut orbit = Orbit::saved(&aim).expect("independent axes");
    let framed = orbit.camera(&bounds, reach, 1.5, Some(frame)).unwrap();
    assert_eq!(framed.visible_height(), 12.0);
    orbit = Orbit::named(NamedView::Iso, true, upright);
    orbit.anchor = Some(Anchor {
        point: [1.0, 2.0, 0.0],
        distance: 30.0,
        offset: [0.0, 0.0],
    });
    let camera = orbit.camera(&bounds, reach, 1.5, None).unwrap();
    assert_eq!(camera.target, [1.0, 2.0, 0.0]);

    let mut walk = Walk::from_camera(&camera, orbit.axes(), radius(&bounds));
    walk.eye = [0.0, 0.0, 5.0];
    let mut solids = Solids::default();
    let rules = Rules {
        solid: true,
        level: true,
        speed: 1.0,
        stairs: true,
    };
    let intent = Intent {
        forward: shaped(0.5),
        ..Intent::default()
    };
    for _ in 0..200 {
        walk.step(intent, 0.05, rules, &mut |ray: &Ray| {
            solids.hit(&meshes, 0, || None, ray)
        });
    }
    assert!((walk.eye[2] - walk.body()).abs() < 1e-9, "{:?}", walk.eye);
    let seen = walk.camera(1.5, &lens);
    assert!(seen.eye.iter().all(|c| c.is_finite()));
}
