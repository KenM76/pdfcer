//! Tests of [`FisheyeMapping::Equirectangular`], the 360° panorama lens.

#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)] // Tests fail loudly by design.

use super::lens_tests::{BACKGROUND, WALLS, camera, check_composition, draw, options, picks, room};
use crate::camera::lens::Lens;
use crate::vec3::{length, normalize, sub};
use crate::{
    AssembledModel, Camera, FisheyeMapping, FrameLens, LightingScheme, ModelLine, Projection,
    RenderError, RenderOptions, render_model,
};

const PANO: FisheyeMapping = FisheyeMapping::Equirectangular;
const W: u32 = 64;
const H: u32 = 32;

fn panorama(width: u32, height: u32) -> Lens {
    Lens::for_camera(&camera(360.0, PANO), width, height)
        .unwrap()
        .unwrap()
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    length(sub(a, b)) < 1e-9
}

#[test]
fn the_panorama_centre_looks_ahead_and_its_rows_end_straight_up_and_down() {
    let lens = panorama(W, H);
    let (w, h) = (f64::from(W), f64::from(H));
    assert_eq!(lens.direction(w / 2.0, h / 2.0), Some([0.0, 0.0, 1.0]));
    assert!(close(
        lens.direction(w / 2.0, 0.0).unwrap(),
        [0.0, 1.0, 0.0]
    ));
    assert!(close(lens.direction(w / 2.0, h).unwrap(), [0.0, -1.0, 0.0]));
    assert!(close(
        lens.direction(0.75 * w, h / 2.0).unwrap(),
        [1.0, 0.0, 0.0]
    ));
    assert!(close(
        lens.direction(0.25 * w, h / 2.0).unwrap(),
        [-1.0, 0.0, 0.0]
    ));
    assert!(close(
        lens.direction(0.0, h / 2.0).unwrap(),
        [0.0, 0.0, -1.0]
    ));
    assert!(close(lens.direction(w, h / 2.0).unwrap(), [0.0, 0.0, -1.0]));
    assert_eq!(lens.pixel([0.0, 0.0, 1.0]), Some([w / 2.0, h / 2.0]));
}

/// `pixel` inverts `direction` everywhere but the poles' single points,
/// including a hair from the poles and either side of the seam.
#[test]
fn the_panorama_round_trips_near_the_poles_and_across_the_seam() {
    for (w, h) in [(W, H), (300, 100), (50, 80)] {
        let lens = panorama(w, h);
        let (fw, fh) = (f64::from(w), f64::from(h));
        for x in [1e-6, 0.3, 0.25 * fw, 0.5 * fw, 0.9 * fw, fw - 1e-6] {
            for y in [1e-4, 0.5, 0.3 * fh, 0.5 * fh, 0.8 * fh, fh - 1e-4] {
                let d = lens.direction(x, y).unwrap();
                assert!((length(d) - 1.0).abs() < 1e-12);
                let [px, py] = lens.pixel(d).unwrap();
                assert!(
                    (px - x).abs() < 1e-6 && (py - y).abs() < 1e-9,
                    "{x},{y} -> {px},{py}"
                );
            }
        }
        let behind_left = lens.pixel([-1e-9, 0.2, -1.0]).unwrap();
        let behind_right = lens.pixel([1e-9, 0.2, -1.0]).unwrap();
        assert!(behind_left[0] < 1e-6 && behind_right[0] > fw - 1e-6);
        for d in [
            [2.0, -3.0, -0.5],
            [0.0, 5.0, 1e-12],
            [-1.0, 0.0, -1e-12],
            [0.0, -2.0, 0.0],
        ] {
            let back = lens.direction(lens.pixel(d).unwrap()[0], lens.pixel(d).unwrap()[1]);
            assert!(close(back.unwrap(), normalize(d).unwrap()), "{d:?}");
        }
        assert_eq!(lens.pixel([0.0; 3]), None);
        assert_eq!(lens.pixel([f64::NAN, 0.0, 1.0]), None);
    }
}

