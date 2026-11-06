//! A casting light takes no share on the side of a face away from it.

use super::*;
use crate::lighting::sun::{SunColour, SunLight};
use crate::{AssembledModel, LightingScheme, MaterialFinish, ModelLight, ModelLightKind};
use crate::{ShadowLight, UpAxis};

const GREY: [u8; 4] = [100, 120, 140, 255];
/// Above the sheet.
const ABOVE: [f64; 3] = [0.0, 0.3, 1.0];
/// Below it.
const BELOW: [f64; 3] = [0.0, -0.3, -1.0];
const MAP_SIZES: [u32; 3] = [64, 256, 2048];

fn from(z: f64, perspective: bool) -> Camera {
    Camera {
        eye: [0.0, 0.0, z],
        target: [0.0; 3],
        up: [0.0, 1.0, 0.0],
        projection: if perspective {
            Projection::Perspective { fov_y: 40.0 }
        } else {
            Projection::Orthographic { height: 4.0 }
        },
    }
}

fn sun_light(direction: [f64; 3], strength: f64) -> ModelLight {
    ModelLight::new(ModelLightKind::Directional { direction }, [strength; 3])
}

fn options() -> RenderOptions {
    RenderOptions {
        lighting: LightingScheme::Artwork,
        ambient_occlusion: false,
        ..small()
    }
}

fn shiny() -> Option<MaterialFinish> {
    let diffuse = [GREY[0], GREY[1], GREY[2]].map(|c| f64::from(c) / 255.0);
    Some(MaterialFinish::new(diffuse, [1.0; 3], 0.05))
}

/// A thin flat sheet, nothing else, at z 0, seen from `camera`.
fn sheet(
    lights: &[ModelLight],
    finish: Option<MaterialFinish>,
    camera: &Camera,
    options: &RenderOptions,
) -> Image {
    let model = AssembledModel {
        meshes: vec![quad(0.0, 3.0, 0.0)],
        colours: vec![Some(GREY)],
        finishes: vec![finish],
        lights: lights.to_vec(),
        ..AssembledModel::default()
    };
    crate::render_model(&model, camera, options).unwrap()
}

fn centre(image: &Image) -> [u8; 3] {
    let [r, g, b, _] = pixel(image, 20, 20);
    [r, g, b]
}

#[test]
fn a_light_above_a_sheet_lights_none_of_its_underside_at_any_map_size() {
    for perspective in [false, true] {
        for shadow_map_size in MAP_SIZES {
            let options = RenderOptions {
                shadow_map_size,
                ..options()
            };
            let lights = [sun_light(ABOVE, 1.0)];
            let under = sheet(&lights, None, &from(-6.0, perspective), &options);
            assert_eq!(centre(&under), [0; 3], "{shadow_map_size} {perspective}");
            let over = sheet(&lights, None, &from(6.0, perspective), &options);
            assert!(centre(&over)[0] > 50, "{:?}", centre(&over));
        }
    }
}

#[test]
fn each_further_light_above_a_sheet_lights_none_of_its_underside() {
    let underneath = from(-6.0, false);
    for finish in [None, shiny()] {
        for shadow_map_size in MAP_SIZES {
            let options = RenderOptions {
                shadow_map_size,
                ..options()
            };
            let alone = sheet(&[sun_light(BELOW, 1.0)], finish, &underneath, &options);
            let both = sheet(
                &[sun_light(BELOW, 1.0), sun_light(ABOVE, 0.5)],
                finish,
                &underneath,
                &options,
            );
            assert_eq!(both.rgba, alone.rgba, "{shadow_map_size} {finish:?}");
        }
    }
}

#[test]
fn a_shiny_sheet_takes_no_share_of_a_key_light_behind_it() {
    let lights = [sun_light(ABOVE, 1.0)];
    let under = sheet(&lights, shiny(), &from(-6.0, false), &options());
    assert_eq!(centre(&under), [0; 3]);
    let over = sheet(&lights, shiny(), &from(6.0, false), &options());
    assert!(centre(&over)[0] > 50, "{:?}", centre(&over));
}

#[test]
fn a_lamp_above_a_sheet_lights_none_of_its_underside() {
    let lamp = ModelLight::new(
        ModelLightKind::Point {
            position: [1.0, 0.0, 2.0],
            attenuation: [1.0, 0.0, 0.0],
        },
        [1.0; 3],
    );
    let lamps = std::slice::from_ref(&lamp);
    for finish in [None, shiny()] {
        let under = sheet(lamps, finish, &from(-6.0, false), &options());
        assert_eq!(centre(&under), [0; 3], "{finish:?}");
        let over = sheet(lamps, finish, &from(6.0, false), &options());
        assert!(centre(&over)[0] > 20, "{:?}", centre(&over));
    }
    // A lamp casting nothing has no shadow to answer with.
    let uncast = RenderOptions {
        lamp_shadows: false,
        ..options()
    };
    assert!(centre(&sheet(lamps, None, &from(-6.0, false), &uncast))[0] > 20);
}

#[test]
fn an_aimed_sun_above_a_sheet_adds_nothing_to_its_underside() {
    let sun = SunLight {
        up: UpAxis::Z,
        negative: false,
        azimuth_deg: 0.0,
        elevation_deg: 35.0,
        colour: SunColour::Kelvin(6500.0),
        gain: 1.0,
    };
    let lit_from_below = [sun_light(BELOW, 1.0)];
    let underneath = from(-6.0, true);
    for shadow_map_size in MAP_SIZES {
        let plain = RenderOptions {
            shadow_map_size,
            ..options()
        };
        let aimed = RenderOptions {
            shadow_light: ShadowLight::AimedSun(sun),
            ..plain
        };
        let with = sheet(&lit_from_below, None, &underneath, &aimed);
        let without = sheet(&lit_from_below, None, &underneath, &plain);
        assert_eq!(with.rgba, without.rgba, "{shadow_map_size}");
    }
    // Seen from above it is the sun's light that adds.
    let above = from(6.0, true);
    let lit_from_above = [sun_light(ABOVE, 0.2)];
    let aimed = RenderOptions {
        shadow_light: ShadowLight::AimedSun(sun),
        ..options()
    };
    let with = sheet(&lit_from_above, None, &above, &aimed);
    let without = sheet(&lit_from_above, None, &above, &options());
    assert!(centre(&with)[0] > centre(&without)[0] + 30);
}

/// Unshadowed lighting stays two-sided, so without a map nothing is lost.
#[test]
fn without_shadows_a_light_still_lights_both_sides() {
    let lights = [sun_light(ABOVE, 1.0)];
    let off = RenderOptions {
        shadows: false,
        ..options()
    };
    assert!(centre(&sheet(&lights, None, &from(-6.0, false), &off))[0] > 50);
}

/// The shadow key halves what a blocked point shows and is not a light's
/// own share, so a face turned from it dims only where something blocks.
#[test]
fn the_shadow_key_dims_nothing_on_a_face_turned_from_it() {
    for tilt in [-5.0, 5.0] {
        let draw = |shadows| {
            let options = RenderOptions {
                shadows,
                ambient_occlusion: false,
                ..small()
            };
            render(&[quad(0.0, 1.0, tilt)], &ortho(6.0), &options).unwrap()
        };
        assert_eq!(draw(true).rgba, draw(false).rgba, "{tilt}");
    }
}
