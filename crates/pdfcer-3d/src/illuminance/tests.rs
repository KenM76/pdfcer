// Tests fail loudly by design and index fixed fixtures.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;
use crate::camera::pick::Ray;
use crate::indirect::tests::quad;
use crate::{
    IndirectOptions, ModelLight, ModelLightKind, ModelUnits, Photometric, Projection, ShadowLight,
};

const UP: [f64; 3] = [0.0, 0.0, 1.0];

/// A square of half-width `h` at height `z`, facing up.
fn slab(h: f64, z: f64) -> TriangleMesh {
    quad([[-h, -h, z], [h, -h, z], [h, h, z], [-h, h, z]])
}

/// A lamp of `candela` at `position`, drawn at a tenth of white.
fn lamp(position: [f64; 3], candela: Option<f64>) -> ModelLight {
    let kind = ModelLightKind::Point {
        position,
        attenuation: [1.0, 0.0, 0.0],
    };
    let mut l = ModelLight::new(kind, [0.1; 3]);
    l.photometric = candela.map(Photometric::Candela);
    l
}

/// A 4 by 4 floor in metres under `lights`.
fn floor(lights: Vec<ModelLight>) -> AssembledModel {
    AssembledModel {
        meshes: vec![slab(2.0, 0.0)],
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

/// The camera every meter here is made with.
pub(super) fn camera() -> Camera {
    Camera {
        eye: [0.0, -6.0, 6.0],
        target: [0.0; 3],
        up: UP,
        projection: Projection::Perspective { fov_y: 50.0 },
    }
}

/// Direct light only.
fn direct() -> RenderOptions {
    RenderOptions {
        indirect: IndirectOptions {
            bounce: Bounce::Off,
            ..IndirectOptions::default()
        },
        ..RenderOptions::default()
    }
}

/// A meter over `model` with `options` and `settings`.
pub(super) fn meter<'m>(
    model: &'m AssembledModel,
    options: &RenderOptions,
    settings: &MeterSettings,
) -> LightMeter<'m> {
    LightMeter::new(
        model,
        &PreparedModel::new(model),
        &camera(),
        options,
        settings,
    )
    .unwrap()
}

fn sun() -> crate::SunLight {
    crate::SunLight {
        up: crate::UpAxis::Z,
        negative: false,
        azimuth_deg: 120.0,
        elevation_deg: 50.0,
        colour: crate::SunColour::Kelvin(6500.0),
        gain: 1.0,
    }
}

/// A point query on the floor (mesh 0), facing up.
pub(super) fn at(p: [f64; 3]) -> IlluminanceQuery {
    IlluminanceQuery::new(0, p, UP)
}

#[test]
fn a_lamp_gives_candela_over_metres_squared() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    assert_eq!(m.unit(), IlluminanceUnit::Lux);
    assert!(m.reasons().is_empty());
    assert!(m.notes().contains(&IlluminanceNote::PictureScaleUnused));
    assert!(m.notes().contains(&IlluminanceNote::DirectOnly));
    let r = m.read(&at([0.0; 3])).unwrap();
    assert!((r.total - 25.0).abs() < 1e-9, "{r:?}");
    assert_eq!((r.on_surface, r.off_edge), (1, 0));
    assert_eq!(r.lights.len(), 1);
    assert_eq!(r.lights[0].model_light, Some(0));
    assert_eq!(r.lights[0].name, "light 1");
    // Off to the side: farther, and at a slant.
    let side = m.read(&at([1.5, 0.0, 0.0])).unwrap();
    let d2: f64 = 1.5 * 1.5 + 4.0;
    assert!((side.total - 100.0 / d2 * (2.0 / d2.sqrt())).abs() < 1e-9);
}

#[test]
fn the_length_unit_sets_the_distance() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let half = MeterSettings {
        mm_per_unit: Some(500.0),
        ..MeterSettings::default()
    };
    let m = meter(&model, &direct(), &half);
    assert!(m.notes().contains(&IlluminanceNote::UnitGiven));
    assert!((m.read(&at([0.0; 3])).unwrap().total - 100.0).abs() < 1e-9);
    let mut unitless = model.clone();
    unitless.units = None;
    let m = meter(&unitless, &direct(), &MeterSettings::default());
    assert_eq!(m.unit(), IlluminanceUnit::Relative);
    assert_eq!(m.reasons(), [RelativeReason::NoUnits]);
    assert_eq!(
        meter(&unitless, &direct(), &half).unit(),
        IlluminanceUnit::Lux
    );
}

