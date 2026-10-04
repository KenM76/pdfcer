//! Texture pictures decoded for drawing, and how a textured surface samples
//! them [WD 7.5.5, 7.5.7; `prc__8137__graphics_materials.md` §13].

/// The most pixels pdfcer decodes from one texture picture.
pub const MAX_TEXTURE_PIXELS: u64 = 1 << 24;

/// How texture coordinates outside 0–1 fold back [WD 7.5.7]. Clamp-to-edge
/// and clamp-to-border both draw as [`Self::Clamp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TextureWrap {
    /// The picture tiles. Also what an unknown mode draws as.
    #[default]
    Repeat,
    /// Coordinates are held to the picture's edge.
    Clamp,
    /// The picture tiles, every other copy mirrored.
    MirroredRepeat,
}

/// How a texel combines with the surface's base material colour
/// [WD 7.5.7], as OpenGL's texture environment defines each.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum TextureFunction {
    /// Base colour × texel.
    Modulate,
    /// The texel alone. Also what an unknown function draws as.
    #[default]
    Replace,
    /// Base colour mixed toward `colour` by the texel.
    Blend {
        /// The blend colour, RGBA 0–1.
        colour: [f64; 4],
    },
    /// The texel laid over the base colour by its own alpha.
    Decal,
}

/// Which picture row texture coordinate v = 0 names. ISO 14739-1 does not
/// say; OpenGL's convention, which PRC's texture model follows, is the
/// bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TextureOrigin {
    /// v = 0 is the picture's bottom row.
    #[default]
    BottomLeft,
    /// v = 0 is the picture's top row.
    TopLeft,
}

/// A decoded texture, ready to sample.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Texture {
    /// Width, pixels.
    pub width: u32,
    /// Height, pixels.
    pub height: u32,
    /// Straight RGBA, row by row from the top of the picture. Channels the
    /// texture definition does not supply are 255.
    pub rgba: Vec<u8>,
    /// Wrapping along u, then v.
    pub wrap: [TextureWrap; 2],
    /// How a texel meets the base colour.
    pub function: TextureFunction,
    /// Stored (u, v) to picture (u, v): `u' = m[0][0]·u + m[0][1]·v +
    /// m[0][2]`, `v'` likewise from `m[1]` [WD 7.5.8].
    pub uv_matrix: [[f64; 3]; 2],
    /// Which of the mesh's texture-coordinate sets the texture reads, an
    /// index into [`crate::TriangleMesh::triangle_uvs`].
    pub uv_set: usize,
    /// Which row v = 0 names.
    pub origin: TextureOrigin,
}

