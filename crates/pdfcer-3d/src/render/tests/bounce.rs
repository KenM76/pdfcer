//! Bounce light through the renderer: what it adds, and what it leaves.

use super::*;
use crate::{AssembledModel, Bounce, IndirectOptions, LightingScheme, ModelLight, ModelLightKind};

const WHITE: [u8; 4] = [230, 230, 230, 255];
const RED: [u8; 4] = [220, 30, 30, 255];
const GREEN: [u8; 4] = [30, 220, 30, 255];

/// A unit floor at z 0 between a red wall at x 0 and a green one at x 1,
/// lit by a lamp over its middle.
fn room(lights: Vec<ModelLight>) -> AssembledModel {
    let floor = TriangleMesh {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
        ..quad(0.0, 1.0, 0.0)
    };
    let wall = |x: f64| TriangleMesh {
        positions: vec![[x, 0., 0.], [x, 1., 0.], [x, 1., 1.], [x, 0., 1.]],
        ..quad(0.0, 1.0, 0.0)
    };
    AssembledModel {
        meshes: vec![floor, wall(0.0), wall(1.0)],
        colours: vec![Some(WHITE), Some(RED), Some(GREEN)],
        finishes: vec![None; 3],
        lights,
        ..AssembledModel::default()
    }
}

fn lamp() -> Vec<ModelLight> {
    vec![ModelLight::new(
        ModelLightKind::Point {
            position: [0.5, 0.5, 0.9],
            attenuation: [1.0, 0.0, 0.0],
        },
        [0.8; 3],
    )]
}

/// Straight down onto the floor; the walls are edge on.
fn above() -> Camera {
    Camera {
        eye: [0.5, 0.5, 5.0],
        target: [0.5, 0.5, 0.0],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Orthographic { height: 1.2 },
    }
}

fn options(bounce: Bounce) -> RenderOptions {
    RenderOptions {
        lighting: LightingScheme::Artwork,
        ambient_occlusion: false,
        indirect: IndirectOptions {
            bounce,
            ..IndirectOptions::default()
        },
        ..small()
    }
}

fn draw(model: &AssembledModel, options: &RenderOptions) -> Image {
    crate::render_model(model, &above(), options).unwrap()
}

/// Red minus green at pixel column `x`, mid height.
fn tint(image: &Image, x: u32) -> i32 {
    let [r, g, _, _] = pixel(image, x, 20);
    i32::from(r) - i32::from(g)
}

fn fnv(image: &Image) -> u64 {
    image.rgba.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn each_wall_tints_the_floor_beside_it() {
    let model = room(lamp());
    let off = draw(&model, &options(Bounce::Off));
    let on = draw(&model, &options(Bounce::ModelLights));
    // Direct light alone is white on a white floor, both sides alike.
    assert_eq!(tint(&off, 6), 0);
    assert_eq!(tint(&off, 33), 0);
    assert!(tint(&on, 6) > 3, "red side {}", tint(&on, 6));
    assert!(tint(&on, 33) < -3, "green side {}", tint(&on, 33));
    assert!(
        on.rgba.iter().zip(&off.rgba).all(|(a, b)| a >= b),
        "bounce only adds"
    );
}

#[test]
fn strength_scales_the_bounce_and_zero_draws_the_direct_light() {
    let model = room(lamp());
    let at = |strength| {
        let mut o = options(Bounce::ModelLights);
        o.indirect.bounce_strength = strength;
        draw(&model, &o)
    };
    assert_eq!(at(0.0).rgba, draw(&model, &options(Bounce::Off)).rgba);
    let sum = |i: &Image| i.rgba.iter().map(|&c| u64::from(c)).sum::<u64>();
    let (one, three) = (sum(&at(1.0)), sum(&at(3.0)));
    assert!(three > one, "{three} {one}");
}

#[test]
fn a_camera_lit_model_bounces_only_under_every_light() {
    let model = room(Vec::new());
    let off = draw(&model, &options(Bounce::Off));
    assert_eq!(draw(&model, &options(Bounce::ModelLights)).rgba, off.rgba);
    let every = draw(&model, &options(Bounce::EveryLight));
    assert_ne!(every.rgba, off.rgba);
    assert!(tint(&every, 6) > 0, "{}", tint(&every, 6));
}

#[test]
fn the_thread_count_does_not_change_a_pixel() {
    let model = room(lamp());
    let at = |threads| {
        let mut o = options(Bounce::ModelLights);
        o.threads = threads;
        draw(&model, &o).rgba
    };
    assert_eq!(at(1), at(3));
}

/// [`fnv`] of the lit room with bounce, pinned so a change to the probe
/// volume or the gather shows.
const ROOM_DIGEST: u64 = 0x235c_b70c_fc21_c8dc;

#[test]
fn the_bounced_room_draws_as_pinned() {
    assert_eq!(
        fnv(&draw(&room(lamp()), &options(Bounce::ModelLights))),
        ROOM_DIGEST
    );
}
