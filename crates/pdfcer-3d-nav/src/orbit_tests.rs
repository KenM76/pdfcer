#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unreachable
)]

use super::*;
use crate::axes::Axis;
use crate::reach::reach;
use pdfcer_3d::TriangleMesh;
use pdfcer_3d::vec3::length;

fn cube() -> Bounds {
    Bounds {
        min: [-1.0; 3],
        max: [1.0; 3],
    }
}

/// A view looking along +x and down, with +y up: no named view's axes. Its
/// matrix is `[-0.8 0 -0.6, 0 1 0, 0.6 0 -0.8, -6 0 8]`.
fn corner() -> SavedAim {
    SavedAim {
        direction: [0.6, 0.0, -0.8],
        up: [0.0, 1.0, 0.0],
        position: [-6.0, 0.0, 8.0],
        orthographic: false,
        framed: false,
    }
}

fn parallel(a: [f64; 3], b: [f64; 3]) -> bool {
    let (a, b) = (normalize(a).unwrap(), normalize(b).unwrap());
    (dot(a, b) - 1.0).abs() < 1e-9
}

fn gap(c: &Camera) -> f64 {
    length(sub(c.eye, c.target))
}

fn finite_camera(c: &Camera) -> bool {
    c.eye.iter().chain(&c.target).all(|v| v.is_finite())
}

#[test]
fn a_huge_or_tiny_box_fits_at_a_finite_positive_distance() {
    for corner in [1e200, 1e-200] {
        let bounds = Bounds {
            min: [-corner; 3],
            max: [corner; 3],
        };
        for perspective in [true, false] {
            let orbit = Orbit::named(NamedView::Iso, perspective, Upright::default());
            let c = orbit
                .camera(&bounds, reach(&[], &bounds), 1.5, None)
                .unwrap();
            let d = gap(&c);
            let h = c.visible_height();
            assert!(finite_camera(&c), "{corner:e}: {c:?}");
            assert!(d.is_finite() && d > 0.0, "{corner:e}: distance {d:e}");
            assert!(h.is_finite() && h > 0.0, "{corner:e}: height {h:e}");
            // The fitted sphere is the model's own, not a clamped one.
            let wanted = corner * 3f64.sqrt() * FIT_MARGIN * 2.0;
            if !perspective {
                assert!((d / wanted - 1.0).abs() < 1e-9, "{corner:e}: {d:e}");
            }
        }
    }
}

#[test]
fn a_box_out_at_the_largest_float_gives_a_finite_eye_or_no_fit() {
    let boxes = [
        Bounds {
            min: [0.0, -1.0, -1.0],
            max: [f64::MAX, 1.0, 1.0],
        },
        Bounds {
            min: [-f64::MAX, -1.0, -1.0],
            max: [f64::MAX, 1.0, 1.0],
        },
    ];
    let mut fits = 0;
    for bounds in boxes {
        for aspect in [1e9, 1.0, 1e-9] {
            for perspective in [true, false] {
                let mut orbit = Orbit::named(NamedView::Iso, perspective, Upright::default());
                for (zoom, pan) in [
                    (1.0, [0.0, 0.0]),
                    (ZOOM_RANGE.0, [0.0, 0.0]),
                    (1.0, [3.0, -2.0]),
                ] {
                    (orbit.zoom, orbit.pan) = (zoom, pan);
                    if let Ok(c) = orbit.camera(&bounds, reach(&[], &bounds), aspect, None) {
                        fits += 1;
                        assert!(finite_camera(&c), "{aspect:e} {zoom}: {c:?}");
                        assert!(c.visible_height().is_finite(), "{c:?}");
                    }
                }
            }
        }
    }
    // Every request here has a finite camera; none is refused.
    assert_eq!(fits, 2 * 3 * 2 * 3);
}

#[test]
fn every_named_view_forms_a_camera_for_every_upright() {
    for up in Axis::ALL {
        let upright = Upright::default().with_up(up);
        for front in upright.fronts() {
            let upright = upright.with_front(front);
            for view in NamedView::ALL {
                let orbit = Orbit::named(view, true, upright);
                let c = orbit.camera(&cube(), 1.0, 1.5, None);
                assert!(c.is_ok(), "{view:?} {upright:?}: {c:?}");
            }
        }
    }
    // Two axes along one line still turn, about the default pair for up.
    let both = Upright {
        up: Axis::PosZ,
        front: Axis::NegZ,
    };
    let orbit = Orbit::named(NamedView::Front, true, both);
    assert_eq!(orbit.axes(), Axes::upright(Upright::default()));
}

