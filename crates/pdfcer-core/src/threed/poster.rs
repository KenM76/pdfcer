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
    /// Of those, meshes only a best-fit search rebuilt
    /// (`pdfcer_3d::AssembledModel::best_fit`); disclose them.
    pub compressed_best_fit: usize,
    /// Compressed meshes left out.
    pub compressed_skipped: usize,
    /// Meshes the model tree gave no colour, drawn grey.
    pub uncoloured_meshes: usize,
    /// Meshes drawn opaque because their material's diffuse alpha of 0.0,
    /// under a style stating no transparency, was read as unset
    /// (`pdfcer_3d::StyleAlpha::ZeroUnset`).
    pub alpha_unset_meshes: usize,
    /// Meshes drawn with their texture picture.
    pub textured_meshes: usize,
    /// Each reason a texture drew its base colour instead, with how many
    /// placed meshes it applied to.
    pub texture_notes: Vec<(String, usize)>,
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
    // The poster is opaque: the renderer's background is the page colour.
    let opaque: Vec<u8> = image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, _]| [r, g, b, 255])
        .collect();
    let imported =
        crate::image_import::ImportedImage::from_rgba8(image.width, image.height, &opaque)
            .map_err(|err| undecodable(err.to_string()))?;
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
            compressed_best_fit: model.best_fit,
            compressed_skipped: model.compressed,
            uncoloured_meshes: uncoloured,
            alpha_unset_meshes: model.alpha_unset,
            textured_meshes: model.textured,
            texture_notes: model.texture_notes,
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
}
