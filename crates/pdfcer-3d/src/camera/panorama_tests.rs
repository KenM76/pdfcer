//! Tests of [`Projection::Panorama`], a part of the sphere, and of
//! [`Camera::panorama`].

#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)] // Tests fail loudly by design.

use super::equirect_tests::close;
use super::lens_tests::{BACKGROUND, WALLS, draw, options, room};
use crate::camera::lens::{Layout, Lens};
use crate::vec3::{add, scale};
use crate::{
    Camera, FisheyeMapping, FrameLens, LightingScheme, PanoramaExtent, PanoramaMapping,
    PanoramaTilt, Projection, RenderError, RenderOptions, pick,
};

const EQ: PanoramaMapping = PanoramaMapping::Equirectangular;
const CYL: PanoramaMapping = PanoramaMapping::Cylindrical;

fn band(horizontal: f64, vertical: f64, pitch: f64, mapping: PanoramaMapping) -> Camera {
    Camera {
        eye: [0.3, -0.2, 0.1],
        target: [3.3, 0.6, 0.1],
        up: [0.0, 0.0, 1.0],
        projection: Projection::Panorama {
            horizontal,
            vertical,
            pitch,
            mapping,
        },
    }
}

fn sphere() -> Camera {
    Camera {
        projection: Projection::Fisheye {
            fov: 360.0,
            mapping: FisheyeMapping::Equirectangular,
        },
        ..band(360.0, 180.0, 0.0, EQ)
    }
}

fn lens(camera: &Camera, w: u32, h: u32) -> Lens {
    Lens::for_camera(camera, w, h).unwrap().unwrap()
}

fn sized(w: u32, h: u32) -> RenderOptions {
    RenderOptions {
        width: w,
        height: h,
        ..options(LightingScheme::None)
    }
}

/// A strip's pixel shows what the whole sphere shows at the same
/// longitude and latitude: a 90° × 45° strip pitched 22.5° up is the
/// sphere picture's columns 135°–225° and rows 45°–90° from the top.
#[test]
fn a_strip_pixel_looks_where_the_sphere_does_at_its_angles() {
    let strip = lens(&band(90.0, 45.0, 22.5, EQ), 40, 20);
    let whole = lens(&sphere(), 160, 80);
    for (x, y) in [(0.0, 0.0), (20.0, 10.0), (40.0, 20.0), (7.3, 15.1)] {
        let d = strip.direction(x, y).unwrap();
        let same = whole.direction(60.0 + x, 20.0 + y).unwrap();
        assert!(close(d, same), "{x},{y}: {d:?} vs {same:?}");
        let [px, py] = strip.pixel(d).unwrap();
        assert!((px - x).abs() < 1e-9 && (py - y).abs() < 1e-9);
    }
}

/// A cylindrical strip's rows are evenly spaced in tan(latitude), so a
/// vertical line in the world is a straight column of evenly spaced
/// heights.
#[test]
fn a_cylindrical_strip_spaces_rows_by_the_tangent_of_latitude() {
    let lens = lens(&band(120.0, 60.0, 0.0, CYL), 120, 60);
    let t = 30f64.to_radians().tan();
    for (y, tan) in [(0.0, t), (15.0, t / 2.0), (30.0, 0.0), (60.0, -t)] {
        let d = lens.direction(60.0, y).unwrap();
        assert!((d[1] / d[2] - tan).abs() < 1e-12, "{y}: {d:?}");
    }
    let [_, y] = lens.pixel([0.0, t / 4.0, 1.0]).unwrap();
    assert!((y - 22.5).abs() < 1e-9);
}