impl Texture {
    /// Whether every texel is opaque.
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        self.rgba.chunks_exact(4).all(|p| p.get(3) == Some(&255))
    }

    /// The texel at stored coordinates `uv`, bilinearly filtered, after
    /// [`Self::uv_matrix`], [`Self::wrap`] and [`Self::origin`]; transparent
    /// black for an empty picture.
    #[must_use]
    pub fn sample(&self, uv: [f64; 2]) -> [u8; 4] {
        let [u, v] = uv;
        let [mu, mv] = self.uv_matrix;
        let row = |m: [f64; 3]| m[0] * u + m[1] * v + m[2];
        let (u, v) = (row(mu), row(mv));
        let v = match self.origin {
            TextureOrigin::BottomLeft => 1.0 - v,
            TextureOrigin::TopLeft => v,
        };
        let (w, h) = (self.width as usize, self.height as usize);
        if w == 0 || h == 0 || !u.is_finite() || !v.is_finite() {
            return [0; 4];
        }
        let x = u * w as f64 - 0.5;
        let y = v * h as f64 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let [wu, wv] = self.wrap;
        let texel = |xi: f64, yi: f64| -> [f64; 4] {
            let (i, j) = (fold(xi, w, wu), fold(yi, h, wv));
            let k = (j * w + i) * 4;
            match self.rgba.get(k..k + 4) {
                Some([r, g, b, a]) => [*r, *g, *b, *a].map(f64::from),
                _ => [0.0; 4],
            }
        };
        let (a, b) = (texel(x0, y0), texel(x0 + 1.0, y0));
        let (c, d) = (texel(x0, y0 + 1.0), texel(x0 + 1.0, y0 + 1.0));
        let mut out = [0u8; 4];
        for (k, o) in out.iter_mut().enumerate() {
            let at = |p: [f64; 4]| p.get(k).copied().unwrap_or(0.0);
            let top = at(a) + (at(b) - at(a)) * fx;
            let bottom = at(c) + (at(d) - at(c)) * fx;
            *o = (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8;
        }
        out
    }

    /// `base` (straight RGBA 0–255) combined with `texel` by
    /// [`Self::function`]. The result's alpha is the base alpha × the
    /// texel alpha, except where the function ignores the texel's alpha.
    #[must_use]
    pub fn apply(&self, base: [u8; 4], texel: [u8; 4]) -> [u8; 4] {
        let b = base.map(|c| f64::from(c) / 255.0);
        let t = texel.map(|c| f64::from(c) / 255.0);
        let ch = |p: [f64; 4], k: usize| p.get(k).copied().unwrap_or(0.0);
        let mut out = [0.0; 4];
        for (k, o) in out.iter_mut().enumerate().take(3) {
            let (bk, tk) = (ch(b, k), ch(t, k));
            *o = match self.function {
                TextureFunction::Modulate => bk * tk,
                TextureFunction::Replace => tk,
                TextureFunction::Blend { colour } => bk * (1.0 - tk) + ch(colour, k) * tk,
                TextureFunction::Decal => bk * (1.0 - ch(t, 3)) + tk * ch(t, 3),
            };
        }
        if let Some(a) = out.get_mut(3) {
            *a = match self.function {
                TextureFunction::Decal => ch(b, 3),
                _ => ch(b, 3) * ch(t, 3),
            };
        }
        out.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

/// Integer texel coordinate `i` folded into `0..n` by `wrap`.
fn fold(i: f64, n: usize, wrap: TextureWrap) -> usize {
    let n_f = n as f64;
    let folded = match wrap {
        TextureWrap::Repeat => i.rem_euclid(n_f),
        TextureWrap::Clamp => i.clamp(0.0, n_f - 1.0),
        TextureWrap::MirroredRepeat => {
            let m = i.rem_euclid(2.0 * n_f);
            if m < n_f { m } else { 2.0 * n_f - 1.0 - m }
        }
    };
    (folded as usize).min(n - 1)
}

/// Why a picture was not decoded.
pub(crate) type Undecoded = &'static str;

/// Picture `bytes` of stored `format` decoded to straight RGBA, with its
/// size [WD 7.5.5.2]: 0 PNG, 1 JPEG (stored size ignored), 2-5 zlib of raw
/// 8-bit RGB, RGBA, grey, grey + alpha at the stored size.
pub(crate) fn decode_picture(
    format: u32,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(u32, u32, Vec<u8>), Undecoded> {
    match format {
        0 => png(bytes),
        1 => jpeg(bytes),
        2..=5 => {
            let components = [3, 4, 1, 2].get(format as usize - 2).copied().unwrap_or(3);
            raw(bytes, width, height, components)
        }
        _ => Err("base colour drawn: an unknown picture format"),
    }
}

fn check_size(width: u32, height: u32) -> Result<usize, Undecoded> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 {
        return Err("base colour drawn: an empty picture");
    }
    if pixels > MAX_TEXTURE_PIXELS {
        return Err("base colour drawn: a picture larger than the texture ceiling");
    }
    Ok(pixels as usize)
}

/// Pixels of `components` 8-bit channels (grey, grey + alpha, RGB, RGBA)
/// widened to RGBA.
fn to_rgba(samples: &[u8], components: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() / components.max(1) * 4);
    for p in samples.chunks_exact(components.max(1)) {
        out.extend_from_slice(&match *p {
            [g] => [g, g, g, 255],
            [g, a] => [g, g, g, a],
            [r, g, b] => [r, g, b, 255],
            [r, g, b, a] => [r, g, b, a],
            _ => [0, 0, 0, 255],
        });
    }
    out
}

fn raw(
    bytes: &[u8],
    width: u32,
    height: u32,
    components: usize,
) -> Result<(u32, u32, Vec<u8>), Undecoded> {
    use std::io::Read;
    let need = check_size(width, height)? * components;
    let mut samples = Vec::with_capacity(need);
    flate2::read::ZlibDecoder::new(bytes)
        .take(need as u64)
        .read_to_end(&mut samples)
        .map_err(|_| "base colour drawn: a picture whose zlib data is damaged")?;
    if samples.len() < need {
        return Err("base colour drawn: a picture with fewer pixels than its size");
    }
    Ok((width, height, to_rgba(&samples, components)))
}

#[cfg(feature = "textures")]
fn png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Undecoded> {
    const BAD: Undecoded = "base colour drawn: a PNG picture that does not decode";
    let limits = png::Limits {
        bytes: (MAX_TEXTURE_PIXELS * 8) as usize,
    };
    let mut d = png::Decoder::new_with_limits(bytes, limits);
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().map_err(|_| BAD)?;
    let (w, h) = r.info().size();
    check_size(w, h)?;
    let mut buf = vec![0; r.output_buffer_size()];
    let frame = r.next_frame(&mut buf).map_err(|_| BAD)?;
    let components = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(BAD),
    };
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(BAD);
    }
    buf.truncate(frame.buffer_size());
    let mut rows = Vec::with_capacity(w as usize * h as usize * components);
    for line in buf.chunks(frame.line_size).take(h as usize) {
        rows.extend(line.iter().take(w as usize * components));
    }
    Ok((w, h, to_rgba(&rows, components)))
}