#[test]
fn the_reasons_for_a_relative_figure_are_named() {
    let two = floor(vec![
        lamp([0.0, 0.0, 2.0], Some(100.0)),
        lamp([1.0, 0.0, 2.0], None),
    ]);
    let m = meter(&two, &direct(), &MeterSettings::default());
    assert_eq!(m.reasons(), [RelativeReason::UnitlessLights(1)]);
    // The picture's own figure: a tenth of white from each lamp overhead.
    let r = m.read(&at([0.0; 3])).unwrap();
    assert_eq!(r.unit, IlluminanceUnit::Relative);
    assert!((r.lights[0].direct - 0.1).abs() < 1e-9, "{r:?}");
    assert!((r.direct - r.lights[0].direct - r.lights[1].direct).abs() < 1e-12);

    let lit = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let asked = MeterSettings {
        basis: MeterBasis::Relative,
        ..MeterSettings::default()
    };
    assert_eq!(
        meter(&lit, &direct(), &asked).reasons(),
        [RelativeReason::Asked]
    );
    let with = |change: fn(&mut RenderOptions)| {
        let mut o = direct();
        change(&mut o);
        meter(&lit, &o, &MeterSettings::default())
            .reasons()
            .to_vec()
    };
    assert_eq!(
        with(|o| o.artwork_headlight = true),
        [RelativeReason::Headlight]
    );
    assert_eq!(
        with(|o| o.shadow_light = ShadowLight::AimedSun(sun())),
        [RelativeReason::Sun]
    );
    assert_eq!(
        with(|o| o.lighting = crate::LightingScheme::Day),
        [RelativeReason::PictureLights]
    );
    assert_eq!(
        with(|o| {
            o.indirect = IndirectOptions {
                bounce_sky: BounceSky::Background,
                ..IndirectOptions::default()
            };
        }),
        [RelativeReason::Sky]
    );
    let bare = floor(Vec::new());
    let m = meter(&bare, &direct(), &MeterSettings::default());
    assert_eq!(m.reasons(), [RelativeReason::PictureLights]);
    let r = m.read(&at([0.0; 3])).unwrap();
    assert_eq!(r.lights[0].name, "headlight");
    assert!(r.ambient > 0.0);
}

#[test]
fn a_shelf_shades_the_floor_under_it() {
    let mut model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    // A shelf over the +x half.
    model.meshes.push(quad([
        [0.5, -2.0, 1.0],
        [2.0, -2.0, 1.0],
        [2.0, 2.0, 1.0],
        [0.5, 2.0, 1.0],
    ]));
    model.colours.push(Some([255, 255, 255, 255]));
    model.finishes.push(None);
    let m = meter(&model, &direct(), &MeterSettings::default());
    assert_eq!(m.read(&at([1.5, 0.0, 0.0])).unwrap().total, 0.0);
    assert!(m.read(&at([-1.5, 0.0, 0.0])).unwrap().total > 1.0);
}

#[test]
fn a_disc_counts_what_falls_off_the_edge() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    let inside = m.read(&at([0.0; 3]).over_disc(0.5, 200)).unwrap();
    assert_eq!((inside.on_surface, inside.off_edge), (200, 0));
    // The mean over a disc under the lamp is a little under the peak.
    assert!(inside.total < 25.0 && inside.total > 22.0, "{inside:?}");
    // Centred on the edge, half the disc is off the floor.
    let edge = m.read(&at([2.0, 0.0, 0.0]).over_disc(0.5, 200)).unwrap();
    assert_eq!(edge.on_surface + edge.off_edge, 200);
    assert!((90..=110).contains(&edge.off_edge), "{edge:?}");
    // A point picked from below still reads the side asked for.
    let ray = Ray {
        origin: [0.0, 0.0, 5.0],
        direction: [0.0, 0.0, -1.0],
        min_distance: 0.0,
    };
    let hit = crate::pick(&model.meshes, &ray).unwrap();
    let q = IlluminanceQuery::from_pick(&hit, &ray);
    assert!((m.read(&q).unwrap().total - 25.0).abs() < 1e-9);
}