/// 360° × 180° at pitch 0 is the whole equirectangular sphere, pixel for
/// pixel, and so is accepted by the GPU lens; any narrower band is drawn
/// only by the CPU renderer.
#[test]
fn the_whole_band_is_the_sphere_and_a_partial_one_is_cpu_only() {
    let opts = sized(64, 32);
    assert_eq!(
        draw(&band(360.0, 180.0, 0.0, EQ), &opts),
        draw(&sphere(), &opts)
    );
    assert!(FrameLens::new(&band(360.0, 180.0, 0.0, EQ), 64, 32).is_ok());
    for camera in [band(360.0, 90.0, 0.0, EQ), band(359.0, 180.0, 0.0, EQ)] {
        assert!(matches!(
            FrameLens::new(&camera, 64, 32),
            Err(RenderError::Camera(_))
        ));
    }
}

/// Every pixel of a strip away from a wall edge shows the wall the ray
/// through it meets: the drawing and the picking agree, so the strip is
/// drawn from the band's own directions, not the sphere's.
#[test]
fn a_strip_draws_the_walls_its_rays_meet() {
    for camera in [
        band(90.0, 45.0, 10.0, EQ),
        band(200.0, 40.0, -20.0, EQ),
        band(360.0, 60.0, 70.0, EQ),
        band(150.0, 50.0, 15.0, CYL),
    ] {
        let (w, h) = (96u32, 32u32);
        let (rgba, _) = draw(&camera, &sized(w, h));
        let wall = |i: i64, j: i64| -> Option<usize> {
            let ray = camera
                .ray_through(i as f64 + 0.5, j as f64 + 0.5, w, h)
                .unwrap();
            pick(&room(), &ray).map(|hit| hit.mesh)
        };
        let mut checked = 0;
        for j in 1..i64::from(h) - 1 {
            for i in 1..i64::from(w) - 1 {
                let here = wall(i, j);
                let inner = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .all(|(di, dj)| wall(i + di, j + dj) == here);
                if !inner {
                    continue;
                }
                let p = ((j * i64::from(w) + i) * 4) as usize;
                assert_eq!(
                    rgba[p..p + 4],
                    WALLS[here.unwrap()],
                    "{:?} at {i},{j}",
                    camera.projection
                );
                checked += 1;
            }
        }
        assert!(checked > 1000, "{checked}");
        assert!(!rgba.chunks(4).any(|p| p == BACKGROUND));
    }
}

/// Only the faces owning a band pixel are drawn: a strip within 90° of
/// the look direction is one planar face, and a 360° strip is a cube.
#[test]
fn a_narrow_strip_is_one_face_and_a_full_turn_a_cube() {
    assert!(matches!(
        lens(&band(90.0, 45.0, 0.0, EQ), 400, 200).layout(),
        Layout::Narrow { .. }
    ));
    assert!(matches!(
        lens(&band(360.0, 30.0, 0.0, EQ), 1200, 100).layout(),
        Layout::Cube { .. }
    ));
}

/// The equirectangular band keeps its height and stops at a pole; the
/// cylindrical one cannot reach a pole and is refused.
#[test]
fn a_band_past_a_pole_is_moved_back_or_refused() {
    let up = lens(&band(120.0, 40.0, 80.0, EQ), 60, 20);
    assert!(close(up.direction(30.0, 0.0).unwrap(), [0.0, 1.0, 0.0]));
    let [_, mid] = up.pixel([0.0, 70f64.to_radians().tan(), 1.0]).unwrap();
    assert!((mid - 10.0).abs() < 1e-9, "{mid}");
    let refused = Err(RenderError::PanoramaField { mapping: CYL });
    assert_eq!(
        Lens::for_camera(&band(120.0, 40.0, 70.0, CYL), 60, 20),
        refused
    );
    assert_eq!(
        Lens::for_camera(&band(120.0, 180.0, 0.0, CYL), 60, 20),
        refused
    );
    for (h, v, p) in [
        (0.0, 40.0, 0.0),
        (361.0, 40.0, 0.0),
        (90.0, 0.0, 0.0),
        (90.0, 181.0, 0.0),
        (90.0, 40.0, f64::NAN),
        (f64::INFINITY, 40.0, 0.0),
    ] {
        assert_eq!(
            crate::render(&room(), &band(h, v, p, EQ), &sized(8, 8)).err(),
            Some(RenderError::PanoramaField { mapping: EQ }),
            "{h} {v} {p}"
        );
    }
}

