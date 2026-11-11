//! The lamp cull and [`crate::LampCost`] against a model of many lamps,
//! most far outside the view.

#![allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.

use crate::{
    AssembledModel, Camera, FrameLights, LIGHT_TABLE_WIDTH, LampCost, LampStats, MaterialFinish,
    ModelLight, ModelLightKind, Projection, RenderOptions, TriangleMesh, render_model,
};

/// The lamps the view meets, by model index: three over the floor, one
/// far off whose reach crosses into the view, and a spot.
pub(crate) const IN_VIEW: [usize; 5] = [2, 7, 50, 90, 120];

fn point(position: [f64; 3], colour: [f64; 3]) -> ModelLight {
    let kind = ModelLightKind::Point {
        position,
        attenuation: [1.0, 0.0, 1.0],
    };
    ModelLight::new(kind, colour)
}

/// 121 dim lamps 10,000 apart, none near the floor, with [`IN_VIEW`]'s
/// inserted among them: 126 lamps, and a sun.
pub(crate) fn lamps() -> Vec<ModelLight> {
    let mut out: Vec<ModelLight> = (0..121)
        .map(|k| {
            let (i, j) = (f64::from(k / 11), f64::from(k % 11));
            point([i * 1e4 - 45e3, j * 1e4 - 55e3, 1.0], [0.4, 0.5, 0.6])
        })
        .collect();
    let mut specular = point([4.0, 2.0, 2.0], [2.0, 1.5, 1.0]);
    specular.specular = Some([0.0, 0.0, 6.0]);
    let spot = ModelLight::new(
        ModelLightKind::Spot {
            position: [0.0, -2.0, 4.0],
            direction: [0.0, 0.3, -1.0],
            attenuation: [1.0, 0.0, 0.5],
            angle: 0.6,
            exponent: 2.0,
        },
        [3.0, 2.0, 2.0],
    );
    let near = [
        point([0.0, 0.0, 3.0], [4.0, 4.0, 3.0]),
        point([200.0, 0.0, 1.0], [1.0; 3]),
        specular,
        spot,
        point([-3.0, -2.0, 2.5], [1.0, 2.0, 4.0]),
    ];
    for (i, l) in IN_VIEW.into_iter().zip(near) {
        out.insert(i, l);
    }
    let sun = ModelLight::new(
        ModelLightKind::Directional {
            direction: [0.2, 0.1, -1.0],
        },
        [0.1; 3],
    );
    out.push(sun);
    out
}

