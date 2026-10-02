//! The poster a `/3D` annotation shows before activation and in readers
//! without 3D (ISO 32000-1 §13.6.2 Table 298 `/AP`): supplied, rendered from
//! the model, or pdfcer's placeholder, and why.

use std::fmt;

use super::{ThreeDFormat, ThreeDSpec};
use crate::image_import::ImportedImage;
use crate::object::ObjId;

/// Which poster [`crate::edit::EditSession::add_3d_annotation`] drew.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreeDPoster {
    /// [`ThreeDSpec::poster`] was supplied and drawn.
    Supplied,
    /// pdfcer rendered the model from its default view
    /// (`pdfcer_3d::DEFAULT_VIEW_DIRECTION`). This is an inference — the
    /// file's own views and lighting are not read — and a caller discloses
    /// it.
    Rendered(RenderedPoster),
    /// The placeholder (a frame and a wireframe cube in
    /// [`ThreeDSpec::color`]) was drawn.
    Placeholder(PlaceholderReason),
}

/// What a [`ThreeDPoster::Rendered`] poster shows, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderedPoster {
    /// The image's width in pixels.
    pub width: u32,
    /// The image's height in pixels.
    pub height: u32,
    /// Meshes drawn (a colour-split part counts once per colour).
    pub meshes: usize,
    /// Triangles drawn.
    pub triangles: usize,
    /// Wire tessellations not drawn.
    pub wires_skipped: usize,
    /// Markup tessellations not drawn.
    pub markups_skipped: usize,
    /// Compressed meshes rebuilt by pdfcer's reconstruction and drawn.
    pub compressed_rebuilt: usize,
    /// Compressed meshes left out.
    pub compressed_skipped: usize,
    /// Meshes the model tree gave no colour, drawn grey.
    pub uncoloured_meshes: usize,
    /// Why part placements were not applied (each mesh then draws once in
    /// its own coordinates), or `None` when they were.
    pub unplaced: Option<String>,
}

/// Why [`ThreeDPoster::Placeholder`] was drawn instead of a rendered poster.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaceholderReason {
    /// [`ThreeDSpec::render_poster`] was `false`.
    Requested,
    /// pdfcer decodes only PRC; the model is in this format (its label).
    NotDecoded {
        /// The model format's label, e.g. `"U3D"`.
        format: String,
    },
    /// This build has no model decoder (`pdfcer-core` without its `3d`
    /// feature).
    NoDecoder,
    /// The PRC model could not be drawn.
    Undecodable {
        /// The decoder's or rasterizer's message.
        why: String,
    },
}

impl fmt::Display for PlaceholderReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Requested => f.write_str("the placeholder was requested"),
            Self::NotDecoded { format } => {
                write!(f, "pdfcer decodes only PRC models; this one is {format}")
            }
            Self::NoDecoder => f.write_str("this build has no 3D model decoder"),
            Self::Undecodable { why } => write!(f, "the model could not be drawn: {why}"),
        }
    }
}

/// What [`crate::edit::EditSession::set_3d_poster`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ThreeDPosterOutcome {
    /// The `/3D` annotation whose `/AP /N` now names the new appearance.
    pub annot_id: ObjId,
    /// The new appearance stream.
    pub appearance_id: ObjId,
    /// The poster's image XObject.
    pub poster_image_id: ObjId,
}

/// Rendered-poster pixels per point of the annotation rectangle.
const PIXELS_PER_POINT: f64 = 2.0;

/// The rendered poster's longest side, in pixels.
const MAX_POSTER_SIDE: f64 = 2048.0;

/// The poster pixel size for `spec`'s rectangle: [`PIXELS_PER_POINT`],
/// scaled down so the long side is at most [`MAX_POSTER_SIDE`], at least 1.
fn poster_size(spec: &ThreeDSpec) -> (u32, u32) {
    let w = (spec.rect.urx - spec.rect.llx).abs() * PIXELS_PER_POINT;
    let h = (spec.rect.ury - spec.rect.lly).abs() * PIXELS_PER_POINT;
    let scale = (MAX_POSTER_SIDE / w.max(h)).min(1.0);
    let px = |v: f64| (v * scale).round().clamp(1.0, MAX_POSTER_SIDE) as u32;
    (px(w), px(h))
}