#[test]
fn a_panorama_field_other_than_360_is_refused_by_name() {
    let refused = RenderError::FisheyeField { mapping: PANO };
    for fov in [359.9, 360.5, 180.0, 0.0, f64::NAN, f64::INFINITY] {
        let camera = camera(fov, PANO);
        let render = crate::render(&room(), &camera, &options(LightingScheme::None));
        assert_eq!(render.err(), Some(refused.clone()), "{fov}");
        assert_eq!(
            camera.ray_through(60.0, 45.0, 120, 90).err(),
            Some(refused.clone())
        );
        assert_eq!(
            FrameLens::new(&camera, 120, 90).err(),
            Some(refused.clone())
        );
    }
    assert!(FrameLens::new(&camera(360.0, PANO), 120, 90).is_ok());
}

/// Every pixel has a ray, and the picture composes from all six faces,
/// which the host is told to draw.
#[test]
fn a_panorama_composes_from_every_cube_face_and_fills_the_picture() {
    check_composition(360.0, PANO, true);
    assert!(picks(&camera(360.0, PANO)).iter().all(Option::is_some));
    for (w, h) in [(120, 90), (2048, 1024), (1, 1), (40, 300)] {
        let frame = FrameLens::new(&camera(360.0, PANO), w, h).unwrap();
        let mut faces: Vec<u32> = frame.faces.iter().map(|f| f.face).collect();
        faces.sort_unstable();
        assert_eq!(faces, [0, 1, 2, 3, 4, 5], "{w}x{h}");
        assert_eq!(frame.mapping, 2);
    }
}

/// From the room's centre looking along +x with +z up: ahead at the
/// centre, behind at both side edges, the left and right walls a quarter
/// in, the ceiling and floor along the top and bottom rows.
#[test]
fn the_panorama_shows_ahead_at_its_centre_and_behind_at_its_edges() {
    let camera = Camera {
        eye: [0.0; 3],
        target: [1.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        projection: Projection::Fisheye {
            fov: 360.0,
            mapping: PANO,
        },
    };
    let options = RenderOptions {
        width: W,
        height: H,
        ..options(LightingScheme::None)
    };
    let (rgba, _) = draw(&camera, &options);
    let at = |i: u32, j: u32| -> [u8; 4] {
        let p = ((j * W + i) * 4) as usize;
        rgba[p..p + 4].try_into().unwrap()
    };
    // Walls: 0 +x, 1 -x, 2 +y, 3 -y, 4 +z, 5 -z; the camera's right is -y.
    assert_eq!(at(W / 2, H / 2), WALLS[0], "ahead");
    assert_eq!(at(0, H / 2), WALLS[1], "behind, left edge");
    assert_eq!(at(W - 1, H / 2), WALLS[1], "behind, right edge");
    assert_eq!(at(W / 4, H / 2), WALLS[2], "left");
    assert_eq!(at(3 * W / 4, H / 2), WALLS[3], "right");
    assert_eq!(at(W / 2, 0), WALLS[4], "up");
    assert_eq!(at(W / 2, H - 1), WALLS[5], "down");
    assert!(
        !rgba.chunks(4).any(|p| p == BACKGROUND),
        "a panorama has no rim"
    );
}

/// A wire passing behind the eye is cut at the seam, not drawn back
/// across the picture through what is ahead.
#[test]
fn a_wire_behind_the_eye_is_cut_at_the_seam() {
    const RED: [u8; 4] = [255, 0, 0, 255];
    let model = AssembledModel {
        meshes: vec![super::lens_tests::wall(0, 1.0)],
        lines: vec![ModelLine {
            points: vec![[-3.0, -20.0, 2.0], [-3.0, 20.0, 2.0]],
            colour: Some(RED),
            ..ModelLine::default()
        }],
        ..AssembledModel::default()
    };
    let camera = Camera {
        eye: [0.0; 3],
        target: [1.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        projection: Projection::Fisheye {
            fov: 360.0,
            mapping: PANO,
        },
    };
    let options = RenderOptions {
        width: 128,
        height: 64,
        ..options(LightingScheme::None)
    };
    let rgba = render_model(&model, &camera, &options).unwrap().rgba;
    let red_in = |columns: std::ops::Range<usize>| {
        (0..64).any(|j| {
            columns
                .clone()
                .any(|i| rgba[(j * 128 + i) * 4..][..4] == RED)
        })
    };
    assert!(
        red_in(0..8) && red_in(120..128),
        "the wire is drawn at both edges"
    );
    assert!(
        !red_in(48..80),
        "the wire is drawn across the picture's centre"
    );
}