#[test]
fn a_bad_or_oversized_query_is_refused() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    let err = |q: IlluminanceQuery| m.read(&q).unwrap_err();
    assert_eq!(err(at([9.0, 0.0, 0.0])), IlluminanceError::OffSurface);
    assert_eq!(err(at([0.0, 0.0, 0.5])), IlluminanceError::OffSurface);
    assert_eq!(
        err(IlluminanceQuery::new(0, [0.0; 3], [0.0; 3])),
        IlluminanceError::BadQuery("normal")
    );
    assert_eq!(
        err(at([f64::NAN, 0.0, 0.0])),
        IlluminanceError::BadQuery("point")
    );
    assert_eq!(
        err(at([0.0; 3]).over_disc(-1.0, 8)),
        IlluminanceError::BadQuery("radius")
    );
    assert_eq!(
        err(at([0.0; 3]).over_disc(0.5, MAX_ILLUMINANCE_SAMPLES + 1)),
        IlluminanceError::TooManySamples {
            asked: MAX_ILLUMINANCE_SAMPLES + 1,
            limit: MAX_ILLUMINANCE_SAMPLES
        }
    );
    // With bounce on, every sample costs its gather rays.
    let heavy = MeterSettings {
        gather_rays: 4096,
        ..MeterSettings::default()
    };
    let m = meter(&model, &RenderOptions::default(), &heavy);
    assert!(matches!(
        m.read(&at([0.0; 3]).over_disc(0.5, 4096)),
        Err(IlluminanceError::TooMuchWork { limit, .. }) if limit == MAX_ILLUMINANCE_RAYS
    ));
}

#[test]
fn a_lit_ceiling_bounces_light_onto_the_floor() {
    // The lamp is under the floor's level of sight: a baffle hides it from
    // the point measured, and a white ceiling over both is lit by it.
    let mut model = floor(vec![lamp([-1.0, 0.0, 0.5], Some(100.0))]);
    let ceiling = quad([
        [-2.0, -2.0, 2.0],
        [-2.0, 2.0, 2.0],
        [2.0, 2.0, 2.0],
        [2.0, -2.0, 2.0],
    ]);
    let baffle = quad([
        [0.0, -2.0, 0.0],
        [0.0, 2.0, 0.0],
        [0.0, 2.0, 1.0],
        [0.0, -2.0, 1.0],
    ]);
    model.meshes.extend([ceiling, baffle]);
    model.colours.extend([Some([255, 255, 255, 255]); 2]);
    model.finishes.extend([None, None]);
    let q = at([1.0, 0.0, 0.0]);
    let off = meter(&model, &direct(), &MeterSettings::default());
    assert_eq!(off.read(&q).unwrap().total, 0.0);
    let on = meter(&model, &RenderOptions::default(), &MeterSettings::default());
    assert_eq!(on.unit(), IlluminanceUnit::Lux);
    assert!(on.notes().contains(&IlluminanceNote::BounceEstimated));
    let r = on.read(&q).unwrap();
    assert_eq!(r.direct, 0.0);
    assert!(r.bounced > 0.05 && r.bounced < 25.0, "{r:?}");
    assert!((r.total - r.bounced).abs() < 1e-12);
    // Half the strength, half the bounced light, and it is said.
    let mut half = RenderOptions::default();
    half.indirect.bounce_strength = 0.5;
    let h = meter(&model, &half, &MeterSettings::default());
    assert!(h.notes().contains(&IlluminanceNote::BounceStrength(0.5)));
    assert!((h.read(&q).unwrap().bounced - r.bounced / 2.0).abs() < 1e-9);
}

