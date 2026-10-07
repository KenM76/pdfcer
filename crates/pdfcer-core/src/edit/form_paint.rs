//! Colour, line width, dash and opacity for paths inside a form XObject
//! (pdfcer-gui request G142): the form-scoped twins of `set_object_paint`
//! and `set_object_stroke_style`.

use super::{
    CommandKind, EditError, EditSession, FormExtras, FormSurgeryOutcome, ObjectWrite, PaintOutcome,
    PaintRefusal, PaintRefusalReason,
};
use crate::object::{Dict, ObjId, Object};
use crate::vector::{PathPaint, Rgb, StrokeStyle, VectorEditError, VectorObject};

/// What [`EditSession::set_object_paint_in_form`] and
/// [`EditSession::set_object_stroke_style_in_form`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FormPaintOutcome {
    /// Per leaf: `changed` and `refused` hold **leaf indices** (into
    /// [`crate::vector::PageObjects::leaves`]), as passed in.
    pub paint: PaintOutcome,
    /// The form rewritten and how far it reaches; `None` when nothing
    /// changed (every leaf refused, or nothing to set).
    pub reach: Option<FormSurgeryOutcome>,
}

/// Why `obj` cannot take the asked-for paint, if it cannot: not a path, or a
/// changed channel painted with a pattern or an undecoded colour space. A
/// channel not being changed never refuses.
pub(super) fn paint_refusal(
    obj: &VectorObject,
    index: usize,
    fill: bool,
    stroke: bool,
) -> Option<PaintRefusal> {
    let VectorObject::Path(p) = obj else {
        return Some(not_a_path(index));
    };
    [(fill, &p.fill_paint), (stroke, &p.stroke_paint)]
        .into_iter()
        .filter(|(want, _)| *want)
        .find_map(|(_, paint)| match paint {
            PathPaint::Other { space, pattern, .. } => Some(PaintRefusal {
                object: index,
                reason: if *pattern {
                    PaintRefusalReason::Pattern
                } else {
                    PaintRefusalReason::UndecodedColourSpace
                },
                space: space.clone(),
            }),
            _ => None,
        })
}

const fn not_a_path(index: usize) -> PaintRefusal {
    PaintRefusal {
        object: index,
        reason: PaintRefusalReason::NotAPath,
        space: None,
    }
}

impl EditSession {
    /// **Recolour paths inside a form XObject** — the form-scoped twin of
    /// [`Self::set_object_paint`], with its refusals (spot or calibrated
    /// inks by name, patterns, non-paths), reported by leaf index.
    ///
    /// `leaf_indices` index [`crate::vector::PageObjects::leaves`] and must
    /// all lie in one invocation of one form, as for
    /// [`Self::move_objects_in_form`]. The edit is in place: it changes every
    /// place the form is drawn, and [`FormPaintOutcome::reach`] says how many
    /// (decision 076; [`Self::unshare_form`] is the operator's option).
    ///
    /// # Errors
    ///
    /// As [`Self::move_objects_in_form`]: [`EditError::FormLeafOutOfRange`],
    /// [`EditError::FormLeafSelectionSpansForms`], the page, encryption and
    /// certification guards — all before anything changes.
    pub fn set_object_paint_in_form(
        &mut self,
        page_index: usize,
        leaf_indices: &[usize],
        fill: Option<Rgb>,
        stroke: Option<Rgb>,
    ) -> Result<FormPaintOutcome, EditError> {
        let (accepted, refused) = self.sort_leaves(page_index, leaf_indices, |obj, i| {
            paint_refusal(obj, i, fill.is_some(), stroke.is_some())
        })?;
        if accepted.is_empty() || (fill.is_none() && stroke.is_none()) {
            return Ok(no_change(refused));
        }
        let reach = self.wrap_leaves(
            CommandKind::SetObjectPaint,
            page_index,
            &accepted,
            FormExtras::default(),
            |stream, objs| crate::vector::plan_recolour(stream, objs, fill, stroke),
        )?;
        Ok(changed(accepted, refused, reach))
    }

    /// **Set line width, dash and/or opacity of paths inside a form
    /// XObject** — the form-scoped twin of [`Self::set_object_stroke_style`].
    ///
    /// Addressing and reach as [`Self::set_object_paint_in_form`]. The
    /// opacity's `/ExtGState` is bound in the **form's** `/Resources`, so
    /// every invocation finds it.
    ///
    /// # Errors
    ///
    /// As [`Self::set_object_paint_in_form`], plus
    /// [`VectorEditError::InvalidStrokeStyle`] for a bad value and
    /// [`EditError::FormInheritsResources`] when opacity is asked of a form
    /// with no `/Resources` of its own.
    pub fn set_object_stroke_style_in_form(
        &mut self,
        page_index: usize,
        leaf_indices: &[usize],
        style: &StrokeStyle,
    ) -> Result<FormPaintOutcome, EditError> {
        style.validate()?;
        let (accepted, refused) = self.sort_leaves(page_index, leaf_indices, |obj, i| {
            (!matches!(obj, VectorObject::Path(_))).then(|| not_a_path(i))
        })?;
        if accepted.is_empty() || style.is_empty() {
            return Ok(no_change(refused));
        }
        let (extras, gs_name) = if style.sets_alpha() {
            let form = self.leaf_form(page_index, &accepted)?;
            let (extras, name) = self.bind_form_ext_gstate(form, style)?;
            (extras, Some(name))
        } else {
            (FormExtras::default(), None)
        };
        let prefix = style.prefix(gs_name.as_deref());
        let reach = self.wrap_leaves(
            CommandKind::SetObjectStrokeStyle,
            page_index,
            &accepted,
            extras,
            |stream, objs| crate::vector::edit::plan_wrap(stream, objs, &prefix),
        )?;
        Ok(changed(accepted, refused, reach))
    }

