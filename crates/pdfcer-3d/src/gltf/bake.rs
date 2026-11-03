//! glTF maps PRC cannot carry, folded into the base-colour texture at
//! conversion: occlusion [glTF 2.0 §3.9.3] and the alpha-mask cut-out
//! [§3.9.4].
//!
//! PRC's texture definitions name a diffuse map and environment maps only;
//! no ambient or occlusion procedure has a published value
//! (`prc__8137__graphics_materials.md` §14.0). So an occlusion map is
//! multiplied into the base colour texels, which every reader draws.

/// How a material's alpha is read [§3.9.4].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Alpha {
    Opaque,
    Blend,
    /// `factor` is the base colour factor's alpha.
    Mask {
        cutoff: f64,
        factor: f64,
    },
}

/// What to bake: a base-colour picture, an occlusion picture with its
/// `strength`, or both over the same texture coordinates.
#[derive(Debug, Clone, Copy)]
pub(super) struct Recipe<'a> {
    pub(super) base: Option<&'a [u8]>,
    pub(super) occlusion: Option<(&'a [u8], f64)>,
    pub(super) alpha: Alpha,
}

impl Recipe<'_> {
    /// Whether the baked picture keeps an alpha channel: blended, or a
    /// mask cut out by a base picture's alpha.
    pub(super) fn keeps_alpha(&self) -> bool {
        match self.alpha {
            Alpha::Opaque => false,
            Alpha::Blend => true,
            Alpha::Mask { .. } => self.base.is_some(),
        }
    }
}

