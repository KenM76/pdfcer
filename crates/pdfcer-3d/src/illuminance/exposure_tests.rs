//! [`Exposure`] over a white floor under one lamp.

// Tests fail loudly by design and index fixed fixtures.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;
use crate::indirect::tests::quad;
use crate::{
    IndirectOptions, ModelLight, ModelLightKind, ModelUnits, Photometric, Projection, ShadeMode,
};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 24;

/// A lamp of `candela` 2 m above the origin, drawn at a tenth of white.
fn lamp(candela: Option<f64>) -> ModelLight {
    let kind = ModelLightKind::Point {
        position: [0.0, 0.0, 2.0],
        attenuation: [1.0, 0.0, 0.0],
    };
    let mut l = ModelLight::new(kind, [0.1; 3]);
    l.photometric = candela.map(Photometric::Candela);
    l
}

/// A white 4 m floor under `lights`, with an importer's ambient fill.
fn floor(lights: Vec<ModelLight>) -> AssembledModel {
    let mut lights = lights;
    lights.push(ModelLight {
        photometric: Some(Photometric::Lux(0.0)),
        ..ModelLight::new(ModelLightKind::Ambient, [0.2; 3])
    });
    AssembledModel {
        meshes: vec![quad([
            [-2.0, -2.0, 0.0],
            [2.0, -2.0, 0.0],
            [2.0, 2.0, 0.0],
            [-2.0, 2.0, 0.0],
        ])],
        colours: vec![Some([255, 255, 255, 255])],
        finishes: vec![None],
        lights,
        units: Some(ModelUnits {
            mm_per_unit: 1000.0,
            from_cad_file: true,
        }),
        ..AssembledModel::default()
    }
}

/// Looking down at the origin, which is the middle pixel.
fn camera() -> Camera {
    Camera {
        eye: [0.0, -0.5, 6.0],
        target: [0.0; 3],
        up: [0.0, 0.0, 1.0],
        projection: Projection::Perspective { fov_y: 40.0 },
    }
}

/// Direct light only, no occlusion darkening.
fn options() -> RenderOptions {
    RenderOptions {
        width: WIDTH,
        height: HEIGHT,
        ambient_occlusion: false,
        indirect: IndirectOptions {
            bounce: Bounce::Off,
            ..IndirectOptions::default()
        },
        ..RenderOptions::default()
    }
}

fn fixed(white_lux: f64) -> Exposure {
    Exposure::Fixed { white_lux }
}

fn lighting(model: &AssembledModel, exposure: Exposure) -> ExposedLighting {
    exposure
        .lighting(model, &options(), &MeterSettings::default())
        .unwrap()
}

/// The red channel of the middle pixel.
fn middle(model: &AssembledModel, exposure: Exposure) -> u8 {
    let image = lighting(model, exposure)
        .render(&PreparedModel::new(model), model, &camera())
        .unwrap();
    let i = 4 * ((HEIGHT / 2) * WIDTH + WIDTH / 2) as usize;
    image.rgba[i]
}

/// What the meter reads at the origin, in lux.
fn lux(model: &AssembledModel) -> f64 {
    let prepared = PreparedModel::new(model);
    let meter = LightMeter::new(
        model,
        &prepared,
        &camera(),
        &options(),
        &MeterSettings::default(),
    )
    .unwrap();
    assert_eq!(meter.unit(), IlluminanceUnit::Lux);
    meter
        .read(&IlluminanceQuery::new(0, [0.0; 3], [0.0, 0.0, 1.0]))
        .unwrap()
        .total
}

#[test]
fn metered_is_the_picture_drawn_without_it() {
    for model in [floor(vec![lamp(Some(100.0))]), floor(vec![lamp(None)])] {
        let o = RenderOptions {
            light_gain: 1.5,
            ..options()
        };
        let prepared = PreparedModel::new(&model);
        let exposed = Exposure::default()
            .lighting(&model, &o, &MeterSettings::default())
            .unwrap();
        assert_eq!(exposed.options, o);
        assert_eq!(exposed.white_lux, None);
        let got = exposed.render(&prepared, &model, &camera()).unwrap();
        assert_eq!(got, prepared.render(&model, &camera(), &o).unwrap());
    }
}

