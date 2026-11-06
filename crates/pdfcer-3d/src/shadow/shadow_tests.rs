//! Frame-level tests of [`crate::ShadowCasters`] and [`crate::ShadowAlpha`]:
//! a floor at z 0, a card above it, an orthographic camera looking down.

#![allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.

use crate::render::raster::Paint;
use crate::render::render_painted;
use crate::{
    Camera, Image, ModelLight, ModelLightKind, Projection, RenderOptions, ShadowAlpha,
    ShadowCasters, Texture, TextureFunction, TextureOrigin, TextureWrap, TriangleMesh,
};

/// A square of half-side `half` at height `z`, texture coordinates 0–1
/// across it (u along x, v along y).
fn quad(z: f64, half: f64) -> TriangleMesh {
    TriangleMesh {
        positions: vec![
            [-half, -half, z],
            [half, -half, z],
            [half, half, z],
            [-half, half, z],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        triangle_uvs: vec![vec![Some([0, 1, 2]), Some([0, 2, 3])]],
        ..TriangleMesh::default()
    }
}

/// A texture of one row whose texels have these alphas, replacing the
/// colour.
fn alphas(alpha: &[u8]) -> Texture {
    Texture {
        width: alpha.len() as u32,
        height: 1,
        rgba: alpha.iter().flat_map(|&a| [200, 200, 200, a]).collect(),
        wrap: [TextureWrap::Clamp; 2],
        function: TextureFunction::Replace,
        uv_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        uv_set: 0,
        origin: TextureOrigin::TopLeft,
    }
}

/// 40 × 40 pixels over 6 units: x = (px + 0.5 − 20) · 0.15.
fn options(shadow_alpha: ShadowAlpha, shadow_casters: ShadowCasters) -> RenderOptions {
    RenderOptions {
        width: 40,
        height: 40,
        ambient_occlusion: false,
        shadow_alpha,
        shadow_casters,
        ..RenderOptions::default()
    }
}

fn camera() -> Camera {
    Camera {
        eye: [0.0, 0.0, 10.0],
        target: [0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Orthographic { height: 6.0 },
    }
}

/// A directional light towards `to_light`, grey `level`.
fn sun(to_light: [f64; 3], level: f64) -> ModelLight {
    ModelLight::new(
        ModelLightKind::Directional {
            direction: to_light,
        },
        [level; 3],
    )
}

/// The floor and a card of `card` paint at z 1, half-side 0.5.
fn draw(lights: &[ModelLight], card: Paint<'_>, options: &RenderOptions) -> Image {
    let meshes = [quad(0.0, 3.0), quad(1.0, 0.5)];
    let floor = Paint {
        colour: [200, 200, 200, 255],
        texture: None,
    };
    let opaque = card.colour[3] == 255 && card.texture.is_none_or(Texture::is_opaque);
    render_painted(
        (&meshes, lights),
        &[floor, card],
        &[true, opaque],
        &camera(),
        options,
    )
    .unwrap()
}

fn red(image: &Image, x: u32) -> i32 {
    i32::from(image.rgba[((20 * image.width + x) * 4) as usize])
}

const SOLID: Paint<'static> = Paint {
    colour: [200, 200, 200, 255],
    texture: None,
};

#[test]
fn every_directional_light_casts_and_takes_only_its_own_light() {
    // Lights up-right and up-left: the card's shadows fall 1 to the left
    // (x -1.5..-0.5, pixel 13) and 1 to the right (pixel 26).
    let both = [sun([1.0, 0.0, 1.0], 0.8), sun([-1.0, 0.0, 1.0], 0.5)];
    let every = draw(
        &both,
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::EveryLight),
    );
    let key = draw(
        &both,
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::KeyLight),
    );
    let open = red(&every, 2);
    assert_eq!(open, red(&key, 2));
    // The brighter light is the key: its shadow (left) is the same either way.
    assert!(red(&key, 13) < open - 20 && red(&every, 13) == red(&key, 13));
    // The other's shadow only when every light casts ...
    assert_eq!(red(&key, 26), open);
    assert!(red(&every, 26) < open - 20, "{} {open}", red(&every, 26));
    // ... where only the key light's share is left.
    let only_a = draw(
        &both[..1],
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::KeyLight),
    );
    assert!((red(&every, 26) - red(&only_a, 26)).abs() <= 1);
    let only_b = draw(
        &both[1..],
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::KeyLight),
    );
    assert!((red(&every, 13) - red(&only_b, 13)).abs() <= 1);
}

#[test]
fn one_casting_light_or_only_opaque_meshes_draw_as_before() {
    let one = [sun([1.0, 0.0, 1.0], 0.8)];
    let every = draw(
        &one,
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::EveryLight),
    );
    let key = draw(
        &one,
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::KeyLight),
    );
    assert_eq!(every, key);
    for alpha in [
        ShadowAlpha::OpaqueOnly,
        ShadowAlpha::Cutout { cutoff: 1 },
        ShadowAlpha::Cutout { cutoff: 255 },
    ] {
        assert_eq!(
            draw(&one, SOLID, &options(alpha, ShadowCasters::KeyLight)),
            key
        );
    }
    // The camera light's fixed fallback casts alone.
    let fallback = draw(
        &[],
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::EveryLight),
    );
    let alone = draw(
        &[],
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::KeyLight),
    );
    assert_eq!(fallback, alone);
}