#[test]
fn the_named_views_look_the_way_their_names_say() {
    let front = Orbit::named(NamedView::Front, false, Upright::default()).direction();
    assert!(
        (front[1] - 1.0).abs() < 1e-9,
        "front looks along +y: {front:?}"
    );
    let top = Orbit::named(NamedView::Top, false, Upright::default()).direction();
    assert!(top[2] < -0.99, "top looks down: {top:?}");
    let iso = Orbit::named(NamedView::Iso, false, Upright::default()).direction();
    assert!(iso[0] < 0.0 && iso[1] > 0.0 && iso[2] < 0.0, "{iso:?}");
    let right = Orbit::named(NamedView::Right, false, Upright::default()).direction();
    assert!(parallel(right, [-1.0, 0.0, 0.0]), "{right:?}");
    let back = Orbit::named(NamedView::Back, false, Upright::default()).direction();
    assert!(parallel(back, [0.0, -1.0, 0.0]), "{back:?}");
}

#[test]
fn with_y_up_the_named_views_turn_about_y() {
    let upright = Upright::default().with_up(Axis::PosY);
    let front = Orbit::named(NamedView::Front, false, upright).direction();
    assert!(front[2] < -0.999, "front looks along -z: {front:?}");
    let top = Orbit::named(NamedView::Top, false, upright).direction();
    assert!(top[1] < -0.99, "top looks down -y: {top:?}");
}

#[test]
fn the_files_view_looks_along_its_own_axis_with_its_own_up() {
    let orbit = Orbit::saved(&corner()).expect("a camera matrix");
    assert!(orbit.perspective && !orbit.framed);
    let camera = orbit
        .camera(&cube(), radius(&cube()), 1.5, None)
        .expect("a view forms");
    let look = sub(camera.target, camera.eye);
    assert!(parallel(look, [0.6, 0.0, -0.8]), "{look:?}");
    assert!(parallel(camera.up, [0.0, 1.0, 0.0]), "{:?}", camera.up);
}

#[test]
fn a_named_views_camera_read_back_as_a_saved_view_looks_the_same_way() {
    let upright = Upright::default().with_up(Axis::PosY);
    for (view, perspective) in [
        (NamedView::Iso, true),
        (NamedView::Front, false),
        (NamedView::Top, true),
        (NamedView::Back, false),
    ] {
        let orbit = Orbit::named(view, perspective, upright);
        let camera = orbit
            .camera(&cube(), radius(&cube()), 1.5, None)
            .expect("a view forms");
        let aim = SavedAim {
            direction: sub(camera.target, camera.eye),
            up: camera.up,
            position: camera.eye,
            orthographic: !perspective,
            framed: false,
        };
        let back = Orbit::saved(&aim).expect("the view has a camera");
        assert!(
            parallel(back.direction(), orbit.direction()),
            "{view:?}: {:?} read back as {:?}",
            orbit.direction(),
            back.direction()
        );
        assert_eq!(back.perspective, perspective, "{view:?}");
    }
}

#[test]
fn a_view_with_up_along_its_look_is_not_offered() {
    let mut aim = corner();
    aim.up = [0.0, 0.0, 1.0];
    aim.direction = [0.0, 0.0, 1.0];
    assert!(Orbit::saved(&aim).is_none());
    aim.direction = [0.0; 3];
    assert!(Orbit::saved(&aim).is_none());
    aim.direction = [f64::NAN, 1.0, 0.0];
    assert!(Orbit::saved(&aim).is_none());
}

#[test]
fn a_framed_view_keeps_its_scale_until_perspective_is_chosen() {
    let mut aim = corner();
    (aim.orthographic, aim.framed) = (true, true);
    let frame = Some(ViewFrame {
        position: aim.position,
        height: 50.0,
    });
    let mut orbit = Orbit::saved(&aim).expect("a camera matrix");
    assert!(orbit.framed && !orbit.perspective);
    let framed = orbit
        .camera(&cube(), radius(&cube()), 2.0, frame)
        .expect("a view forms");
    let fitted = orbit
        .camera(&cube(), radius(&cube()), 2.0, None)
        .expect("a view forms");
    assert_eq!(framed.projection, Projection::Orthographic { height: 50.0 });
    assert_ne!(framed.projection, fitted.projection);
    orbit.perspective = true;
    let free = orbit
        .camera(&cube(), radius(&cube()), 2.0, frame)
        .expect("a view forms");
    assert!(matches!(free.projection, Projection::Perspective { .. }));
}

