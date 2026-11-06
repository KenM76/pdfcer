//! Writing a 3D annotation's views ([`EditSession::set_3d_views`]).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::annot::Annotation;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::threed::{
    MAX_3D_VIEWS, ThreeDEmbedError, ThreeDSavedView, ThreeDViewsOutcome, carry_view,
    check_new_measurements, check_view, check_view_fields, view_dict,
};

/// Disclosed for every orthographic view written.
const ORTHO_DISCLOSURE: &str = "an orthographic view's scale is written as pdfcer reads it (the bound side spans 1/OS camera units); ISO 32000 gives the scale no unit, so another reader may frame it differently";

/// Disclosed when any view carries a 3D measurement.
pub(super) const MEASURE_DISCLOSURE: &str = "3D measurements are a PDF 2.0 feature (Adobe extension level 3 before it); the file's declared version is not changed, and a reader shows each value as written rather than measuring it";

/// Disclosed when a measurement written was linked to a comment.
pub(super) const COMMENT_DISCLOSURE: &str = "a 3D measurement linked to a comment (a Projection annotation) is written without that link; the comment stays on the page but no longer points at a measurement in these views";

/// Disclosed when the 3D stream is reached through a `/3DRef`.
pub(super) const SHARED_DISCLOSURE: &str = "the 3D stream is shared through a 3D reference dictionary, so every annotation that names it gets these views";

/// Disclosed when a measurement left out was linked to a comment.
pub(super) const REMOVED_COMMENT_DISCLOSURE: &str = "a removed 3D measurement was linked to a comment (a Projection annotation); the comment stays on the page but no longer points at a measurement";

/// Disclosed when a written `/MA` keeps entries pdfcer cannot read.
pub(super) const UNREAD_MEASURES_DISCLOSURE: &str =
    "a view's measurement list holds entries pdfcer cannot read; they were kept as they were";

/// Disclosed when a view's source dictionary is not among those being replaced.
const NOT_CARRIED_DISCLOSURE: &str = "a view that was not read from these views (another document, or an earlier state of this one) is written from pdfcer's own fields, so what pdfcer does not edit in it was not carried over";

/// How the new `/VA` is built, and what that means for the disclosures.
#[derive(Default)]
struct ViewEntries {
    entries: Vec<Object>,
    writes: Vec<ObjectWrite>,
    lost_comment: bool,
    removed_comment: bool,
    unread: bool,
    not_carried: bool,
}