#[cfg(feature = "textures")]
fn jpeg(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Undecoded> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    const BAD: Undecoded = "base colour drawn: a JPEG picture that does not decode";
    let cap = 1usize << 16;
    let options = |out: ColorSpace| {
        DecoderOptions::default()
            .set_max_width(cap)
            .set_max_height(cap)
            .jpeg_set_out_colorspace(out)
    };
    let mut probe = JpegDecoder::new_with_options(ZCursor::new(bytes), options(ColorSpace::RGB));
    probe.decode_headers().map_err(|_| BAD)?;
    let (out, components) = match probe.input_colorspace() {
        Some(ColorSpace::Luma) => (ColorSpace::Luma, 1),
        Some(ColorSpace::YCbCr | ColorSpace::RGB) => (ColorSpace::RGB, 3),
        _ => {
            return Err(
                "base colour drawn: a JPEG picture in CMYK or another unsupported colour space",
            );
        }
    };
    let (w, h) = probe.dimensions().ok_or(BAD)?;
    let (w, h) = (
        u32::try_from(w).map_err(|_| BAD)?,
        u32::try_from(h).map_err(|_| BAD)?,
    );
    check_size(w, h)?;
    let mut d = JpegDecoder::new_with_options(ZCursor::new(bytes), options(out));
    let samples = d.decode().map_err(|_| BAD)?;
    if samples.len() < w as usize * h as usize * components {
        return Err(BAD);
    }
    Ok((w, h, to_rgba(&samples, components)))
}

#[cfg(not(feature = "textures"))]
fn png(_: &[u8]) -> Result<(u32, u32, Vec<u8>), Undecoded> {
    Err("base colour drawn: a PNG picture, and this build has no PNG decoder")
}

#[cfg(not(feature = "textures"))]
fn jpeg(_: &[u8]) -> Result<(u32, u32, Vec<u8>), Undecoded> {
    Err("base colour drawn: a JPEG picture, and this build has no JPEG decoder")
}