#[test]
fn zoom_brings_the_eye_closer_and_pan_moves_the_target() {
    let bounds = cube();
    let mut orbit = Orbit::named(NamedView::Front, true, Upright::default());
    let fitted = orbit
        .camera(&bounds, radius(&bounds), 1.0, None)
        .expect("a view forms");
    orbit.zoom = 2.0;
    orbit.pan = [0.5, 0.0];
    let moved = orbit
        .camera(&bounds, radius(&bounds), 1.0, None)
        .expect("a view forms");
    assert!((gap(&moved) - gap(&fitted) / 2.0).abs() < 1e-9);
    assert!(moved.target != fitted.target);
    assert!(
        moved.target[2].abs() < 1e-9,
        "a sideways pan keeps the height"
    );
}

/// The model point on the target plane at `offset` (picture heights,
/// right and up from the centre).
fn under(orbit: &Orbit, bounds: &Bounds, aspect: f64, offset: [f64; 2]) -> [f64; 3] {
    let camera = orbit
        .camera(bounds, radius(bounds), aspect, None)
        .expect("a view forms");
    let (right, up) = picture_axes(orbit.direction(), orbit.axes.up);
    let h = camera.visible_height();
    add(
        camera.target,
        add(scale(right, offset[0] * h), scale(up, offset[1] * h)),
    )
}

#[test]
fn a_wheel_zoom_keeps_the_point_under_the_pointer_still() {
    let bounds = Bounds {
        min: [-1.0, -2.0, -0.5],
        max: [3.0, 1.0, 2.0],
    };
    let saved = Orbit::saved(&corner()).expect("a camera matrix");
    for start in [
        Orbit::named(NamedView::Iso, true, Upright::default()),
        Orbit::named(NamedView::Iso, false, Upright::default()),
        saved,
    ] {
        let mut orbit = start;
        orbit.pan = [0.1, -0.2];
        let (aspect, offset) = (1.6, [0.3, -0.15]);
        let before = under(&orbit, &bounds, aspect, offset);
        orbit.zoom_at(&bounds, radius(&bounds), aspect, 2.5, offset, None);
        let after = under(&orbit, &bounds, aspect, offset);
        for i in 0..3 {
            assert!(
                (before[i] - after[i]).abs() < 1e-9,
                "{before:?} vs {after:?}"
            );
        }
        assert!((orbit.zoom - 2.5).abs() < 1e-12);
    }
}

#[test]
fn a_wheel_zoom_at_the_centre_does_not_pan() {
    let mut orbit = Orbit::named(NamedView::Front, true, Upright::default());
    orbit.zoom_at(&cube(), radius(&cube()), 1.0, 3.0, [0.0, 0.0], None);
    assert_eq!(orbit.pan, [0.0, 0.0]);
}

#[test]
fn a_nonsense_zoom_pan_or_point_changes_nothing() {
    let start = Orbit::named(NamedView::Iso, true, Upright::default());
    let camera = start.camera(&cube(), 1.0, 1.0, None).unwrap();
    let mut orbit = start;
    for factor in [f64::NAN, 0.0, -2.0, f64::INFINITY] {
        orbit.zoom_at(&cube(), 1.0, 1.0, factor, [0.2, 0.1], None);
    }
    orbit.zoom_at(&cube(), 1.0, 1.0, 2.0, [f64::NAN, f64::INFINITY], None);
    assert_eq!(orbit.pan, [0.0, 0.0]);
    orbit = start;
    orbit.anchor_at(&camera, [f64::NAN, 0.0, 0.0]);
    orbit.centre_on(&cube(), [f64::INFINITY, 0.0, 0.0]);
    assert_eq!(orbit, start);
    // A zoom or pan that is not finite counts as none, not as a broken view.
    (orbit.zoom, orbit.pan) = (f64::NAN, [f64::NAN, 0.0]);
    let c = orbit.camera(&cube(), 1.0, f64::NAN, None).unwrap();
    assert_eq!(c, start.camera(&cube(), 1.0, 1.0, None).unwrap());
    // An anchor whose offset would carry the target past the floats turns
    // about the point itself.
    let mut front = Orbit::named(NamedView::Front, false, Upright::default());
    front.anchor = Some(Anchor {
        point: [f64::MAX, 0.0, 0.0],
        distance: 1.0,
        offset: [-f64::MAX, 0.0],
    });
    let c = front.camera(&cube(), 1.0, 1.0, None).unwrap();
    assert_eq!(c.target, [f64::MAX, 0.0, 0.0]);
}