#[test]
fn a_translucent_card_casts_a_partial_shadow() {
    let one = [sun([1.0, 0.0, 1.0], 0.8)];
    let veil = Paint {
        colour: [200, 200, 200, 102],
        texture: None,
    };
    let key = |alpha| options(alpha, ShadowCasters::KeyLight);
    let open = red(&draw(&one, SOLID, &key(ShadowAlpha::Transmit)), 2);
    let full = red(&draw(&one, SOLID, &key(ShadowAlpha::Transmit)), 13);
    let partial = red(&draw(&one, veil, &key(ShadowAlpha::Transmit)), 13);
    // Alpha 0.4 lets 0.6 of the light through.
    let expected = f64::from(full) + 0.6 * f64::from(open - full);
    assert!(
        (f64::from(partial) - expected).abs() <= 2.0,
        "{partial} {expected}"
    );
    assert_eq!(
        red(&draw(&one, veil, &key(ShadowAlpha::OpaqueOnly)), 13),
        open
    );
    let cutout = |cutoff| ShadowAlpha::Cutout { cutoff };
    assert_eq!(red(&draw(&one, veil, &key(cutout(128))), 13), open);
    assert_eq!(red(&draw(&one, veil, &key(cutout(64))), 13), full);
}

#[test]
fn a_cut_out_texture_casts_per_texel() {
    // The card's left half is opaque, its right half clear: of the shadow
    // at x -1.5..-0.5 only -1.5..-1 is cast (pixel 11), not -1..-0.5
    // (pixel 15).
    let one = [sun([1.0, 0.0, 1.0], 0.8)];
    let texture = alphas(&[255, 255, 0, 0]);
    let card = Paint {
        colour: [255; 4],
        texture: Some(&texture),
    };
    let solid = draw(
        &one,
        SOLID,
        &options(ShadowAlpha::Transmit, ShadowCasters::KeyLight),
    );
    let open = red(&solid, 2);
    for alpha in [ShadowAlpha::Transmit, ShadowAlpha::Cutout { cutoff: 128 }] {
        let image = draw(&one, card, &options(alpha, ShadowCasters::KeyLight));
        assert_eq!(red(&image, 11), red(&solid, 11), "{alpha:?}");
        assert_eq!(red(&image, 15), open, "{alpha:?}");
    }
    let none = draw(
        &one,
        card,
        &options(ShadowAlpha::OpaqueOnly, ShadowCasters::KeyLight),
    );
    assert_eq!(red(&none, 11), open);
}

#[test]
fn a_lamp_sees_through_translucent_and_cut_out_cards() {
    // Lamp at (-1, 0, 2): the card's shadow covers x 0..2 on the floor;
    // its left half's x 0..1 (pixel 24), its right half's x 1..2 (pixel 28).
    let lamp = [ModelLight::new(
        ModelLightKind::Point {
            position: [-1.0, 0.0, 2.0],
            attenuation: [1.0, 0.0, 0.0],
        },
        [1.0; 3],
    )];
    let key = options(ShadowAlpha::Transmit, ShadowCasters::KeyLight);
    let solid = draw(&lamp, SOLID, &key);
    let lit = draw(
        &lamp,
        SOLID,
        &RenderOptions {
            lamp_shadows: false,
            ..key
        },
    );
    let (dark, open) = (red(&solid, 28), red(&lit, 28));
    assert!(dark < open - 50, "{dark} {open}");
    let veil = Paint {
        colour: [200, 200, 200, 102],
        texture: None,
    };
    let partial = f64::from(red(&draw(&lamp, veil, &key), 28));
    let expected = f64::from(dark) + 0.6 * f64::from(open - dark);
    assert!((partial - expected).abs() <= 2.0, "{partial} {expected}");
    let texture = alphas(&[255, 255, 0, 0]);
    let card = Paint {
        colour: [255; 4],
        texture: Some(&texture),
    };
    let cut = draw(&lamp, card, &key);
    assert_eq!(red(&cut, 24), red(&solid, 24));
    assert_eq!(red(&cut, 28), open);
}

#[test]
fn a_highlight_dims_inside_the_shadow_of_the_light_that_makes_it() {
    // The floor shines; the card shades the second sun's share at pixel 26.
    let both = [sun([1.0, 0.0, 1.0], 0.8), sun([-1.0, 0.0, 1.0], 0.5)];
    let meshes = [quad(0.0, 3.0), quad(1.0, 0.5)];
    let shiny = crate::MaterialFinish::new([0.0; 3], [0.3; 3], 0.05);
    let dark = Paint {
        colour: [60, 60, 60, 255],
        texture: None,
    };
    let options = options(ShadowAlpha::Transmit, ShadowCasters::EveryLight);
    let shot = |finishes: &[Option<crate::MaterialFinish>]| {
        let model = (&meshes[..], &both[..], finishes);
        crate::render::render_finished(model, &[dark, SOLID], &[true, true], &camera(), &options)
            .unwrap()
            .0
    };
    let plain = shot(&[]);
    let glossy = shot(&[Some(shiny), None]);
    let shine = |x| red(&glossy, x) - red(&plain, x);
    assert!(shine(2) > 10, "{}", shine(2));
    assert!(shine(26) < shine(2) - 5, "{} {}", shine(26), shine(2));
}