#[test]
fn colour_counts_only_when_asked() {
    let mut red = lamp([0.0, 0.0, 2.0], Some(100.0));
    red.colour = [1.0, 0.0, 0.0];
    let model = floor(vec![red]);
    let neutral = meter(&model, &direct(), &MeterSettings::default());
    assert!((neutral.read(&at([0.0; 3])).unwrap().total - 25.0).abs() < 1e-9);
    let tinted = MeterSettings {
        tint: LightTint::Coloured,
        ..MeterSettings::default()
    };
    let m = meter(&model, &direct(), &tinted);
    assert!(m.notes().contains(&IlluminanceNote::TintApplied));
    assert!((m.read(&at([0.0; 3])).unwrap().total - 25.0 * 0.2126).abs() < 1e-9);
}

#[test]
fn a_model_with_every_lamp_left_out_reads_nothing() {
    let model = floor(Vec::new());
    // By default an empty list is a file with no lights: the headlight.
    let m = meter(&model, &direct(), &MeterSettings::default());
    assert_eq!(m.reasons(), [RelativeReason::PictureLights]);
    assert!(m.read(&at([0.0; 3])).unwrap().total > 0.0);
    // Asked to stay dark, there is no lamp to count and nothing in the way
    // of lux.
    let dark = RenderOptions {
        empty_artwork: crate::EmptyArtwork::Dark,
        ..direct()
    };
    let m = meter(&model, &dark, &MeterSettings::default());
    assert_eq!(m.unit(), IlluminanceUnit::Lux);
    assert!(m.reasons().is_empty());
    let r = m.read(&at([0.0; 3])).unwrap();
    assert_eq!((r.total, r.lights.len()), (0.0, 0));
    // Nothing follows the camera either, where the headlight does.
    let lamps = |empty_artwork| {
        let o = RenderOptions {
            empty_artwork,
            ..RenderOptions::default()
        };
        let s = crate::BounceStats::for_model(&camera(), &o, &[]).unwrap();
        (s.bouncing, s.camera_lights)
    };
    assert_eq!(lamps(crate::EmptyArtwork::Dark), (0, 0));
    assert_eq!(lamps(crate::EmptyArtwork::Headlight), (0, 1));
}

/// A dark floor under a 4 by 4 ceiling one unit up that glows `emissive`,
/// with every lamp left out.
pub(super) fn glow_room(emissive: [f64; 3]) -> (AssembledModel, RenderOptions) {
    let mut model = floor(Vec::new());
    model.meshes.push(quad([
        [-2.0, -2.0, 1.0],
        [-2.0, 2.0, 1.0],
        [2.0, 2.0, 1.0],
        [2.0, -2.0, 1.0],
    ]));
    model.colours.push(Some([255, 255, 255, 255]));
    model.finishes.push(Some(crate::MaterialFinish {
        ambient: [0.0; 3],
        specular: [0.0; 3],
        emissive,
        emissive_nits: None,
        shininess: 0.0,
    }));
    let dark = RenderOptions {
        empty_artwork: crate::EmptyArtwork::Dark,
        ..direct()
    };
    (model, dark)
}

/// The share of a point's view a parallel square of half-width `h`, `z`
/// over it, fills (cosine-weighted).
fn square_overhead(h: f64, z: f64) -> f64 {
    let s = h / h.hypot(z);
    4.0 / std::f64::consts::PI * s * s.atan()
}

#[test]
fn a_glowing_ceiling_lights_the_floor_under_it() {
    let (model, dark) = glow_room([1.0; 3]);
    let settings = MeterSettings {
        gather_rays: 4096,
        ..MeterSettings::default()
    };
    let m = meter(&model, &dark, &settings);
    // A glow has no physical strength.
    assert_eq!(m.unit(), IlluminanceUnit::Relative);
    assert_eq!(m.reasons(), [RelativeReason::GlowingParts(1)]);
    assert!(m.notes().contains(&IlluminanceNote::GlowNotBounced));
    let r = m.read(&at([0.0; 3])).unwrap();
    let want = square_overhead(2.0, 1.0);
    assert!((r.emitted - want).abs() < 0.02, "{r:?} want {want}");
    assert_eq!((r.direct, r.bounced, r.lights.len()), (0.0, 0.0, 0));
    assert!((r.total - r.emitted).abs() < 1e-12);
    // Half the glow, half the light; a corner sees less ceiling.
    let (dim, _) = glow_room([0.5; 3]);
    let half = meter(&dim, &dark, &settings).read(&at([0.0; 3])).unwrap();
    assert!((half.emitted - r.emitted / 2.0).abs() < 1e-9);
    let corner = m.read(&at([1.9, 1.9, 0.0])).unwrap();
    assert!(
        corner.emitted > 0.0 && corner.emitted < 0.6 * r.emitted,
        "{corner:?}"
    );
}

