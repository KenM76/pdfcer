//! Measure and correct the skew of a placed scan image (pdfcer-gui request
//! G165). The image analysis is in [`crate::deskew`].

use super::{CommandKind, EditError, EditSession, ObjectWrite};
use crate::deskew::{MAX_SKEW_DEGREES, Raster, SkewEstimate};
use crate::object::{Dict, ObjId, Object, Stream};
use crate::page_tree::{self, Page};
use crate::redact_image::{Allocator, Decoded, ImageOutcome};
use crate::vector::{ImageObject, ImageSource, Matrix, PageObjects, VectorObject, decompose_page};
use crate::view::DocumentView;

/// The smallest correction [`EditSession::deskew_image`] performs; a smaller
/// one would resample every pixel for no visible change.
pub const MIN_DESKEW_DEGREES: f64 = 0.01;

/// What [`EditSession::deskew_image`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImageDeskew {
    /// The new, straightened image XObject.
    pub image_id: ObjId,
    /// The `/XObject` resource name the page's content now invokes.
    pub resource_name: Vec<u8>,
    /// The image XObject no longer drawn at this place. It stays in the
    /// file: other placements may draw it, and an incremental save keeps
    /// its bytes.
    pub replaced: ObjId,
    /// The correction applied, in degrees: the content was rotated this far
    /// clockwise as displayed ([`crate::deskew`] on the sign).
    pub angle_degrees: f64,
    /// The encoded size of the old image stream, in bytes.
    pub old_stream_bytes: usize,
    /// The encoded size of the new one. It is `FlateDecode`, so a scan that
    /// was JPEG- or JBIG2-coded usually grows.
    pub new_stream_bytes: usize,
    /// What the operator should know (rule 4): the pixels were resampled,
    /// and anything about the colour space or resources that changed.
    pub notes: Vec<String>,
}

/// A placed image decoded for analysis.
struct Target {
    placed: ImageObject,
    id: ObjId,
    stream: Stream,
    decoded: Decoded,
    raster: Raster,
    resources: Dict,
}

impl EditSession {
    /// The index into [`Self::page_objects`] of the page's largest image by
    /// area on the page — on a scanned page, the scan. `None` when the page
    /// draws no image (form XObjects are not counted).
    ///
    /// # Errors
    ///
    /// As [`Self::page_objects`].
    pub fn page_scan_image(&mut self, page_index: usize) -> Result<Option<usize>, EditError> {
        Ok(largest_image(&*self.page_objects(page_index)?))
    }

    /// **Measure the skew** of the image `object_index` on `page_index`
    /// (an index into [`Self::page_objects`]), in the image's own sample
    /// grid. Read-only.
    ///
    /// Returns `Ok(None)` when the image holds too little ink to measure.
    /// The angle is relative to the image's rows; an image placed rotated
    /// on the page is measured before that rotation.
    ///
    /// # Errors
    ///
    /// As [`Self::deskew_image`], except the angle checks.
    pub fn detect_image_skew(
        &mut self,
        page_index: usize,
        object_index: usize,
    ) -> Result<Option<SkewEstimate>, EditError> {
        let t = self.deskew_target(page_index, object_index)?;
        Ok(crate::deskew::detect_in_samples(
            &t.raster,
            &t.decoded.samples,
        ))
    }

    /// **Straighten a placed image**: rotate its samples `angle_degrees`
    /// clockwise as displayed about the image centre — the correction for a
    /// skew [`Self::detect_image_skew`] measured as `angle_degrees` — one
    /// undo entry ([`CommandKind::DeskewImage`]).
    ///
    /// The result is a **new** image XObject of the same size, colour space,
    /// `/Decode` and bit depth, `FlateDecode`-coded, drawn by a `Do` that
    /// replaces this placement's own; the CTM in force is unchanged, so the
    /// image stays where it was on the page. Other placements of the old
    /// XObject keep drawing it. Corners the rotation uncovers take the
    /// image's background value ([`crate::deskew`]).
    ///
    /// # Errors
    ///
    /// - [`EditError::DeskewUnsupported`] for an inline image, an image with
    ///   `/SMask`, `/Mask` or embedded alpha, an image whose samples cannot
    ///   be decoded, a non-finite angle, one beyond ±[`MAX_SKEW_DEGREES`], or
    ///   one smaller than [`MIN_DESKEW_DEGREES`].
    /// - [`EditError::ReplaceImageOnOther`] when the object is a path, text
    ///   or a form XObject.
    /// - [`crate::vector::VectorEditError::ObjectOutOfRange`],
    ///   [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   [`EditError::DocumentEncrypted`],
    ///   [`EditError::CertificationForbidsChange`],
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    ///   [`EditError::ObjectNumbersExhausted`].
    pub fn deskew_image(
        &mut self,
        page_index: usize,
        object_index: usize,
        angle_degrees: f64,
    ) -> Result<ImageDeskew, EditError> {
        check_angle(angle_degrees)?;
        let t = self.deskew_target(page_index, object_index)?;
        let rotated = crate::deskew::rotate_samples(&t.raster, &t.decoded.samples, angle_degrees)
            .ok_or_else(|| {
            unsupported("its samples are shorter than its dimensions require")
        })?;
        let mut prior = Vec::new();
        let (image_id, new_stream_bytes, mut notes) =
            self.stage_deskewed(&t, &rotated, &mut prior)?;
        let resource_name = self.repoint_image_placement(
            CommandKind::DeskewImage,
            page_index,
            t.placed.bytes,
            image_id,
            prior,
            |name| {
                let mut out = vec![b'/'];
                // `free_name_in` names are regular-character ASCII (§7.3.5).
                out.extend_from_slice(name);
                out.extend_from_slice(b" Do");
                out
            },
        )?;
        notes.insert(
            0,
            format!(
                "deskew: the image's pixels were resampled to rotate it {angle_degrees}° clockwise"
            ),
        );
        Ok(ImageDeskew {
            image_id,
            resource_name,
            replaced: t.id,
            angle_degrees,
            old_stream_bytes: t.stream.data_span.len,
            new_stream_bytes,
            notes,
        })
    }

