//! The texture pictures a GPU bounce build samples, packed for the trace
//! shader's `scene_colour`, which mirrors [`Texture::sample`] and
//! [`Texture::apply`].

use super::{ByteTexture, FLAT_WIDTH, FloatTexture};
use crate::render::raster::Paint;
use crate::{Texture, TextureFunction, TextureOrigin, TextureWrap};

/// Texels per packed texture in the materials texture.
pub(super) const MATERIAL_TEXELS: usize = 5;
/// Picture texels per full layer.
const LAYER: usize = FLAT_WIDTH * FLAT_WIDTH;
/// The most picture layers a host is asked for.
const MAX_LAYERS: usize = 16;
/// Offsets are split at 2^24 so each half is exact in `f32`.
const OFFSET_SPLIT: usize = 1 << 24;

/// Every distinct texture of `paints`, with the slot each mesh samples;
/// `None` for an untextured mesh and for one whose picture no longer fits
/// the 16 picture layers, which reflects its centroid colour.
pub(super) fn pack_pictures(paints: &[Paint]) -> (ByteTexture, FloatTexture, Vec<Option<f32>>) {
    let budget = LAYER * MAX_LAYERS;
    let mut seen: Vec<&Texture> = Vec::new();
    let mut slots: Vec<Option<f32>> = Vec::with_capacity(paints.len());
    let mut rgba: Vec<u8> = Vec::new();
    let mut materials: Vec<[f32; 4]> = Vec::new();
    for paint in paints {
        let Some(texture) = paint.texture else {
            slots.push(None);
            continue;
        };
        if let Some(i) = seen.iter().position(|t| std::ptr::eq(*t, texture)) {
            slots.push(Some(i as f32));
            continue;
        }
        let pixels = texture.width as usize * texture.height as usize;
        let offset = rgba.len() / 4;
        let picture = texture.rgba.get(..pixels * 4);
        let Some(picture) = picture.filter(|_| offset + pixels <= budget) else {
            slots.push(None);
            continue;
        };
        slots.push(Some(seen.len() as f32));
        seen.push(texture);
        rgba.extend_from_slice(picture);
        materials.extend(material_texels(texture, offset));
    }
    let used = rgba.len() / 4;
    let (height, layers) = if used <= LAYER {
        (used.div_ceil(FLAT_WIDTH).max(1), 1)
    } else {
        (FLAT_WIDTH, used.div_ceil(LAYER))
    };
    rgba.resize(FLAT_WIDTH * height * layers * 4, 0);
    let pictures = ByteTexture {
        texels: rgba,
        width: FLAT_WIDTH as u32,
        height: height as u32,
        layers: layers as u32,
    };
    (
        pictures,
        FloatTexture::from_rows(FLAT_WIDTH, materials),
        slots,
    )
}

/// `[offset low, width, height, origin]`, `[uv row 0, wrap u]`, `[uv row
/// 1, wrap v]`, `[function, offset high, 0, 0]`, `[blend colour]`.
fn material_texels(t: &Texture, offset: usize) -> [[f32; 4]; MATERIAL_TEXELS] {
    let wrap = |w: TextureWrap| match w {
        TextureWrap::Clamp => 1.0,
        TextureWrap::MirroredRepeat => 2.0,
        _ => 0.0,
    };
    let origin = match t.origin {
        TextureOrigin::TopLeft => 1.0,
        _ => 0.0,
    };
    let (function, blend) = match t.function {
        TextureFunction::Modulate => (0.0, [0.0; 4]),
        TextureFunction::Blend { colour } => (2.0, colour.map(|c| c as f32)),
        TextureFunction::Decal => (3.0, [0.0; 4]),
        _ => (1.0, [0.0; 4]),
    };
    let [m0, m1] = t.uv_matrix.map(|r| r.map(|v| v as f32));
    let [wu, wv] = t.wrap.map(wrap);
    [
        [
            (offset % OFFSET_SPLIT) as f32,
            t.width as f32,
            t.height as f32,
            origin,
        ],
        [m0[0], m0[1], m0[2], wu],
        [m1[0], m1[1], m1[2], wv],
        [function, (offset / OFFSET_SPLIT) as f32, 0.0, 0.0],
        blend,
    ]
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, fill: u8) -> Texture {
        Texture {
            width,
            height,
            rgba: vec![fill; (width * height * 4) as usize],
            wrap: [TextureWrap::Clamp, TextureWrap::MirroredRepeat],
            function: TextureFunction::Blend {
                colour: [0.25, 0.5, 0.75, 1.0],
            },
            uv_matrix: [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            uv_set: 0,
            origin: TextureOrigin::TopLeft,
        }
    }

    #[test]
    fn a_shared_texture_is_packed_once_and_an_untextured_mesh_has_no_slot() {
        let (a, b) = (picture(3, 2, 7), picture(4, 1, 9));
        let paint = |texture| Paint {
            colour: [1, 2, 3, 4],
            texture,
        };
        let paints = [
            paint(Some(&a)),
            paint(None),
            paint(Some(&b)),
            paint(Some(&a)),
        ];
        let (pictures, materials, slots) = pack_pictures(&paints);
        assert_eq!(slots, [Some(0.0), None, Some(1.0), Some(0.0)]);
        assert_eq!((pictures.width, pictures.height), (2048, 1));
        assert!(pictures.texels[..24].iter().all(|&v| v == 7));
        assert!(pictures.texels[24..40].iter().all(|&v| v == 9));
        assert!(pictures.texels[40..].iter().all(|&v| v == 0));
        let m = &materials.texels;
        assert_eq!(
            m[..12],
            [0.0, 3.0, 2.0, 1.0, 1.0, 2.0, 3.0, 1.0, 4.0, 5.0, 6.0, 2.0]
        );
        assert_eq!(m[12..20], [2.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(m[20], 6.0);
    }

    #[test]
    fn a_picture_past_the_budget_falls_back_to_no_slot() {
        let big = picture(FLAT_WIDTH as u32, (FLAT_WIDTH * MAX_LAYERS) as u32 + 1, 1);
        let paints = [Paint {
            colour: [0; 4],
            texture: Some(&big),
        }];
        let (pictures, _, slots) = pack_pictures(&paints);
        assert_eq!(slots, [None]);
        assert_eq!((pictures.height, pictures.layers), (1, 1));
    }

    #[test]
    fn pictures_past_one_layer_fill_whole_layers_and_split_the_offset() {
        let (a, b) = (picture(2048, 2048, 3), picture(2048, 4096, 5));
        let paint = |texture| Paint {
            colour: [0; 4],
            texture,
        };
        let paints = [paint(Some(&a)), paint(Some(&b)), paint(Some(&a))];
        let (pictures, materials, slots) = pack_pictures(&paints);
        assert_eq!(slots, [Some(0.0), Some(1.0), Some(0.0)]);
        assert_eq!((pictures.height, pictures.layers), (2048, 3));
        assert_eq!(pictures.texels.len(), LAYER * 3 * 4);
        let m = &materials.texels;
        assert_eq!((m[20], m[33]), (LAYER as f32, 0.0));
        assert_eq!(LAYER, OFFSET_SPLIT / 4);
    }
}