impl EditSession {
    /// Replace the named views of the `/3D` annotation `annot_id` on page
    /// `page_index` with `views`, and make `default` (an index into
    /// `views`) the one a reader opens on. One undo entry.
    ///
    /// - The views go into the 3D stream's `/VA` (ISO 32000-1 §13.6.3 Table
    ///   300), one Table 304 view dictionary each, in order; `/DV` becomes
    ///   `default` as an integer, or is removed. The stream's data is not
    ///   touched, so [`crate::threed::extract_3d`] returns the same bytes.
    /// - A view from [`crate::threed::saved_3d_views`] keeps its dictionary:
    ///   an unchanged one stays exactly as it was, and a changed one has only
    ///   the keys of the changed fields replaced (a changed camera also drops
    ///   `/U3DPath`), in place when `/VA` refers to it. `/BG`, `/SA`, `/NA`,
    ///   `/IN` and anything else pdfcer does not read survive. A view built
    ///   any other way is written whole, and one read from elsewhere is too,
    ///   which is disclosed.
    /// - The annotation's `/3DV` (Table 298, which outranks `/DV`) becomes
    ///   the same integer, or is removed, so an older `/3DV` cannot override
    ///   the new default.
    /// - Empty `views` removes `/VA`, `/DV` and `/3DV`: the reader opens on
    ///   the artwork's own default view.
    /// - Each view's [`ThreeDSavedView::measurements`] become its `/MA`
    ///   (ISO 32000-2 §13.6.7.3), one direct dictionary each, without `/S`,
    ///   except that a measurement a read view already held keeps its entry,
    ///   as [`Self::set_3d_view_measurements`] describes.
    /// - [`ThreeDViewsOutcome::disclosures`] states an orthographic view's
    ///   scale reading, a stream shared through a `/3DRef`, that
    ///   measurements are PDF 2.0, a measurement whose comment link was
    ///   dropped, a removed one that had a link, unreadable entries kept, and
    ///   a view whose other keys were not carried.
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
        let built = self.view_entries(&stream.dict, views)?;
        stream.dict.remove(b"VA");
        stream.dict.remove(b"DV");
        let default_object = default
            .and_then(|d| i64::try_from(d).ok())
            .map(Object::Integer);
        if !views.is_empty() {
            let array = Object::Array(built.entries.clone());
            stream.dict.insert(Name::from(b"VA"), array);
            if let Some(d) = &default_object {
                stream.dict.insert(Name::from(b"DV"), d.clone());
            }
        }
        let mut new_annot = annot;
        new_annot.remove(b"3DV");
        if let (false, Some(d)) = (views.is_empty(), default_object) {
            new_annot.insert(Name::from(b"3DV"), d);
        }
        let disclosures = view_disclosures(views, &built, shared_stream);
        let mut objects = vec![
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
        objects.extend(built.writes);
        self.commit(Command {
            kind: CommandKind::SetThreeDViews,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
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

    /// The new `/VA` entries for `views`, each a read view's own entry or
    /// dictionary with its changes written in, else a fresh dictionary.
    fn view_entries(
        &self,
        stream: &Dict,
        views: &[ThreeDSavedView],
    ) -> Result<ViewEntries, EditError> {
        let graph = self.graph();
        let current = stream
            .get(b"VA")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
            .unwrap_or_default();
        let mut unused = vec![true; current.len()];
        let mut built = ViewEntries::default();
        for (index, v) in views.iter().enumerate() {
            let found = v.source.dict().and_then(|source| {
                let i = (0..current.len()).find(|i| {
                    unused.get(*i) == Some(&true)
                        && current
                            .get(*i)
                            .and_then(|e| graph.resolve(e).as_dict())
                            .is_some_and(|d| d == source)
                })?;
                Some((i, current.get(i)?, source))
            });
            let Some((i, entry, source)) = found else {
                if v.source.dict().is_some() {
                    check_view(index, v)?;
                    built.not_carried = true;
                }
                built.lost_comment |= v.measurements.iter().any(|m| m.has_comment);
                built.entries.push(Object::Dict(view_dict(v)));
                continue;
            };
            if let Some(slot) = unused.get_mut(i) {
                *slot = false;
            }
            let (changed, merge) = carry_view(&graph, source, v);
            if changed.is_some() {
                check_view_fields(index, v)?;
                check_new_measurements(index, &v.measurements, &merge.new)?;
            }
            built.lost_comment |= merge.lost_comment;
            built.removed_comment |= merge.removed_comment;
            built.unread |= merge.unread > 0;
            match (changed, entry) {
                (None, _) => built.entries.push(entry.clone()),
                (Some(d), Object::Reference(id))
                    if current.iter().filter(|e| *e == entry).count() == 1 =>
                {
                    built.writes.push(ObjectWrite {
                        id: *id,
                        before: self.state.get(id).cloned(),
                        after: Some(Object::Dict(d)),
                    });
                    built.entries.push(entry.clone());
                }
                (Some(d), _) => built.entries.push(Object::Dict(d)),
            }
        }
        Ok(built)
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
    pub(super) fn three_d_stream_id(&self, annot: &Dict) -> Result<(ObjId, bool), EditError> {
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

fn view_disclosures(views: &[ThreeDSavedView], built: &ViewEntries, shared: bool) -> Vec<String> {
    let measured = views.iter().any(|v| !v.measurements.is_empty());
    let says = [
        (views.iter().any(|v| v.orthographic), ORTHO_DISCLOSURE),
        (measured, MEASURE_DISCLOSURE),
        (built.lost_comment, COMMENT_DISCLOSURE),
        (built.removed_comment, REMOVED_COMMENT_DISCLOSURE),
        (built.unread, UNREAD_MEASURES_DISCLOSURE),
        (built.not_carried, NOT_CARRIED_DISCLOSURE),
        (shared, SHARED_DISCLOSURE),
    ];
    says.iter()
        .filter(|(applies, _)| *applies)
        .map(|(_, text)| (*text).to_owned())
        .collect()
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
    let built_here = views
        .iter()
        .enumerate()
        .filter(|(_, v)| v.source.dict().is_none());
    for (index, v) in built_here {
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