const UP: [f64; 3] = [0.0, 0.0, 1.0];
const FRONT: [f64; 3] = [0.0, 1.0, 0.0];

fn looking_down() -> Camera {
    Camera {
        eye: [1.0, 2.0, 3.0],
        target: add([1.0, 2.0, 3.0], [0.0, 3.0, -3.0f64.sqrt()]),
        up: [0.0, 0.5, 0.8],
        projection: Projection::Perspective { fov_y: 50.0 },
    }
}

fn extent(tilt: PanoramaTilt) -> PanoramaExtent {
    PanoramaExtent {
        horizontal: 180.0,
        vertical: Some(60.0),
        tilt,
        mapping: EQ,
    }
}

/// A ring at the eye's height lands on one row either way: the middle
/// row levelled, and the top row of a 60° band tilted with the view's 30°
/// downward pitch.
#[test]
fn levelled_and_tilted_strips_both_keep_the_horizon_straight() {
    for (tilt, row) in [(PanoramaTilt::Level, 50.0), (PanoramaTilt::View, 0.0)] {
        let camera = looking_down().panorama(UP, FRONT, extent(tilt)).unwrap();
        assert_eq!(camera.up, UP);
        for k in -4..=4 {
            let turn = f64::from(k) * 0.35 + std::f64::consts::FRAC_PI_2;
            let round = add(camera.eye, scale([turn.cos(), turn.sin(), 0.0], 5.0));
            let [_, y] = camera.pixel_of(round, 300, 100).unwrap();
            assert!((y - row).abs() < 1e-9, "{tilt:?} {k}: {y}");
        }
    }
    let tilted = looking_down()
        .panorama(UP, FRONT, extent(PanoramaTilt::View))
        .unwrap();
    let Projection::Panorama { pitch, .. } = tilted.projection else {
        panic!()
    };
    assert!((pitch + 30.0).abs() < 1e-9, "{pitch}");
}

/// `vertical: None` takes the view's own angle; an orthographic view has
/// none; a view pitched past what the band can centre on is moved inside
/// the pole, and a cylindrical one is refused.
#[test]
fn the_vertical_angle_can_come_from_the_view() {
    let from_view = PanoramaExtent {
        vertical: None,
        ..extent(PanoramaTilt::Level)
    };
    let done = looking_down().panorama(UP, FRONT, from_view).unwrap();
    assert!(matches!(done.projection, Projection::Panorama { vertical, .. } if vertical == 50.0));
    let flat = Camera {
        projection: Projection::Orthographic { height: 4.0 },
        ..looking_down()
    };
    assert!(matches!(
        flat.panorama(UP, FRONT, from_view),
        Err(RenderError::Camera(_))
    ));
    let steep = PanoramaExtent {
        vertical: Some(100.0),
        ..extent(PanoramaTilt::View)
    };
    let done = looking_down().panorama(UP, FRONT, steep).unwrap();
    assert!(
        matches!(done.projection, Projection::Panorama { pitch, .. } if (pitch + 30.0).abs() < 1e-9)
    );
    let steeper = PanoramaExtent {
        vertical: Some(140.0),
        ..steep
    };
    let done = looking_down().panorama(UP, FRONT, steeper).unwrap();
    assert!(matches!(done.projection, Projection::Panorama { pitch, .. } if pitch == -20.0));
    let cylinder = PanoramaExtent {
        mapping: CYL,
        ..steeper
    };
    assert_eq!(
        looking_down().panorama(UP, FRONT, cylinder),
        Err(RenderError::PanoramaField { mapping: CYL })
    );
}