#[test]
fn a_glow_left_out_is_said_and_reads_nothing() {
    let (model, dark) = glow_room([1.0; 3]);
    let settings = MeterSettings {
        glow: GlowLight::LeftOut,
        ..MeterSettings::default()
    };
    let m = meter(&model, &dark, &settings);
    assert_eq!(m.unit(), IlluminanceUnit::Lux);
    assert!(m.reasons().is_empty());
    assert!(m.notes().contains(&IlluminanceNote::GlowLeftOut(1)));
    let r = m.read(&at([0.0; 3])).unwrap();
    assert_eq!((r.total, r.emitted), (0.0, 0.0));
    // A black emissive term is not a glow: nothing is said of it.
    let (black, _) = glow_room([0.0; 3]);
    let m = meter(&black, &dark, &MeterSettings::default());
    assert!(m.reasons().is_empty());
    assert!(!m.notes().iter().any(|n| matches!(
        n,
        IlluminanceNote::GlowLeftOut(_) | IlluminanceNote::GlowNotBounced
    )));
    assert_eq!(m.read(&at([0.0; 3])).unwrap().total, 0.0);
}

#[test]
fn a_glow_bounces_where_bounce_is_on() {
    let (mut model, _) = glow_room([1.0; 3]);
    // A shelf between floor and ceiling, measured on its underside.
    model.meshes.push(quad([
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ]));
    model.colours.push(Some([255, 255, 255, 255]));
    model.finishes.push(None);
    let under = IlluminanceQuery::new(2, [0.0, 0.0, 0.5], [0.0, 0.0, -1.0]);
    let dark = |bounce| RenderOptions {
        empty_artwork: crate::EmptyArtwork::Dark,
        indirect: IndirectOptions {
            bounce,
            ..IndirectOptions::default()
        },
        ..RenderOptions::default()
    };
    // The underside sees only the floor, which glows nothing.
    let off = meter(&model, &dark(Bounce::Off), &MeterSettings::default());
    assert_eq!(off.read(&under).unwrap().total, 0.0);
    let on = meter(
        &model,
        &dark(IndirectOptions::default().bounce),
        &MeterSettings::default(),
    );
    assert!(!on.notes().contains(&IlluminanceNote::GlowNotBounced));
    let r = on.read(&under).unwrap();
    assert_eq!((r.direct, r.emitted), (0.0, 0.0));
    assert!(r.bounced > 0.02 && r.bounced < 1.0, "{r:?}");
    // The floor's own reading has both parts.
    let f = on.read(&at([1.5, 1.5, 0.0])).unwrap();
    assert!(f.emitted > 0.1, "{f:?}");
    assert!((f.total - f.emitted - f.bounced).abs() < 1e-12);
}

/// `model`'s glow with luminance `nits` stated on mesh `i`.
fn stating(mut model: AssembledModel, i: usize, nits: f64) -> AssembledModel {
    if let Some(Some(f)) = model.finishes.get_mut(i) {
        f.emissive_nits = Some(nits);
    }
    model
}

#[test]
fn a_glow_of_stated_luminance_reads_in_lux() {
    let (model, dark) = glow_room([1.0, 0.5, 0.25]);
    let settings = MeterSettings {
        gather_rays: 4096,
        ..MeterSettings::default()
    };
    let lit = stating(model, 1, 100.0);
    let m = meter(&lit, &dark, &settings);
    assert_eq!(m.unit(), IlluminanceUnit::Lux, "{:?}", m.reasons());
    assert!(m.notes().contains(&IlluminanceNote::GlowLuminance(1)));
    assert!(m.notes().contains(&IlluminanceNote::GlowNotBounced));
    let r = m.read(&at([0.0; 3])).unwrap();
    let y = 0.2126 + 0.7152 * 0.5 + 0.0722 * 0.25;
    let want = std::f64::consts::PI * 100.0 * y * square_overhead(2.0, 1.0);
    assert!((r.emitted - want).abs() < 0.02 * want, "{r:?} want {want}");
    // Coloured weighs each channel; the luminance is the same.
    let coloured = MeterSettings {
        tint: LightTint::Coloured,
        ..settings
    };
    let c = meter(&lit, &dark, &coloured).read(&at([0.0; 3])).unwrap();
    assert!((c.emitted - r.emitted).abs() < 1e-9 * want, "{c:?}");
}