#[test]
fn a_surface_draws_its_lux_over_the_white_point() {
    let model = floor(vec![lamp(Some(100.0))]);
    let e = lux(&model);
    assert!((e - 25.0).abs() < 1e-9, "{e}");
    // The middle pixel sits a hair off the origin, so it reads a little
    // under 25 lux.
    for white in [25.0, 50.0, 100.0, 400.0] {
        let want = 255.0 * e / white;
        let got = f64::from(middle(&model, fixed(white)));
        assert!(
            (got - want).abs() <= want * 0.01 + 1.0,
            "{white}: {got} vs {want}"
        );
    }
    // Above the white point a channel stops at full white.
    assert_eq!(middle(&model, fixed(20.0)), 255);
}

#[test]
fn twice_the_candela_draws_twice_as_bright() {
    let at = |cd, exposure| middle(&floor(vec![lamp(Some(cd))]), exposure);
    let (one, two) = (at(100.0, fixed(200.0)), at(200.0, fixed(200.0)));
    assert!(one > 20 && two < 250, "{one} {two}");
    assert!(
        (f64::from(two) - 2.0 * f64::from(one)).abs() <= 1.0,
        "{one} {two}"
    );
    // The metered picture draws the stored strength, which is the same.
    assert_eq!(at(100.0, Exposure::Metered), at(200.0, Exposure::Metered));
}

#[test]
fn the_gain_and_the_fill_play_no_part() {
    let model = floor(vec![lamp(Some(100.0))]);
    let exposed = lighting(&model, fixed(100.0));
    assert_eq!(exposed.options.light_gain, 1.0);
    assert_eq!(exposed.white_lux, Some(100.0));
    assert_eq!(exposed.lights[1].intensity, 0.0, "the fill lights nothing");
    let o = RenderOptions {
        light_gain: 3.0,
        ..options()
    };
    let gained = fixed(100.0)
        .lighting(&model, &o, &MeterSettings::default())
        .unwrap();
    assert_eq!(gained.lights, exposed.lights);
    assert!(gained.notes.contains(&IlluminanceNote::AmbientFillLeftOut));
}

#[test]
fn a_light_without_a_physical_strength_is_refused_with_the_reason() {
    let refused = |model: &AssembledModel, o: &RenderOptions| {
        fixed(500.0)
            .lighting(model, o, &MeterSettings::default())
            .unwrap_err()
    };
    let unitless = floor(vec![lamp(Some(100.0)), lamp(None)]);
    let err = refused(&unitless, &options());
    assert_eq!(
        err,
        ExposureError::Relative(vec![RelativeReason::UnitlessLights(1)])
    );
    assert!(
        err.to_string()
            .contains("1 of the model's lights carry no physical strength"),
        "{err}"
    );
    let model = floor(vec![lamp(Some(100.0))]);
    let headlight = RenderOptions {
        artwork_headlight: true,
        ..options()
    };
    assert_eq!(
        refused(&model, &headlight),
        ExposureError::Relative(vec![RelativeReason::Headlight])
    );
    let mut no_units = model.clone();
    no_units.units = None;
    assert_eq!(
        refused(&no_units, &options()),
        ExposureError::Relative(vec![RelativeReason::NoUnits])
    );
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            fixed(bad).lighting(&model, &options(), &MeterSettings::default()),
            Err(ExposureError::WhiteLux),
            "{bad}"
        );
    }
}

#[test]
fn the_gpu_lights_shade_the_floor_as_the_cpu_picture_does() {
    let model = floor(vec![lamp(Some(100.0))]);
    let exposed = lighting(&model, fixed(100.0));
    let l = exposed.frame_lights(&camera()).unwrap();
    assert_eq!((l.mode, l.local_count), (ShadeMode::Lights, 1));
    // The reference shader's local-light term at the origin, facing up.
    let [x, y, z, _] = l.local_positions[0];
    let d = (x * x + y * y + z * z).sqrt();
    let [a, b, c] = l.local_attenuation[0];
    let facing = z / d;
    let gpu = l.ambient[0] + l.local_colours[0][0] * facing / (a + b * d + c * d * d);
    let cpu = f32::from(middle(&model, fixed(100.0))) / 255.0;
    assert!((gpu - 0.25).abs() < 1e-6, "{gpu}");
    assert!((gpu - cpu).abs() <= 1.0 / 255.0, "{gpu} vs {cpu}");
}