#[test]
fn turning_or_widening_the_picture_keeps_the_models_size() {
    let bounds = Bounds {
        min: [-4.0, -1.0, 0.0],
        max: [4.0, 1.0, 0.5],
    };
    let mut orbit = Orbit::named(NamedView::Iso, true, Upright::default());
    let first = orbit.camera(&bounds, 4.0, 1.5, None).expect("a view forms");
    for yaw in [0.3, 1.1, 2.0] {
        orbit.yaw = yaw;
        for aspect in [1.2, 1.5, 3.0] {
            let c = orbit
                .camera(&bounds, 4.0, aspect, None)
                .expect("a view forms");
            assert!((gap(&c) - gap(&first)).abs() < 1e-9, "{yaw} {aspect}");
            assert_eq!(c.target, first.target);
        }
    }
}

#[test]
fn an_anchor_keeps_the_picture_then_turns_about_its_point() {
    let bounds = cube();
    for perspective in [true, false] {
        let mut orbit = Orbit::named(NamedView::Iso, perspective, Upright::default());
        orbit.zoom = 1.7;
        orbit.pan = [0.2, -0.1];
        let before = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
        let point = add(before.target, scale(orbit.direction(), 0.4));
        orbit.anchor_at(&before, point);
        let after = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
        if perspective {
            for i in 0..3 {
                assert!((after.eye[i] - before.eye[i]).abs() < 1e-9, "{perspective}");
            }
        } else {
            assert!((after.visible_height() - before.visible_height()).abs() < 1e-9);
        }
        orbit.yaw += 0.8;
        orbit.pitch = 0.4;
        let turned = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
        for (t, p) in turned.target.iter().zip(point) {
            assert!((t - p).abs() < 1e-9);
        }
    }
}

#[test]
fn a_refit_forgets_the_point_turned_about() {
    let bounds = cube();
    let mut orbit = Orbit::named(NamedView::Iso, true, Upright::default());
    let fitted = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
    orbit.anchor_at(&fitted, [0.3, 0.2, 0.1]);
    orbit.zoom = 2.0;
    orbit.refit();
    assert_eq!(orbit.anchor, None);
    let again = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
    assert_eq!(again, fitted);
}

/// Where `p` shows on `c`'s picture: right and up of the centre, as a
/// share of the visible height at `p`'s depth.
fn shown(c: &Camera, p: [f64; 3]) -> [f64; 2] {
    let f = normalize(sub(c.target, c.eye)).unwrap();
    let (r, u) = picture_axes(f, c.up);
    let v = sub(p, c.eye);
    let h = match c.projection {
        Projection::Perspective { fov_y } => 2.0 * dot(v, f) * (fov_y / 2.0).to_radians().tan(),
        Projection::Fisheye { .. } => unreachable!("orbit tests use flat lenses"),
        Projection::Orthographic { height } => height,
    };
    [dot(v, r) / h, dot(v, u) / h]
}

#[test]
fn a_pressed_point_off_centre_keeps_its_place_as_the_view_turns() {
    let bounds = cube();
    for perspective in [true, false] {
        let mut orbit = Orbit::named(NamedView::Iso, perspective, Upright::default());
        orbit.zoom = 1.3;
        let before = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
        let f = orbit.direction();
        let (r, u) = picture_axes(f, orbit.axes.up);
        let point = add(
            before.target,
            add(add(scale(r, 0.3), scale(u, 0.2)), scale(f, 0.4)),
        );
        let at = shown(&before, point);
        orbit.anchor_at(&before, point);
        for (yaw, pitch) in [(0.0, 0.0), (0.8, 0.4), (-1.9, -0.7)] {
            orbit.yaw += yaw;
            orbit.pitch = pitch;
            let c = orbit.camera(&bounds, 1.5, 1.4, None).expect("a view forms");
            let now = shown(&c, point);
            for i in 0..2 {
                assert!(
                    (now[i] - at[i]).abs() < 1e-9,
                    "{perspective} {now:?} {at:?}"
                );
            }
        }
    }
}

#[test]
fn the_reach_of_drawn_vertices_sets_the_fit() {
    let mut mesh = TriangleMesh::default();
    mesh.positions = vec![[0.0; 3], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];
    mesh.triangles = vec![[0, 1, 2]];
    let bounds = Bounds {
        min: [0.0; 3],
        max: [4.0, 4.0, 0.0],
    };
    let orbit = Orbit::named(NamedView::Top, false, Upright::default());
    let r = reach(&[mesh], &bounds);
    let c = orbit.camera(&bounds, r, 1.0, None).unwrap();
    assert!((c.visible_height() - 2.0 * r * FIT_MARGIN).abs() < 1e-9);
}
