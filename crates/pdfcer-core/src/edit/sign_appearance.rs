//! A visible signature's `/AP /N`: the signer name, date, reason and
//! location (ISO 32000-1 §12.8.1 Table 252) and an optional image, built by
//! [`crate::widget_draw::SignatureAppearanceBuilder`] and staged as new
//! objects.

use super::{EditSession, ObjectWrite};
use crate::object::{Name, ObjId, Object, Stream};
use crate::sign::apply::{self, SignApplyError, SignRequest};
use crate::widget_draw::{SignatureAppearanceBuilder, SignatureImage, SignatureLabels};

/// What staging a visible signature's appearance produced.
pub(super) struct StagedSignatureAppearance {
    /// The `/AP /N` form XObject.
    pub ap_id: ObjId,
    /// The lines drawn, for [`apply::SignReport::appearance_lines`].
    pub lines: Vec<String>,
    /// Whether the request's image was drawn.
    pub image_drawn: bool,
}

impl EditSession {
    /// Stage the appearance of a `w` x `h` pt visible signature by
    /// `leaf_subject` into `objects`. Every line asked for is drawn or the
    /// signature is refused: a dropped reason or location is an overflow
    /// here, since the operator supplied it.
    pub(super) fn stage_signature_appearance(
        &mut self,
        leaf_subject: &str,
        request: &SignRequest,
        (w, h): (f64, f64),
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<StagedSignatureAppearance, SignApplyError> {
        let mut builder = SignatureAppearanceBuilder::new(
            apply::signer_display_name(leaf_subject),
            request.signing_time.as_str(),
        )
        .labels(SignatureLabels::Described);
        if let Some(r) = &request.reason {
            builder = builder.reason(r.as_str());
        }
        if let Some(l) = &request.location {
            builder = builder.location(l.as_str());
        }
        let overflow = || SignApplyError::AppearanceOverflow {
            lines: apply::appearance_lines(
                leaf_subject,
                &request.signing_time,
                request.reason.as_deref(),
                request.location.as_deref(),
            )
            .len(),
            width: w,
            height: h,
            min_size: apply::APPEARANCE_MIN_SIZE,
        };
        // Lay out against a stand-in id first, so a refusal stages nothing.
        let mut trial = builder.clone();
        if let Some(img) = &request.appearance_image {
            trial = trial.image(SignatureImage::new(ObjId::new(0, 0), img.width, img.height));
        }
        if !trial
            .build(w, h)
            .map_err(|_| overflow())?
            .dropped
            .is_empty()
        {
            return Err(overflow());
        }
        if let Some(img) = &request.appearance_image {
            let (image_id, _) = self.stage_image_xobject(img, objects)?;
            builder = builder.image(SignatureImage::new(image_id, img.width, img.height));
        }
        let built = builder.build(w, h).map_err(|_| overflow())?;
        let ap_id = ObjId::new(self.alloc_number()?, 0);
        let mut ap_dict = built.ap_dict;
        ap_dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(built.content.len()).unwrap_or(0)),
        );
        let span = self.stage_bytes(&built.content);
        objects.push(ObjectWrite {
            id: ap_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: ap_dict,
                data_span: span,
            })),
        });
        Ok(StagedSignatureAppearance {
            ap_id,
            lines: built.lines,
            image_drawn: built.image_drawn,
        })
    }
}