#[test]
fn a_picture_in_lux_rolls_over_range_toward_white_unless_told_to_clip() {
    use crate::OverRange;
    let model = floor(vec![lamp(Some(100.0))]);
    for (asked, drawn) in [
        (OverRange::Auto, OverRange::TowardWhite),
        (OverRange::Clip, OverRange::Clip),
        (OverRange::TowardWhite, OverRange::TowardWhite),
    ] {
        let o = RenderOptions {
            over_range: asked,
            ..options()
        };
        let exposed = fixed(100.0)
            .lighting(&model, &o, &MeterSettings::default())
            .unwrap();
        assert_eq!(exposed.options.over_range, drawn, "{asked:?}");
        let gpu = exposed.frame_lights(&camera()).unwrap();
        assert_eq!(gpu.toward_white, drawn.rolls(), "{asked:?}");
    }
}

/// The floor under a 100 cd lamp, glowing a twentieth of white at `nits`.
fn glowing_floor(nits: Option<f64>) -> AssembledModel {
    let mut model = floor(vec![lamp(Some(100.0))]);
    let mut glow = crate::MaterialFinish::new([1.0; 3], [0.0; 3], 0.0);
    glow.emissive = [0.05; 3];
    glow.emissive_nits = nits;
    model.finishes = vec![Some(glow)];
    model
}

#[test]
fn a_glow_of_stated_luminance_is_drawn_against_the_white_point() {
    let nits = 4000.0 / std::f64::consts::PI;
    let lit = middle(&floor(vec![lamp(Some(100.0))]), fixed(1000.0));
    // 0.05 × nits·π / 1000 = 0.2 of full white on top of the lamp's light.
    let stated = middle(&glowing_floor(Some(nits)), fixed(1000.0));
    assert!(
        (i32::from(stated) - i32::from(lit) - 51).abs() <= 1,
        "{lit} {stated}"
    );
    let half = middle(&glowing_floor(Some(nits)), fixed(2000.0));
    let lit_half = middle(&floor(vec![lamp(Some(100.0))]), fixed(2000.0));
    assert!(
        (i32::from(half) - i32::from(lit_half) - 26).abs() <= 1,
        "{half}"
    );
    // Unstated: the stored colour, and a note says so.
    let relative = middle(&glowing_floor(None), fixed(1000.0));
    assert!(
        (i32::from(relative) - i32::from(lit) - 13).abs() <= 1,
        "{relative}"
    );
    let notes = |m| lighting(&m, fixed(1000.0)).notes;
    assert!(notes(glowing_floor(None)).contains(&IlluminanceNote::GlowRelative(1)));
    assert!(!notes(glowing_floor(Some(nits))).contains(&IlluminanceNote::GlowRelative(1)));
    // The GPU's uniform is scaled the same way.
    let exposed = lighting(&glowing_floor(Some(nits)), fixed(1000.0));
    let finish = crate::gpu::finish::DrawFinish::new(
        &glowing_floor(Some(nits)).finishes[0].unwrap(),
        &exposed.options,
    );
    assert!(
        (finish.emissive[0] - 0.2).abs() < 1e-5,
        "{:?}",
        finish.emissive
    );
    // Without an exposure the luminance plays no part.
    let metered = Exposure::Metered
        .lighting(
            &glowing_floor(Some(nits)),
            &options(),
            &MeterSettings::default(),
        )
        .unwrap();
    assert_eq!(metered.options.emissive_white_lux, None);
}

#[test]
fn the_gpu_bounce_build_traces_the_exposed_lights() {
    let model = floor(vec![lamp(Some(100.0))]);
    let mut lit = options();
    lit.indirect.bounce = Bounce::ModelLights;
    let exposed = fixed(100.0)
        .lighting(&model, &lit, &MeterSettings::default())
        .unwrap();
    let prepared = PreparedModel::new(&model);
    let built = exposed
        .gpu_bounce_build(&prepared, &model, &camera())
        .unwrap()
        .unwrap();
    let relit = AssembledModel {
        lights: exposed.lights.clone(),
        ..model.clone()
    };
    let want = PreparedModel::new(&relit)
        .gpu_bounce_build(&relit, &camera(), &exposed.options)
        .unwrap()
        .unwrap();
    assert_eq!(built, want);
    let unexposed = prepared
        .gpu_bounce_build(&model, &camera(), &lit)
        .unwrap()
        .unwrap();
    assert_ne!(built.lamps, unexposed.lamps);
    let cpu = exposed
        .frame_bounce(&prepared, &model, &camera())
        .unwrap()
        .unwrap();
    let b = &built.bounce;
    assert_eq!(
        (b.origin, b.spacing, b.dims, b.strength, b.follows_camera),
        (
            cpu.origin,
            cpu.spacing,
            cpu.dims,
            cpu.strength,
            cpu.follows_camera
        )
    );
}