    /// Validate the whole selection (range, one invocation), then split it
    /// into leaves to change and leaves `refuse` turns away.
    fn sort_leaves(
        &mut self,
        page_index: usize,
        leaf_indices: &[usize],
        refuse: impl Fn(&VectorObject, usize) -> Option<PaintRefusal>,
    ) -> Result<(Vec<usize>, Vec<PaintRefusal>), EditError> {
        self.leaf_siblings(page_index, leaf_indices)?;
        let model = self.page_objects(page_index)?;
        let mut accepted = Vec::new();
        let mut refused = Vec::new();
        for &i in leaf_indices {
            let count = model.leaves.len();
            let leaf = model
                .leaves
                .get(i)
                .ok_or(EditError::FormLeafOutOfRange { index: i, count })?;
            match refuse(&leaf.object, i) {
                Some(r) => refused.push(r),
                None => accepted.push(i),
            }
        }
        Ok((accepted, refused))
    }

    /// The form XObject the (already validated) leaves live in.
    fn leaf_form(&mut self, page_index: usize, leaves: &[usize]) -> Result<ObjId, EditError> {
        let model = self.page_objects(page_index)?;
        let count = model.leaves.len();
        let first = leaves.first().copied().unwrap_or_default();
        model
            .leaves
            .get(first)
            .and_then(|l| l.containment.last().copied())
            .ok_or(EditError::FormLeafOutOfRange {
                index: first,
                count,
            })
    }

    /// A new `/ExtGState` for `style`'s alphas, bound under a free name in
    /// `form`'s own `/Resources`.
    fn bind_form_ext_gstate(
        &mut self,
        form: ObjId,
        style: &StrokeStyle,
    ) -> Result<(FormExtras, Vec<u8>), EditError> {
        let Some(Object::Stream(stream)) = self.value(form) else {
            return Err(EditError::NotADictionary {
                id: form,
                key: "Subtype",
            });
        };
        let form_dict = stream.dict.clone();
        let resources = match form_dict.get(b"Resources") {
            Some(r) => self.resolve_value(r).as_dict().cloned().unwrap_or_default(),
            None => return Err(EditError::FormInheritsResources { form }),
        };
        let name = self.free_name_in(&resources, b"ExtGState", "pdfcerGS");
        let gs_id = ObjId::new(self.alloc_number()?, 0);
        let (writes, shared) = crate::text_edit::addtext::bind_resource(
            &self.graph(),
            form,
            false,
            b"ExtGState",
            &name,
            Object::Reference(gs_id),
        );
        let mut extras = FormExtras {
            writes: vec![ObjectWrite {
                id: gs_id,
                before: None,
                after: Some(Object::Dict(style.ext_gstate())),
            }],
            ..FormExtras::default()
        };
        for (id, value) in writes {
            // A direct `/Resources` is rebuilt inside the form's own
            // dictionary, which travels with the new content instead.
            match value {
                Object::Dict(d) if id == form => {
                    extras.form_dict = Some(merge_form_dict(&form_dict, d))
                }
                value => extras.writes.push(ObjectWrite {
                    id,
                    before: self.state.get(&id).cloned(),
                    after: Some(value),
                }),
            }
        }
        if shared {
            extras.disclosures.push(SHARED_FORM_GS_NOTE.to_owned());
        }
        Ok((extras, name))
    }

    /// Wrap `leaves` (page leaf indices, one invocation) in their form's
    /// stream with `plan`, as one command.
    fn wrap_leaves(
        &mut self,
        kind: CommandKind,
        page_index: usize,
        leaves: &[usize],
        extras: FormExtras,
        plan: impl FnOnce(
            &crate::content::ContentStream,
            &[&VectorObject],
        ) -> Result<crate::vector::PlannedEdit, VectorEditError>,
    ) -> Result<FormSurgeryOutcome, EditError> {
        let siblings = self.leaf_siblings(page_index, leaves)?;
        let first = leaves.first().copied().unwrap_or_default();
        self.form_surgery_extras(kind, page_index, first, extras, |stream, model, _| {
            let count = model.objects.len();
            let objs = siblings
                .iter()
                .map(|&i| {
                    model
                        .objects
                        .get(i)
                        .ok_or(VectorEditError::ObjectOutOfRange { index: i, count })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(plan(stream, &objs)?)
        })
    }
}

/// `bind_resource` returns the form's dictionary as read from the graph; keep
/// the rest of the session's current dictionary and take only `/Resources`.
fn merge_form_dict(current: &Dict, bound: Dict) -> Dict {
    let mut out = current.clone();
    if let Some(r) = bound.get(b"Resources") {
        out.insert(crate::object::Name::from(b"Resources"), r.clone());
    }
    out
}

fn no_change(refused: Vec<PaintRefusal>) -> FormPaintOutcome {
    FormPaintOutcome {
        paint: PaintOutcome {
            changed: Vec::new(),
            refused,
        },
        reach: None,
    }
}

fn changed(
    changed: Vec<usize>,
    refused: Vec<PaintRefusal>,
    reach: FormSurgeryOutcome,
) -> FormPaintOutcome {
    FormPaintOutcome {
        paint: PaintOutcome { changed, refused },
        reach: Some(reach),
    }
}

const SHARED_FORM_GS_NOTE: &str = "stroke style: the /ExtGState entry was added to a /Resources \
     dictionary the form shares with other forms or pages; it is unreferenced there and changes \
     nothing about how they render";
