//! Writing a 3D annotation's views ([`EditSession::set_3d_views`]).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::annot::Annotation;
use crate::object::{Dict, Name, ObjId, Object};
use crate::threed::{
    MAX_3D_VIEWS, ThreeDEmbedError, ThreeDSavedView, ThreeDViewsOutcome, check_view, view_dict,
};

/// Disclosed for every orthographic view written.
const ORTHO_DISCLOSURE: &str = "an orthographic view's scale is written as pdfcer reads it (the bound side spans 1/OS camera units); ISO 32000 gives the scale no unit, so another reader may frame it differently";

/// Disclosed when the 3D stream is reached through a `/3DRef`.
const SHARED_DISCLOSURE: &str = "the 3D stream is shared through a 3D reference dictionary, so every annotation that names it gets these views";

impl EditSession {
    /// Replace the named views of the `/3D` annotation `annot_id` on page
    /// `page_index` with `views`, and make `default` (an index into
    /// `views`) the one a reader opens on. One undo entry.
    ///
    /// - The views go into the 3D stream's `/VA` (ISO 32000-1 §13.6.3 Table
    ///   300), one Table 304 view dictionary each, in order; `/DV` becomes
    ///   `default` as an integer, or is removed. The stream's data is not
    ///   touched, so [`crate::threed::extract_3d`] returns the same bytes.
    /// - The annotation's `/3DV` (Table 298, which outranks `/DV`) becomes
    ///   the same integer, or is removed, so an older `/3DV` cannot override
    ///   the new default.
    /// - Empty `views` removes `/VA`, `/DV` and `/3DV`: the reader opens on
    ///   the artwork's own default view.
    /// - [`ThreeDViewsOutcome::disclosures`] states an orthographic view's
    ///   scale reading and a stream shared through a `/3DRef`.
    ///
    /// # Errors
    ///
    /// As [`Self::set_3d_poster`] for the annotation, except a missing
    /// `/Rect` is allowed; then [`EditError::ThreeD`] with
    /// [`ThreeDEmbedError::TooManyViews`] over [`MAX_3D_VIEWS`],
    /// [`ThreeDEmbedError::ViewNameEmpty`] or
    /// [`ThreeDEmbedError::ViewInvalid`] for the first bad view,
    /// [`ThreeDEmbedError::DefaultViewOutOfRange`], and
    /// [`ThreeDEmbedError::NoThreeDStream`] when `/3DD` names no 3D stream.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{edit::EditSession, object::ObjId};
    /// # use pdfcer_core::threed::ThreeDSavedView;
    /// # fn demo(session: &mut EditSession, annot: ObjId) -> Result<(), Box<dyn std::error::Error>> {
    /// let back = ThreeDSavedView::new("Back", [1., 0., 0., 0., 0., 1., 0., -1., 0., 0., 5., 0.])
    ///     .with_orbit_distance(5.0)
    ///     .with_perspective(30.0);
    /// let outcome = session.set_3d_views(0, annot, &[back], Some(0))?;
    /// assert_eq!(outcome.views_after, 1);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_3d_views(
        &mut self,
        page_index: usize,
        annot_id: ObjId,
        views: &[ThreeDSavedView],
        default: Option<usize>,
    ) -> Result<ThreeDViewsOutcome, EditError> {
        check_views(views, default)?;
        let (_target, annot) = self.three_d_annot(page_index, annot_id)?;
        let (stream_id, shared_stream) = self.three_d_stream_id(&annot)?;
        let Some(Object::Stream(mut stream)) = self.value(stream_id).cloned() else {
            return Err(ThreeDEmbedError::NoThreeDStream.into());
        };
        let views_before = match stream.dict.get(b"VA").map(|o| self.resolve_value(o)) {
            Some(Object::Array(a)) => a.len(),
            _ => 0,
        };
        stream.dict.remove(b"VA");
        stream.dict.remove(b"DV");
        let default_object = default
            .and_then(|d| i64::try_from(d).ok())
            .map(Object::Integer);
        if !views.is_empty() {
            let array = views.iter().map(|v| Object::Dict(view_dict(v))).collect();
            stream.dict.insert(Name::from(b"VA"), Object::Array(array));
            if let Some(d) = &default_object {
                stream.dict.insert(Name::from(b"DV"), d.clone());
            }
        }
        let mut new_annot = annot;
        new_annot.remove(b"3DV");
        if let (false, Some(d)) = (views.is_empty(), default_object) {
            new_annot.insert(Name::from(b"3DV"), d);
        }
        let objects = vec![
            ObjectWrite {
                id: stream_id,
                before: self.state.get(&stream_id).cloned(),
                after: Some(Object::Stream(stream)),
            },
            ObjectWrite {
                id: annot_id,
                before: self.state.get(&annot_id).cloned(),
                after: Some(Object::Dict(new_annot)),
            },
        ];
        self.commit(Command {
            kind: CommandKind::SetThreeDViews,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        let mut disclosures = Vec::new();
        if views.iter().any(|v| v.orthographic) {
            disclosures.push(ORTHO_DISCLOSURE.to_owned());
        }
        if shared_stream {
            disclosures.push(SHARED_DISCLOSURE.to_owned());
        }
        Ok(ThreeDViewsOutcome {
            annot_id,
            stream_id,
            views_before,
            views_after: views.len(),
            default: default.filter(|_| !views.is_empty()),
            shared_stream,
            disclosures,
        })
    }

    /// The `/3D` annotation `annot_id` on page `page_index` and its
    /// dictionary, refused as [`Self::set_3d_poster`] documents (encrypted,
    /// certification, page, not listed, another subtype, locked).
    pub(super) fn three_d_annot(
        &mut self,
        page_index: usize,
        annot_id: ObjId,
    ) -> Result<(Annotation, Dict), EditError> {
        let (_slots, page_id) = self.annotation_author_target(page_index)?;
        let target = crate::annot::page_annotations(&self.graph(), page_id)
            .into_iter()
            .find(|a| a.id == Some(annot_id))
            .ok_or(EditError::AnnotationNotFound { id: annot_id })?;
        let subtype = String::from_utf8_lossy(&target.subtype).into_owned();
        if target.subtype != b"3D" {
            return Err(ThreeDEmbedError::NotA3dAnnotation { subtype }.into());
        }
        if target.flags.locked() {
            return Err(EditError::AnnotationLocked {
                id: annot_id,
                subtype,
            });
        }
        let Some(Object::Dict(dict)) = self.value(annot_id).cloned() else {
            return Err(EditError::AnnotationNotFound { id: annot_id });
        };
        Ok((target, dict))
    }

    /// The 3D stream `annot`'s `/3DD` names, and whether it is reached
    /// through a 3D reference dictionary (§13.6.3.3), which other
    /// annotations may share.
    fn three_d_stream_id(&self, annot: &Dict) -> Result<(ObjId, bool), EditError> {
        let is_3d_stream = |id: ObjId| {
            matches!(self.value(id), Some(Object::Stream(s))
                if s.dict.get(b"Type").and_then(Object::as_name).map(Name::as_bytes) == Some(b"3D"))
        };
        let reference_dict = match annot.get(b"3DD") {
            Some(Object::Reference(id)) if is_3d_stream(*id) => return Ok((*id, false)),
            Some(Object::Reference(id)) => self.value(*id),
            other => other,
        };
        if let Some(Object::Dict(r)) = reference_dict
            && let Some(Object::Reference(id)) = r.get(b"3D")
            && is_3d_stream(*id)
        {
            return Ok((*id, true));
        }
        Err(ThreeDEmbedError::NoThreeDStream.into())
    }
}

/// Refuse more than [`MAX_3D_VIEWS`], the first invalid view, or a default
/// past the end.
fn check_views(views: &[ThreeDSavedView], default: Option<usize>) -> Result<(), EditError> {
    if views.len() > MAX_3D_VIEWS {
        return Err(ThreeDEmbedError::TooManyViews {
            count: views.len(),
            max: MAX_3D_VIEWS,
        }
        .into());
    }
    for (index, v) in views.iter().enumerate() {
        check_view(index, v)?;
    }
    match default {
        Some(index) if index >= views.len() && !views.is_empty() => {
            Err(ThreeDEmbedError::DefaultViewOutOfRange {
                index,
                count: views.len(),
            }
            .into())
        }
        _ => Ok(()),
    }
}