/// Set to 255 every channel `channels` (`texture_mapping_attributes`:
/// 0x1 R, 0x2 G, 0x4 B, 0x8 A; 0 = all four) does not supply [WD 7.5.7].
pub(crate) fn keep_channels(rgba: &mut [u8], channels: u32) {
    let keep = if channels & 0xF == 0 { 0xF } else { channels };
    if keep & 0xF == 0xF {
        return;
    }
    for p in rgba.chunks_exact_mut(4) {
        for (k, c) in p.iter_mut().enumerate() {
            if keep & (1 << k) == 0 {
                *c = 255;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use std::io::Write as _;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// Red, green on the top row; blue, white below.
    fn quad() -> Texture {
        Texture {
            width: 2,
            height: 2,
            rgba: [RED, GREEN, BLUE, WHITE].concat(),
            wrap: [TextureWrap::Repeat; 2],
            function: TextureFunction::Replace,
            uv_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uv_set: 0,
            origin: TextureOrigin::BottomLeft,
        }
    }

    #[test]
    fn raw_pictures_widen_every_layout_to_rgba() {
        let cases: [(u32, &[u8], [u8; 4]); 4] = [
            (2, &[1, 2, 3], [1, 2, 3, 255]),
            (3, &[1, 2, 3, 4], [1, 2, 3, 4]),
            (4, &[9], [9, 9, 9, 255]),
            (5, &[9, 7], [9, 9, 9, 7]),
        ];
        for (format, pixel, want) in cases {
            let (w, h, rgba) = decode_picture(format, &zlib(pixel), 1, 1).unwrap();
            assert_eq!((w, h, rgba), (1, 1, want.to_vec()), "format {format}");
        }
        assert!(decode_picture(2, &zlib(&[1, 2]), 1, 1).is_err(), "short");
        assert!(decode_picture(2, b"not zlib", 1, 1).is_err(), "damaged");
        assert!(decode_picture(2, &zlib(&[0; 3]), 0, 1).is_err(), "empty");
        assert!(
            decode_picture(2, &zlib(&[0; 3]), 1 << 13, 1 << 12).is_err(),
            "ceiling"
        );
        assert!(decode_picture(6, &zlib(&[0; 3]), 1, 1).is_err(), "unknown");
    }

    #[cfg(feature = "textures")]
    #[test]
    fn png_pictures_decode_and_jpeg_garbage_is_refused() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 2, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[1, 2, 3, 4, 5, 6])
                .unwrap();
        }
        let (w, h, rgba) = decode_picture(0, &bytes, 0, 0).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
        assert!(decode_picture(0, b"\x89PNG broken", 0, 0).is_err());
        assert!(decode_picture(1, b"\xFF\xD8 broken", 0, 0).is_err());
    }

    #[test]
    fn channels_not_supplied_read_as_full() {
        let mut rgba = vec![10, 20, 30, 40];
        keep_channels(&mut rgba, 0);
        assert_eq!(rgba, [10, 20, 30, 40]);
        keep_channels(&mut rgba, 0x1 | 0x8);
        assert_eq!(rgba, [10, 255, 255, 40]);
    }

    #[test]
    fn v_zero_is_the_bottom_row_unless_the_origin_says_top() {
        let mut t = quad();
        assert_eq!(t.sample([0.25, 0.25]), BLUE);
        assert_eq!(t.sample([0.75, 0.75]), GREEN);
        t.origin = TextureOrigin::TopLeft;
        assert_eq!(t.sample([0.25, 0.25]), RED);
        assert_eq!(t.sample([0.75, 0.75]), WHITE);
        // Halfway between red and green, bilinearly.
        assert_eq!(t.sample([0.5, 0.25]), [128, 128, 0, 255]);
    }

    #[test]
    fn wrapping_and_the_uv_matrix_fold_coordinates_back() {
        let mut t = quad();
        t.origin = TextureOrigin::TopLeft;
        assert_eq!(t.sample([1.25, 0.25]), RED, "repeat");
        t.wrap = [TextureWrap::Clamp; 2];
        assert_eq!(t.sample([3.0, 0.25]), GREEN, "clamp");
        t.wrap = [TextureWrap::MirroredRepeat; 2];
        assert_eq!(t.sample([1.25, 0.25]), GREEN, "mirrored");
        assert_eq!(fold(-1.0, 4, TextureWrap::Repeat), 3);
        assert_eq!(fold(-1.0, 4, TextureWrap::MirroredRepeat), 0);
        assert_eq!(fold(4.0, 4, TextureWrap::MirroredRepeat), 3);
        t.wrap = [TextureWrap::Repeat; 2];
        t.uv_matrix = [[0.5, 0.0, 0.5], [0.0, 1.0, 0.0]];
        assert_eq!(t.sample([0.5, 0.25]), GREEN, "u' = u / 2 + 1/2");
    }

    #[test]
    fn each_function_meets_the_base_colour_its_own_way() {
        let mut t = quad();
        let base = [200, 100, 50, 128];
        let texel = [255, 0, 255, 128];
        assert_eq!(t.apply(base, texel), [255, 0, 255, 64], "replace");
        t.function = TextureFunction::Modulate;
        assert_eq!(t.apply(base, texel), [200, 0, 50, 64]);
        t.function = TextureFunction::Decal;
        assert_eq!(t.apply(base, texel), [228, 50, 153, 128]);
        t.function = TextureFunction::Blend {
            colour: [0.0, 1.0, 0.0, 1.0],
        };
        assert_eq!(t.apply(base, texel), [0, 100, 0, 64]);
        assert!(t.is_opaque());
        t.rgba[3] = 254;
        assert!(!t.is_opaque());
    }
}