/// The poster drawn when [`ThreeDSpec::poster`] is `None`: the model's
/// default view for a PRC model that meshes, else the reason it is not.
pub(crate) fn default_poster(
    spec: &ThreeDSpec,
) -> Result<(ImportedImage, RenderedPoster), PlaceholderReason> {
    if !spec.render_poster {
        return Err(PlaceholderReason::Requested);
    }
    if spec.format != ThreeDFormat::Prc {
        return Err(PlaceholderReason::NotDecoded {
            format: spec.format.label(),
        });
    }
    render(&spec.data, poster_size(spec))
}

#[cfg(feature = "3d")]
fn render(
    data: &[u8],
    (width, height): (u32, u32),
) -> Result<(ImportedImage, RenderedPoster), PlaceholderReason> {
    let undecodable = |why: String| PlaceholderReason::Undecodable { why };
    let model = pdfcer_3d::assemble(data).map_err(|err| undecodable(err.to_string()))?;
    let image = pdfcer_3d::render_default_view(&model, width, height)
        .map_err(|err| undecodable(err.to_string()))?;
    let rgb: Vec<u8> = image
        .rgba
        .chunks_exact(4)
        .flat_map(|px| px.iter().take(3).copied())
        .collect();
    let png = encode_rgb_png(image.width, image.height, &rgb)
        .map_err(|err| undecodable(err.to_string()))?;
    let imported = crate::image_import::import(&png).map_err(|err| undecodable(err.to_string()))?;
    let uncoloured = model.meshes.len() - model.colours.iter().flatten().count();
    Ok((
        imported,
        RenderedPoster {
            width: image.width,
            height: image.height,
            meshes: model.meshes.len(),
            triangles: model.triangles,
            wires_skipped: model.wires,
            markups_skipped: model.markups,
            compressed_rebuilt: model.rebuilt,
            compressed_skipped: model.compressed,
            uncoloured_meshes: uncoloured,
            unplaced: model.unplaced,
        },
    ))
}

#[cfg(not(feature = "3d"))]
fn render(
    _data: &[u8],
    _size: (u32, u32),
) -> Result<(ImportedImage, RenderedPoster), PlaceholderReason> {
    Err(PlaceholderReason::NoDecoder)
}

/// A non-interlaced 8-bit RGB PNG (ISO/IEC 15948 §11.2): IHDR, one zlib
/// IDAT whose rows each carry filter type 0, IEND.
#[cfg(feature = "3d")]
fn encode_rgb_png(width: u32, height: u32, rgb: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Write;
    let row = width as usize * 3;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgb.chunks_exact(row.max(1)) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw)?;
    let idat = z.finish()?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [(b"IHDR", &ihdr), (b"IDAT", &idat), (b"IEND", &Vec::new())] {
        let len = u32::try_from(body.len()).map_err(std::io::Error::other)?;
        png.extend_from_slice(&len.to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(body);
        png.extend_from_slice(kind);
        png.extend_from_slice(body);
        png.extend_from_slice(&crc.sum().to_be_bytes());
    }
    Ok(png)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::page_tree::Rect;

    fn spec(w: f64, h: f64) -> ThreeDSpec {
        let rect = Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 10.0 + w,
            ury: 10.0 + h,
        };
        ThreeDSpec::new(rect, b"PRC\x08\x00".to_vec()).expect("PRC signature")
    }

    #[test]
    fn the_poster_is_two_pixels_per_point_capped_at_2048() {
        assert_eq!(poster_size(&spec(180.0, 90.0)), (360, 180));
        assert_eq!(poster_size(&spec(4096.0, 1024.0)), (2048, 512));
        assert_eq!(poster_size(&spec(0.1, 0.1)), (1, 1));
    }

    #[test]
    fn the_reason_names_the_format_or_the_request() {
        let mut s = spec(10.0, 10.0);
        s.render_poster = false;
        assert_eq!(default_poster(&s).err(), Some(PlaceholderReason::Requested));
        s.render_poster = true;
        s.format = ThreeDFormat::U3d;
        let why = default_poster(&s).err();
        assert_eq!(
            why,
            Some(PlaceholderReason::NotDecoded {
                format: "U3D".into()
            })
        );
    }

    #[cfg(feature = "3d")]
    #[test]
    fn the_png_encoder_round_trips_through_import() {
        let png = encode_rgb_png(2, 1, &[255, 0, 0, 0, 0, 255]).expect("encodes");
        let image = crate::image_import::import(&png).expect("imports");
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.bits_per_component, 8);
    }
}