#[test]
fn only_a_glow_stating_no_luminance_makes_the_reading_relative() {
    let (model, dark) = glow_room([1.0; 3]);
    let mut two = model.clone();
    two.meshes.push(two.meshes[1].clone());
    two.finishes.push(two.finishes[1]);
    two.colours.push(two.colours[1]);
    let mixed = stating(two, 1, 50.0);
    let m = meter(&mixed, &dark, &MeterSettings::default());
    assert_eq!(m.unit(), IlluminanceUnit::Relative);
    assert_eq!(m.reasons(), [RelativeReason::GlowingParts(1)]);
}

/// What a lamp of 100 cd 2 m over the origin gives floor point `(x, y)`.
fn under_lamp(x: f64, y: f64) -> f64 {
    let d2 = x * x + y * y + 4.0;
    100.0 / d2 * (2.0 / d2.sqrt())
}

fn check_patch(p: &MapPatch) {
    let d = p.divisions;
    for j in 0..=d {
        for i in 0..=d - j {
            let [a, b, c] = p.corners;
            let at = |k: usize| {
                a[k] + (b[k] - a[k]) * i as f64 / d as f64 + (c[k] - a[k]) * j as f64 / d as f64
            };
            let got = p.figures[p.index(i, j)];
            assert!(
                (got - under_lamp(at(0), at(1))).abs() < 1e-9,
                "{i} {j} {got}"
            );
        }
    }
}

#[test]
fn a_map_gives_every_corner_the_reading_there() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    let mut seen = Vec::new();
    let map = m
        .map(None, MapSide::FacingCamera, &mut |p| {
            seen.push(p);
            true
        })
        .unwrap();
    assert_eq!(map.unit, IlluminanceUnit::Lux);
    assert_eq!(map.notes, m.notes());
    // Two triangles share two corners: four points, measured once each.
    assert_eq!(seen.first().map(|p| (p.done, p.total)), Some((0, 4)));
    assert_eq!(seen.last().map(|p| (p.done, p.total)), Some((4, 4)));
    for t in 0..2 {
        let p = map.patch(0, t).unwrap();
        assert_eq!((p.divisions, p.figures.len()), (1, 3));
        check_patch(p);
    }
    let (lo, hi) = map.range().unwrap();
    assert!((lo - under_lamp(2.0, 2.0)).abs() < 1e-9 && (hi - lo).abs() < 1e-9);
}

#[test]
fn a_map_spaced_finer_measures_inside_each_triangle() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    let mut total = 0;
    let map = m
        .map(Some(1.0), MapSide::FacingCamera, &mut |p| {
            total = p.total;
            true
        })
        .unwrap();
    // The 4 m sides cut in 4, the 5.66 m diagonal in 6: the shared
    // diagonal's 7 points are measured once.
    for t in 0..2 {
        let p = map.patch(0, t).unwrap();
        assert_eq!((p.divisions, p.figures.len()), (6, 28));
        check_patch(p);
    }
    assert_eq!(total, 2 * 28 - 7);
    let (_, hi) = map.range().unwrap();
    assert!(
        (hi - 25.0).abs() < 1e-9,
        "the floor's centre, under the lamp: {hi}"
    );
    let small = map.triangles().count();
    assert_eq!(small, 2 * 36);
    let buffer = map.vertex_buffer();
    assert_eq!(buffer.len(), small * 3 * MAP_VERTEX_FLOATS);
    for v in buffer.chunks(MAP_VERTEX_FLOATS) {
        let want = under_lamp(f64::from(v[0]), f64::from(v[1]));
        assert!((f64::from(v[3]) - want).abs() < 1e-4, "{v:?} {want}");
    }
}