    /// Stage the straightened image (and any `/ICCBased` stream its
    /// dictionary rewrite needs) into `prior`; returns its id, its encoded
    /// size and the rewrite's notes.
    fn stage_deskewed(
        &mut self,
        t: &Target,
        rotated: &[u8],
        prior: &mut Vec<ObjectWrite>,
    ) -> Result<(ObjId, usize, Vec<String>), EditError> {
        let image_id = ObjId::new(self.alloc_number()?, 0);
        let mut next = self.next_number.ok_or(EditError::ObjectNumbersExhausted)?;
        let mut out = ImageOutcome::default();
        let base_len = self.base.bytes().len();
        let mut alloc = Allocator {
            staging: &mut self.staging,
            base_len,
            next_num: &mut next,
        };
        let dict = crate::redact_image::rewrite_dict(
            &t.stream.dict,
            &t.decoded,
            &t.resources,
            &mut alloc,
            &mut out,
        );
        let image = alloc.flate_stream(dict, rotated);
        self.next_number = Some(next);
        let new_bytes = match &image {
            Object::Stream(s) => s.data_span.len,
            _ => 0,
        };
        for (id, object) in out.objects.into_iter().chain([(image_id, image)]) {
            prior.push(ObjectWrite {
                id,
                before: None,
                after: Some(object),
            });
        }
        let notes = out
            .notes
            .into_iter()
            .map(|n| n.replacen("redaction:", "deskew:", 1))
            .collect();
        Ok((image_id, new_bytes, notes))
    }

    /// The image placement, decoded, after every refusal that does not
    /// depend on the angle.
    fn deskew_target(
        &mut self,
        page_index: usize,
        object_index: usize,
    ) -> Result<Target, EditError> {
        let placed = self.editable_placed_image(page_index, object_index)?;
        let pages = self.pages()?;
        let resources = pages
            .get(page_index)
            .map(|p| p.resources.clone())
            .unwrap_or_default();
        decode_target(&self.view(), placed, resources)
    }
}

/// The page's largest placed image by area on the page, `None` when it
/// draws none (form XObjects are not counted).
fn largest_image(model: &PageObjects) -> Option<usize> {
    let area = |img: &ImageObject| (img.ctm.a * img.ctm.d - img.ctm.b * img.ctm.c).abs();
    model
        .objects
        .iter()
        .enumerate()
        .filter_map(|(i, o)| match o {
            VectorObject::Image(img) if img.source != ImageSource::Form => Some((i, area(img))),
            _ => None,
        })
        .fold(None, |best: Option<(usize, f64)>, (i, a)| match best {
            Some((_, b)) if b >= a => best,
            _ => Some((i, a)),
        })
        .map(|(i, _)| i)
}

/// The page `page_index` of `view` and its decomposition, as
/// [`EditSession::page_objects`] builds it on the session's own view.
fn view_page_objects(
    view: &DocumentView<'_>,
    page_index: usize,
) -> Result<(Page, PageObjects), EditError> {
    let mut pages = page_tree::pages_in(view.graph())?;
    let count = pages.len();
    if page_index >= count {
        return Err(EditError::PageOutOfRange {
            index: page_index,
            count,
        });
    }
    let page = pages.swap_remove(page_index);
    if page.contents.is_empty() {
        return Err(EditError::VectorEditNoContents { page_index });
    }
    let model =
        decompose_page(view, &page, Matrix::IDENTITY).map_err(EditError::VectorEditContent)?;
    Ok((page, model))
}