/// The recipe as a PNG file, or why it cannot be made.
#[cfg(not(feature = "textures"))]
pub(super) fn bake(r: &Recipe<'_>) -> Result<Vec<u8>, &'static str> {
    // The field is read only by the decoding build.
    let _ = r.occlusion;
    Err("this build decodes no texture pictures")
}

/// The recipe as a PNG file, or why it cannot be made. The result has the
/// larger of the two pictures' widths and heights (the base picture's when
/// that exceeds [`crate::MAX_TEXTURE_PIXELS`]); each is sampled
/// bilinearly at the same (u, v).
#[cfg(feature = "textures")]
pub(super) fn bake(r: &Recipe<'_>) -> Result<Vec<u8>, &'static str> {
    let base = r.base.map(decode).transpose()?;
    let occlusion = match r.occlusion {
        Some((bytes, strength)) => Some((decode(bytes)?, strength.clamp(0.0, 1.0))),
        None => None,
    };
    let size = |t: &crate::Texture| (t.width, t.height);
    let (w, h) = match (
        base.as_ref().map(size),
        occlusion.as_ref().map(|o| size(&o.0)),
    ) {
        (Some(b), Some(o)) => {
            let both = (b.0.max(o.0), b.1.max(o.1));
            let fits = u64::from(both.0) * u64::from(both.1) <= crate::MAX_TEXTURE_PIXELS;
            if fits { both } else { b }
        }
        (Some(s), None) | (None, Some(s)) => s,
        (None, None) => return Err("there is no picture to bake"),
    };
    let channels = if r.keeps_alpha() { 4 } else { 3 };
    let mut out = Vec::with_capacity(w as usize * h as usize * channels);
    for y in 0..h {
        for x in 0..w {
            let uv = [
                (f64::from(x) + 0.5) / f64::from(w),
                (f64::from(y) + 0.5) / f64::from(h),
            ];
            let texel = base.as_ref().map_or([255; 4], |b| b.sample(uv));
            let shade = occlusion.as_ref().map_or(1.0, |(o, s)| {
                let ao = f64::from(o.sample(uv)[0]) / 255.0;
                1.0 + s * (ao - 1.0)
            });
            out.extend(baked_texel(texel, shade, r.alpha).iter().take(channels));
        }
    }
    encode(w, h, &out, channels)
}

/// sRGB `texel` with its colour multiplied by `shade` in linear light, and
/// its alpha thresholded when `alpha` is a mask.
#[cfg(feature = "textures")]
fn baked_texel(texel: [u8; 4], shade: f64, alpha: Alpha) -> [u8; 4] {
    let mut out = texel;
    for c in out.iter_mut().take(3) {
        let lit = super::build::srgb(linear(f64::from(*c) / 255.0) * shade);
        *c = (lit * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    if let (Alpha::Mask { cutoff, factor }, Some(a)) = (alpha, out.get_mut(3)) {
        let shown = factor * f64::from(*a) / 255.0 >= cutoff;
        *a = if shown { 255 } else { 0 };
    }
    out
}

/// IEC 61966-2-1 sRGB decoding of an encoded component.
#[cfg(feature = "textures")]
pub(super) fn linear(c: f64) -> f64 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A PNG or JPEG file decoded for sampling, top row first, clamped.
#[cfg(feature = "textures")]
fn decode(bytes: &[u8]) -> Result<crate::Texture, &'static str> {
    use crate::{Texture, TextureFunction, TextureOrigin, TextureWrap};
    let (format, w, h) = crate::write::picture_info(bytes).ok_or("it is not a PNG or JPEG file")?;
    let (width, height, rgba) = crate::texture::decode_picture(format, bytes, w, h)
        .map_err(|_| "its picture does not decode within the texture ceiling")?;
    Ok(Texture {
        width,
        height,
        rgba,
        wrap: [TextureWrap::Clamp; 2],
        function: TextureFunction::Replace,
        uv_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        uv_set: 0,
        origin: TextureOrigin::TopLeft,
    })
}

/// 8-bit RGB (`channels` 3) or RGBA (4) samples as a PNG file.
#[cfg(feature = "textures")]
fn encode(w: u32, h: u32, samples: &[u8], channels: usize) -> Result<Vec<u8>, &'static str> {
    const BAD: &str = "the baked picture could not be written";
    let mut png = Vec::new();
    let mut e = png::Encoder::new(&mut png, w, h);
    e.set_color(if channels == 4 {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    e.set_depth(png::BitDepth::Eight);
    let mut writer = e.write_header().map_err(|_| BAD)?;
    writer.write_image_data(samples).map_err(|_| BAD)?;
    writer.finish().map_err(|_| BAD)?;
    Ok(png)
}

#[cfg(all(test, feature = "textures"))]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
        encode(w, h, rgba, 4).unwrap()
    }

    fn pixels(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let (f, w, h) = crate::write::picture_info(png).unwrap();
        crate::texture::decode_picture(f, png, w, h).unwrap()
    }

    #[test]
    fn occlusion_darkens_in_linear_light_by_strength() {
        // White base; occlusion R = 0 left, 255 right (G and B ignored).
        let base = png_of(1, 1, &[255, 255, 255, 255]);
        let ao = png_of(2, 1, &[0, 255, 255, 255, 255, 0, 0, 255]);
        let r = Recipe {
            base: Some(&base),
            occlusion: Some((&ao, 0.5)),
            alpha: Alpha::Opaque,
        };
        let (w, h, px) = pixels(&bake(&r).unwrap());
        assert_eq!((w, h), (2, 1), "the larger picture's size");
        // Left texel centre u = 0.25 samples AO between 0 and 255 bilinearly
        // at the edge clamp: AO 0, factor 1 + 0.5(0 - 1) = 0.5 linear.
        let half = (super::super::build::srgb(0.5) * 255.0).round() as u8;
        assert_eq!(px[0], half, "{px:?}");
        assert_eq!(&px[4..7], &[255, 255, 255], "AO 1 leaves the colour");
        assert_eq!(px[3], 255, "opaque");
    }

    #[test]
    fn a_mask_thresholds_the_texel_alpha_against_the_factor() {
        let base = png_of(2, 1, &[9, 9, 9, 200, 9, 9, 9, 100]);
        let r = Recipe {
            base: Some(&base),
            occlusion: None,
            alpha: Alpha::Mask {
                cutoff: 0.5,
                factor: 0.8,
            },
        };
        let (_, _, px) = pixels(&bake(&r).unwrap());
        // 0.8 × 200/255 = 0.63 shown; 0.8 × 100/255 = 0.31 cut.
        assert_eq!((px[3], px[7]), (255, 0), "{px:?}");
        assert_eq!(&px[..3], &[9, 9, 9], "colour unchanged without occlusion");
    }

    #[test]
    fn occlusion_alone_bakes_a_grey_picture() {
        let ao = png_of(1, 1, &[0, 0, 0, 255]);
        let r = Recipe {
            base: None,
            occlusion: Some((&ao, 1.0)),
            alpha: Alpha::Mask {
                cutoff: 0.5,
                factor: 1.0,
            },
        };
        assert!(!r.keeps_alpha(), "no base picture: nothing to cut out");
        let (_, _, px) = pixels(&bake(&r).unwrap());
        assert_eq!(px, [0, 0, 0, 255]);
    }

    #[test]
    fn an_undecodable_picture_is_refused() {
        let r = Recipe {
            base: Some(b"GIF89a"),
            occlusion: None,
            alpha: Alpha::Blend,
        };
        assert_eq!(bake(&r), Err("it is not a PNG or JPEG file"));
    }
}
