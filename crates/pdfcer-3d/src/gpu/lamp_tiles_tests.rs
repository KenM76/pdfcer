#![allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.

use crate::{
    Camera, FrameLights, LAMP_TILE_SIZE, ModelLight, ModelLightKind, Projection, RenderOptions,
};

const W: u32 = 70;
const H: u32 = 50;

fn lamps() -> Vec<ModelLight> {
    let mut out = Vec::new();
    for i in 0..15 {
        for j in 0..15 {
            let position = [
                -14.0 + 2.0 * f64::from(i),
                -14.0 + 2.0 * f64::from(j),
                0.3 * f64::from((i + j) % 3),
            ];
            out.push(ModelLight::new(
                ModelLightKind::Point {
                    position,
                    attenuation: [1.0, 0.0, 3000.0],
                },
                [1.0, 0.8, 0.5],
            ));
        }
    }
    out
}

fn camera(projection: Projection) -> Camera {
    Camera {
        eye: [1.0, -12.0, 9.0],
        target: [0.5, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        projection,
    }
}

fn options() -> RenderOptions {
    RenderOptions {
        width: W,
        height: H,
        ..RenderOptions::default()
    }
}

/// A small generator, so the points are the same every run.
struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn mul(m: &[f32; 16], p: [f32; 4]) -> [f32; 4] {
    [0, 1, 2, 3].map(|r| (0..4).map(|c| m[c * 4 + r] * p[c]).sum())
}

/// Every point a lamp reaches, as the GPU sees it (single precision, its
/// matrices), lies in a tile that lists the lamp.
fn every_reached_point_is_listed(projection: Projection) {
    let camera = camera(projection);
    let lights = FrameLights::for_model(&camera, &options(), &lamps()).unwrap();
    let tiles = &lights.tiles;
    assert_eq!(tiles.tile_size, LAMP_TILE_SIZE);
    assert_eq!((tiles.columns, tiles.rows), (5, 4));
    assert!(lights.local_count > 50, "{}", lights.local_count);
    let entries: usize = tiles.lists.iter().map(Vec::len).sum();
    assert!(
        entries * 2 <= tiles.lists.len() * lights.local_count,
        "{entries}"
    );
    let view = camera.view_matrix().unwrap();
    let aspect = f64::from(W) / f64::from(H);
    let bounds = crate::Bounds {
        min: [-16.0, -16.0, -1.0],
        max: [16.0, 16.0, 2.0],
    };
    let proj = camera.projection_matrix(aspect, &bounds).unwrap();
    let mut rng = Lcg(7);
    let mut checked = 0;
    for (i, at) in lights.local_positions.iter().enumerate() {
        let reach = lights.local_reach[i];
        assert!(reach.is_finite() && reach < 10.0, "{reach}");
        for _ in 0..400 {
            let d = [0, 1, 2].map(|_| rng.unit().mul_add(2.0, -1.0) as f32);
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if len <= 0.0 {
                continue;
            }
            let k = reach * (0.9 + 0.1 * rng.unit() as f32) / len;
            let p = [at[0] + d[0] * k, at[1] + d[1] * k, at[2] + d[2] * k, 1.0];
            let c = mul(&proj, mul(&view, p));
            if c[3] <= 0.0 || c[0].abs() > c[3] || c[1].abs() > c[3] {
                continue;
            }
            let x = (c[0] / c[3] + 1.0) / 2.0 * W as f32;
            let y = (c[1] / c[3] + 1.0) / 2.0 * H as f32;
            let (col, row) = ((x as u32 / LAMP_TILE_SIZE), (y as u32 / LAMP_TILE_SIZE));
            let list = &tiles.lists
                [(row.min(tiles.rows - 1) * tiles.columns + col.min(tiles.columns - 1)) as usize];
            assert!(list.contains(&(i as u16)), "lamp {i} at pixel {x} {y}");
            checked += 1;
        }
    }
    assert!(checked > 5000, "{checked}");
}

#[test]
fn a_perspective_tile_lists_every_lamp_reaching_it() {
    every_reached_point_is_listed(Projection::Perspective { fov_y: 50.0 });
}

#[test]
fn an_orthographic_tile_lists_every_lamp_reaching_it() {
    every_reached_point_is_listed(Projection::Orthographic { height: 20.0 });
}

#[test]
fn the_lists_keep_the_lamp_order_and_pack_as_documented() {
    let lights = FrameLights::for_model(
        &camera(Projection::Perspective { fov_y: 50.0 }),
        &options(),
        &lamps(),
    )
    .unwrap();
    let tiles = &lights.tiles;
    for list in &tiles.lists {
        assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
    }
    let t = tiles.texture();
    assert_eq!(t.width, 2048);
    let texel = |i: usize| &t.texels[4 * i..4 * i + 4];
    let count = tiles.lists.len();
    for (n, list) in tiles.lists.iter().enumerate() {
        let (first, len) = (texel(n)[0] as usize, texel(n)[1] as usize);
        assert_eq!(len, list.len());
        for (j, &lamp) in list.iter().enumerate() {
            let k = first + j;
            assert_eq!(texel(count + k / 4)[k % 4], f32::from(lamp));
        }
    }
}

#[test]
fn a_fisheye_or_a_frame_without_lamps_has_no_tiles() {
    let fisheye = Camera {
        projection: Projection::Fisheye {
            fov: 180.0,
            mapping: crate::FisheyeMapping::default(),
        },
        ..camera(Projection::Perspective { fov_y: 50.0 })
    };
    let lit = FrameLights::for_model(&fisheye, &options(), &lamps()).unwrap();
    assert!(lit.local_count > 0);
    assert_eq!(lit.tiles, crate::LampTiles::default());
    let dark = FrameLights::for_model(
        &camera(Projection::Perspective { fov_y: 50.0 }),
        &options(),
        &[],
    )
    .unwrap();
    assert_eq!(dark.tiles.tile_size, 0);
}

#[test]
fn tiles_that_would_not_halve_the_visits_are_left_out() {
    let wide: Vec<ModelLight> = lamps()
        .into_iter()
        .map(|mut l| {
            if let ModelLightKind::Point { attenuation, .. } = &mut l.kind {
                *attenuation = [1.0, 0.0, 0.01];
            }
            l
        })
        .collect();
    let camera = camera(Projection::Perspective { fov_y: 50.0 });
    let lights = FrameLights::for_model(&camera, &options(), &wide).unwrap();
    assert!(lights.local_count > 0);
    assert_eq!(lights.tiles, crate::LampTiles::default());
}