fn quad(p: [[f64; 3]; 4]) -> TriangleMesh {
    TriangleMesh {
        positions: p.to_vec(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        ..TriangleMesh::default()
    }
}

/// A floor, and a tilted shiny card over it.
pub(crate) fn model(lights: Vec<ModelLight>) -> AssembledModel {
    AssembledModel {
        meshes: vec![
            quad([
                [-8.0, -8.0, 0.0],
                [8.0, -8.0, 0.0],
                [8.0, 8.0, 0.0],
                [-8.0, 8.0, 0.0],
            ]),
            quad([
                [-1.0, -1.0, 0.8],
                [1.0, -1.0, 1.2],
                [1.0, 1.0, 1.2],
                [-1.0, 1.0, 0.8],
            ]),
        ],
        finishes: vec![None, Some(MaterialFinish::new([0.2; 3], [1.0; 3], 40.0))],
        lights,
        ..AssembledModel::default()
    }
}

/// A perspective and an orthographic camera on the same eye, so a test covers both cull shapes.
pub(crate) fn cameras() -> [Camera; 2] {
    let at = |projection| Camera {
        eye: [3.0, -4.0, 12.0],
        target: [0.0; 3],
        up: [0.0, 0.0, 1.0],
        projection,
    };
    [
        at(Projection::Perspective { fov_y: 40.0 }),
        at(Projection::Orthographic { height: 14.0 }),
    ]
}

/// Small render options with lamp shadows on or off, shared by the cull tests.
pub(crate) fn options(lamp_shadows: bool) -> RenderOptions {
    RenderOptions {
        width: 96,
        height: 64,
        lamp_shadows,
        lamp_shadow_map_size: 64,
        shadow_map_size: 256,
        // These pin the direct light, digests from before the bounce.
        indirect: crate::IndirectOptions::off(),
        ..RenderOptions::default()
    }
}

fn fnv(h: u64, bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes
        .into_iter()
        .fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// FNV-1a over [`model`]'s frames of `lights` from both cameras, at each
/// of `shadows` for lamp shadows.
fn cpu_digest(lights: Vec<ModelLight>, which: &[bool]) -> u64 {
    let model = model(lights);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for camera in cameras() {
        for &shadows in which {
            h = fnv(
                h,
                render_model(&model, &camera, &options(shadows))
                    .unwrap()
                    .rgba,
            );
        }
    }
    h
}

/// FNV-1a over the lamp rows of the light table, from the first camera,
/// texel 1's `w` left out.
fn table_digest(lights: &FrameLights, rows: &[usize]) -> u64 {
    let table = lights.light_table();
    let row = LIGHT_TABLE_WIDTH * 4;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &r in rows {
        let mut texels = table[(lights.count + r) * row..][..row].to_vec();
        texels[7] = 0.0;
        h = fnv(h, texels.iter().flat_map(|v| v.to_bits().to_le_bytes()));
    }
    h
}

/// [`cpu_digest`] of [`lamps`], lamp shadows off, on the tree before the
/// cull.
const CPU_DIGEST_BEFORE_THE_CULL: u64 = 0xfff0_d35f_4ab0_2184;
/// Both CPU digests have the far side of a face taking no casting light.
/// [`cpu_digest`] of [`few`], lamp shadows on, on the tree before the cull
/// and the lamp shadow choice: each of its five lamps cast there too.
const FEW_DIGEST_BEFORE_THE_CULL: u64 = 0x54d6_8adf_b218_e231;
/// [`table_digest`] of the full table's [`IN_VIEW`] rows before the cull.
const TABLE_DIGEST_BEFORE_THE_CULL: u64 = 0x2a3f_0202_cdbe_27ba;

/// [`IN_VIEW`]'s lamps and the sun alone.
fn few() -> Vec<ModelLight> {
    let all = lamps();
    let mut out: Vec<ModelLight> = IN_VIEW.iter().map(|&i| all[i].clone()).collect();
    out.extend(all.last().cloned());
    out
}

#[test]
fn the_cull_draws_every_pixel_as_before_it() {
    assert_eq!(cpu_digest(lamps(), &[false]), CPU_DIGEST_BEFORE_THE_CULL);
    // Five lamps under the cap: choosing the casters by their claim casts
    // all five, as before, whatever slots they take.
    assert_eq!(cpu_digest(few(), &[true]), FEW_DIGEST_BEFORE_THE_CULL);
}

#[test]
fn culled_lamps_never_reach_the_light_table() {
    let [camera, _] = cameras();
    let lights = FrameLights::for_model(&camera, &options(true), &lamps()).unwrap();
    assert_eq!(lights.local_lamps, IN_VIEW);
    assert_eq!(lights.local_count, IN_VIEW.len());
    assert_eq!(lights.light_table_rows(), lights.count + IN_VIEW.len());
    let rows: Vec<usize> = (0..IN_VIEW.len()).collect();
    assert_eq!(table_digest(&lights, &rows), TABLE_DIGEST_BEFORE_THE_CULL);
    // Each row names its lamp's shadow slot, brightest at the target
    // first: lamp 2 above it, the spot (90), 120, 50, then 7 far off.
    assert_eq!(
        lights.local_slots,
        [Some(0), Some(4), Some(3), Some(1), Some(2)]
    );
    let table = lights.light_table();
    let w: Vec<f32> = rows
        .iter()
        .map(|r| table[(lights.count + r) * LIGHT_TABLE_WIDTH * 4 + 7])
        .collect();
    assert_eq!(w, [1.0, 5.0, 4.0, 2.0, 3.0]);
}

#[test]
fn the_counts_say_what_was_culled_on_both_paths() {
    for camera in cameras() {
        let o = options(true);
        let n = IN_VIEW.len();
        let gpu = FrameLights::for_model(&camera, &o, &lamps()).unwrap().lamps;
        let cpu = LampStats::for_model(&camera, &o, &lamps()).unwrap();
        assert_eq!(cpu, gpu);
        assert_eq!((cpu.in_model, cpu.in_view, cpu.lit_this_frame), (126, n, n));
        assert_eq!((cpu.would_cast, cpu.casting), (n, n));
        let bounds = crate::Bounds::of(&model(lamps()).meshes).unwrap();
        let shadows = crate::FrameLampShadows::for_model(&camera, &o, &bounds, &lamps())
            .unwrap()
            .unwrap();
        assert_eq!((shadows.would_cast, shadows.casting), (n, n));
    }
    // A fisheye has no single frustum: nothing is culled.
    let fisheye = Camera {
        projection: Projection::Fisheye {
            fov: 180.0,
            mapping: crate::FisheyeMapping::Equidistant,
        },
        ..cameras()[0]
    };
    let s = LampStats::for_model(&fisheye, &options(true), &lamps()).unwrap();
    assert_eq!((s.in_model, s.in_view), (126, 126));
    // Every lamp would cast; the cap holds.
    assert_eq!((s.would_cast, s.casting), (126, crate::MAX_LAMP_SHADOWS));
}

#[test]
fn nearest_per_frame_keeps_the_lamps_brightest_at_the_target() {
    let [camera, _] = cameras();
    let o = RenderOptions {
        lamp_cost: LampCost::NearestPerFrame(2),
        ..options(true)
    };
    let lights = FrameLights::for_model(&camera, &o, &lamps()).unwrap();
    // At the origin: the lamp 3 above it (16 / 10 over 1 + 9) and the
    // spot (cone and fade) beat the rest; kept in model order.
    assert_eq!(lights.local_lamps, [2, 90]);
    let s = lights.lamps;
    assert_eq!((s.in_view, s.lit_this_frame), (IN_VIEW.len(), 2));
    assert_eq!(LampStats::for_model(&camera, &o, &lamps()).unwrap(), s);
    // It changes the picture, and Full does not.
    let model = model(lamps());
    let draw = |o: &RenderOptions| render_model(&model, &camera, o).unwrap().rgba;
    assert_ne!(draw(&o), draw(&options(true)));
    let none = RenderOptions {
        lamp_cost: LampCost::NearestPerFrame(0),
        ..o
    };
    assert_eq!(
        FrameLights::for_model(&camera, &none, &lamps())
            .unwrap()
            .local_count,
        0
    );
}