/// [`EditSession::page_scan_image`] on any [`DocumentView`], read-only: the
/// index, into [`decompose_page`]`(view, page, Matrix::IDENTITY)`, of the
/// page's largest image by area. On [`EditSession::view`] it is the index
/// [`EditSession::page_objects`] uses.
///
/// # Errors
///
/// [`EditError::PageTree`], [`EditError::PageOutOfRange`],
/// [`EditError::VectorEditNoContents`], [`EditError::VectorEditContent`].
pub fn page_scan_image(
    view: &DocumentView<'_>,
    page_index: usize,
) -> Result<Option<usize>, EditError> {
    Ok(largest_image(&view_page_objects(view, page_index)?.1))
}

/// [`EditSession::detect_image_skew`] on any [`DocumentView`], read-only:
/// the same angle and confidence, so a shell can measure on a worker thread
/// and commit with [`EditSession::deskew_image`]. `object_index` is as in
/// [`page_scan_image`].
///
/// The session method's edit refusals (encryption permissions,
/// certification, hidden objects) are not applied: they guard the
/// correction, not the measurement.
///
/// # Errors
///
/// As [`page_scan_image`], plus [`EditError::DeskewUnsupported`] and
/// [`EditError::ReplaceImageOnOther`] and
/// [`crate::vector::VectorEditError::ObjectOutOfRange`] as
/// [`EditSession::deskew_image`] reports them.
pub fn detect_image_skew(
    view: &DocumentView<'_>,
    page_index: usize,
    object_index: usize,
) -> Result<Option<SkewEstimate>, EditError> {
    let (page, model) = view_page_objects(view, page_index)?;
    let placed = super::image_replace::placed_image(&model, object_index)?;
    let t = decode_target(view, placed, page.resources)?;
    Ok(crate::deskew::detect_in_samples(
        &t.raster,
        &t.decoded.samples,
    ))
}

/// `placed`, decoded for analysis, after every refusal that does not depend
/// on the angle.
fn decode_target(
    view: &DocumentView<'_>,
    placed: ImageObject,
    resources: Dict,
) -> Result<Target, EditError> {
    let id = match (placed.source, placed.xobject) {
        (ImageSource::XObject, Some(id)) => id,
        _ => return Err(unsupported("it is an inline image")),
    };
    let Some(Object::Stream(stream)) = view.graph().value(id).cloned() else {
        return Err(unsupported("its XObject is not a stream"));
    };
    for key in [&b"SMask"[..], b"Mask"] {
        if stream.dict.get(key).is_some() {
            let key = String::from_utf8_lossy(key);
            return Err(unsupported(&format!(
                "it has a /{key}, which would no longer line up"
            )));
        }
    }
    let raw = view
        .slice(stream.data_span)
        .ok_or_else(|| unsupported("its stream data lies outside the file"))?;
    let decoded = crate::redact_image::decode(view, &stream.dict, raw, false, &resources)
        .map_err(|why| unsupported(&why))?;
    if decoded.embedded_alpha.is_some() || decoded.preblended_alpha_dropped {
        return Err(unsupported(
            "its codestream carries transparency, which would not survive",
        ));
    }
    let raster = Raster {
        width: decoded.width,
        height: decoded.height,
        components: decoded.components,
        bpc: decoded.bpc,
        paper_is_ones: decoded.paper,
        indexed: is_indexed(view, &stream.dict, &resources),
    };
    Ok(Target {
        placed,
        id,
        stream,
        decoded,
        raster,
        resources,
    })
}

fn unsupported(reason: &str) -> EditError {
    EditError::DeskewUnsupported {
        reason: reason.to_owned(),
    }
}

fn check_angle(angle: f64) -> Result<(), EditError> {
    if !angle.is_finite() || angle.abs() > MAX_SKEW_DEGREES {
        return Err(unsupported(&format!(
            "the angle {angle}° is outside ±{MAX_SKEW_DEGREES}°"
        )));
    }
    if angle.abs() < MIN_DESKEW_DEGREES {
        return Err(unsupported(&format!(
            "the angle {angle}° is below {MIN_DESKEW_DEGREES}°; there is nothing to correct"
        )));
    }
    Ok(())
}

/// Whether the image's `/ColorSpace` (directly, or by name through the
/// page's `/ColorSpace` resources) is `/Indexed` (§8.6.6.3).
fn is_indexed(view: &DocumentView<'_>, dict: &Dict, resources: &Dict) -> bool {
    let g = view.graph();
    let mut cs = dict.get(b"ColorSpace").map(|o| g.resolve(o));
    // One level of naming is all §8.6.3 allows for an image's space.
    if let Some(Object::Name(name)) = cs {
        let named = g
            .resolve(resources.get(b"ColorSpace").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|d| d.get(name.as_bytes()))
            .map(|o| g.resolve(o));
        if named.is_some() {
            cs = named;
        }
    }
    match cs {
        Some(Object::Name(n)) => matches!(n.as_bytes(), b"Indexed" | b"I"),
        Some(Object::Array(items)) => items
            .first()
            .map(|o| g.resolve(o))
            .and_then(|o| match o {
                Object::Name(n) => Some(matches!(n.as_bytes(), b"Indexed" | b"I")),
                _ => None,
            })
            .unwrap_or(false),
        _ => false,
    }
}