#[test]
fn a_map_stopped_by_its_callback_is_refused() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    assert_eq!(
        m.map(None, MapSide::FacingCamera, &mut |_| false),
        Err(IlluminanceError::Cancelled)
    );
    let mut calls = 0;
    let late = m.map(None, MapSide::FacingCamera, &mut |p| {
        calls += 1;
        p.done == 0
    });
    assert_eq!((late, calls), (Err(IlluminanceError::Cancelled), 2));
}

#[test]
fn a_map_leaves_out_a_mesh_drawn_fully_transparent() {
    let mut model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    model.meshes.push(slab(1.0, 1.0));
    model.colours.push(Some([255, 255, 255, 0]));
    model.finishes.push(None);
    let m = meter(&model, &direct(), &MeterSettings::default());
    let map = m.map(None, MapSide::FacingCamera, &mut |_| true).unwrap();
    assert!(map.patch(0, 0).is_some());
    assert_eq!(map.meshes[1], vec![None, None]);
}

#[test]
fn a_map_measures_the_side_the_camera_sees_unless_told_the_normals() {
    let mut model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    for t in &mut model.meshes[0].triangles {
        t.swap(1, 2);
    }
    let m = meter(&model, &direct(), &MeterSettings::default());
    let seen = m.map(None, MapSide::FacingCamera, &mut |_| true).unwrap();
    check_patch(seen.patch(0, 0).unwrap());
    let along = m.map(None, MapSide::AlongNormals, &mut |_| true).unwrap();
    assert_eq!(along.range(), Some((0.0, 0.0)));
}

/// The lattice a GPU map packs, read back as the GPU would measure it.
fn gpu_read(g: &GpuLightMap, figure: impl Fn([f64; 3]) -> f64) -> Vec<f32> {
    let mut read = vec![0.0f32; 4 * (g.target_width * g.target_height) as usize];
    for (i, p) in g
        .points
        .texels
        .chunks(4)
        .take(g.point_count as usize)
        .enumerate()
    {
        let at = [0, 1, 2].map(|k| f64::from(p[k]) + g.origin[k]);
        read[4 * i] = figure(at) as f32;
    }
    read
}

#[test]
fn a_gpu_map_packs_the_lattice_the_cpu_maps() {
    let model = floor(vec![lamp([0.0, 0.0, 2.0], Some(100.0))]);
    let m = meter(&model, &direct(), &MeterSettings::default());
    let g = m
        .gpu_map(Some(1.0), MapSide::FacingCamera)
        .unwrap()
        .unwrap();
    assert_eq!(
        (g.point_count, g.target_width, g.target_height),
        (49, 2048, 1)
    );
    assert_eq!((g.lamp_count, g.bounce_count, g.direction_count), (1, 0, 0));
    assert_eq!((g.glow_count, g.bounce.is_none()), (0, true));
    let read = gpu_read(&g, |p| under_lamp(p[0], p[1]));
    let cpu = m
        .map(Some(1.0), MapSide::FacingCamera, &mut |_| true)
        .unwrap();
    let map = g.clone().finish(&read).unwrap();
    assert_eq!((map.unit, &map.notes), (cpu.unit, &cpu.notes));
    for t in 0..2 {
        let (got, want) = (map.patch(0, t).unwrap(), cpu.patch(0, t).unwrap());
        assert_eq!((got.corners, got.divisions), (want.corners, want.divisions));
        for (a, b) in got.figures.iter().zip(&want.figures) {
            assert!((a - b).abs() < 1e-5 * b, "{a} {b}");
        }
    }
    assert_eq!(
        g.finish(&read[..8]),
        Err(IlluminanceError::BadQuery("read"))
    );
}

#[test]
fn a_gpu_map_gathers_a_glow_in_every_direction() {
    let (model, dark) = glow_room([1.0; 3]);
    let m = meter(&model, &dark, &MeterSettings::default());
    let g = m.gpu_map(None, MapSide::FacingCamera).unwrap().unwrap();
    let rays = MeterSettings::default().gather_rays as u32;
    assert_eq!((g.glow_count, g.direction_count), (2, 2 * rays));
    assert_eq!(&g.glow.texels[4..7], &[1.0; 3]);
    assert!(g.bounce.is_none() && g.eps > 0.0 && g.far >= g.eps);
}
