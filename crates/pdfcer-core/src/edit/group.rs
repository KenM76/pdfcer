//! Grouping page objects into a form XObject and ungrouping a form
//! (pdfcer-gui request G280).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Range;

use super::restack::ext_gstate;
use super::{CommandKind, EditError, EditSession, ObjectWrite, PendingGraph, PermissionBit};
use crate::content::ContentStream;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Page;
use crate::pageops::region::ResourceUse;
use crate::vector::{
    Bounds, GroupError, GsResources, ImageSource, MARKER_KEY, Matrix, PageObjects, ResourceKey,
    VectorObject, form_box, inlined, is_marker, marker, marker_date, paint_outset, plan_group,
    used_resources,
};

/// The `/XObject` resource-name prefix a group is bound under.
const GROUP_PREFIX: &str = "pdfcerG";
/// The resource-name prefix an ungrouped form's clashing resource takes.
const RENAME_PREFIX: &str = "pdfcerU";

/// What [`EditSession::group_objects`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GroupOutcome {
    /// The group's index in the page's next `page_objects`.
    pub index: usize,
    /// The new form XObject.
    pub form: ObjId,
    /// How many objects went into it.
    pub members: usize,
}

/// What [`EditSession::ungroup_form`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UngroupOutcome {
    /// The indices, in the page's next `page_objects`, of the objects that
    /// replaced the form.
    pub indices: Range<usize>,
    /// The form that was inlined. It is not deleted: another `Do`, on this
    /// page or another, may still paint it.
    pub form: ObjId,
    /// Whether the form's `/BBox` was kept as a clip around the objects.
    /// `false` only for a group pdfcer made whose contents lie inside it.
    pub clipped: bool,
    /// How many of the form's resources were bound on the page under a new
    /// name because the page already used theirs for something else.
    pub renamed: usize,
}

/// A form's resources merged into a page's as its content is inlined.
struct Merge<'a, G: ObjectGraph + ?Sized> {
    graph: &'a G,
    own: Option<Dict>,
    page: &'a Dict,
    /// `(category, name)` to the value to bind on the page.
    bind: BTreeMap<(&'static [u8], Vec<u8>), Object>,
    /// `(category, the form's name)` to the name the page binds it under.
    renamed: BTreeMap<(&'static [u8], Vec<u8>), Vec<u8>>,
}

impl<G: ObjectGraph + ?Sized> Merge<'_, G> {
    fn entry(&self, resources: &Dict, category: &[u8], name: &[u8]) -> Option<Object> {
        self.graph
            .resolve(resources.get(category)?)
            .as_dict()?
            .get(name)
            .cloned()
    }

    /// The new name for a use, if the page's own would mean something else.
    fn rename(&mut self, u: &ResourceUse<'_>) -> Option<Vec<u8>> {
        let value = self.entry(self.own.as_ref()?, u.category, u.name)?;
        let key = (u.category, u.name.to_vec());
        if let Some(new) = self.renamed.get(&key) {
            return Some(new.clone());
        }
        let bound = self
            .bind
            .get(&key)
            .cloned()
            .or_else(|| self.entry(self.page, u.category, u.name));
        match bound {
            Some(existing) if existing == value => None,
            None => {
                self.bind.insert(key, value);
                None
            }
            Some(_) => {
                let new = (1u32..)
                    .map(|n| format!("{RENAME_PREFIX}{n}").into_bytes())
                    .find(|c| {
                        self.entry(self.page, u.category, c).is_none()
                            && !self.bind.contains_key(&(u.category, c.clone()))
                    })?;
                self.bind.insert((u.category, new.clone()), value);
                self.renamed.insert(key, new.clone());
                Some(new)
            }
        }
    }
}

impl EditSession {
    /// Group page objects into **one form XObject** painted by one `Do` —
    /// one command, one undo entry ([`CommandKind::GroupObjects`]). Indices
    /// are into [`Self::page_objects`]`(page_index).objects`; order and
    /// repeats do not matter.
    ///
    /// The group takes the front-most member's place in paint order. Each
    /// member's bytes move into the form wrapped in `q … Q` rebuilding the
    /// graphics state it was painted under, exactly as
    /// [`Self::restack_objects`] moves an object, so the page renders the
    /// same — except that a member which was *behind* an unselected object
    /// the front-most member is in front of now paints over it, as grouping
    /// does in every editor.
    ///
    /// The form has no `/Group` entry (it is not a transparency group), a
    /// `/BBox` enclosing its members' paint, `/Resources` naming only what
    /// its content uses, and a `/PieceInfo` `/pdfcer` marker
    /// ([`crate::vector::is_group_form`]). It is bound in the page's
    /// `/XObject` resources under a free `pdfcerG<n>` name.
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`GroupError::NoObjects`] for an empty list.
    /// - [`GroupError::MemberCannotJoin`] naming the first object that
    ///   cannot be gathered with the others: one under a different clip or
    ///   marked-content section (an `/OC` layer, a tagged element) than the
    ///   front-most member, or whose own bytes set one.
    /// - [`GroupError::NotInvertible`] under a degenerate transform.
    /// - [`crate::vector::VectorEditError::ObjectOutOfRange`] and
    ///   `OverlappingObjectSpans` (as [`GroupError::Edit`]).
    /// - [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   and the encryption and certification guards.
    pub fn group_objects(
        &mut self,
        page_index: usize,
        object_indices: &[usize],
    ) -> Result<GroupOutcome, EditError> {
        let (page, content_id) = self.group_target(page_index)?;
        let (stream, model) = self.page_content_and_objects(&page)?;
        let free_names = self.free_names(&page.resources, object_indices.len());
        let name = self.free_name_in(&page.resources, b"XObject", GROUP_PREFIX);
        let plan = {
            let graph = self.graph();
            let lookup = |n: &[u8]| ext_gstate(&graph, &page.resources, n);
            let res = GsResources {
                ext_gstate: &lookup,
                free_names: &free_names,
            };
            plan_group(
                &stream,
                &model.objects,
                model.initial,
                object_indices,
                &res,
                &name,
            )?
        };
        let resources = self.group_resources(&page.resources, &plan.form_content, plan.bind)?;
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
        dict.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Form")));
        dict.insert(
            Name::from(b"BBox"),
            Object::Array(plan.bbox.iter().map(|v| Object::Real(*v)).collect()),
        );
        dict.insert(Name::from(b"Resources"), Object::Dict(resources));
        let mut piece = Dict::new();
        piece.insert(Name(MARKER_KEY.to_vec()), Object::Dict(marker()));
        dict.insert(Name::from(b"PieceInfo"), Object::Dict(piece));
        dict.insert(
            Name::from(b"LastModified"),
            Object::String(marker_date().to_vec()),
        );
        let mut prior = Vec::new();
        let form = self.new_stream(dict, &plan.form_content, &mut prior)?;
        let binds = vec![((&b"XObject"[..], name), Object::Reference(form))];
        prior.extend(self.bind_on_page(&page, binds));
        let mut disclosures = Vec::new();
        let (command, _) = self.text_edit_command(
            CommandKind::GroupObjects,
            content_id,
            &page,
            plan.content,
            prior,
            &mut disclosures,
        )?;
        self.commit(command);
        Ok(GroupOutcome {
            index: plan.index,
            form,
            members: plan.members,
        })
    }

    /// **Ungroup**: replace the `Do` painting a form XObject with the form's
    /// content — one command, one undo entry
    /// ([`CommandKind::UngroupForm`]). `object_index` is into
    /// [`Self::page_objects`]`(page_index).objects`.
    ///
    /// The content is written as the `Do` executed it (ISO 32000-1
    /// §8.10.1): inside `q … Q`, under the form's `/Matrix`, clipped to its
    /// `/BBox`, so the page renders the same. The clip is left out
    /// ([`UngroupOutcome::clipped`]) only for a group pdfcer made whose
    /// contents lie inside the box, which leaves the objects free to
    /// restack and regroup.
    ///
    /// Resources the content names are bound on the page: under the same
    /// name when the page has none or the same one, under a free
    /// `pdfcerU<n>` name (and renamed in the content) when the page's means
    /// something else.
    ///
    /// Works on any form, grouped by pdfcer or imported. A form painted
    /// more than once is inlined at this `Do` only and stays as it is
    /// everywhere else. The form object and its page resource entry are
    /// left in place.
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`GroupError::NotAForm`] when the object is not a `Do` of a form
    ///   XObject held as an indirect object.
    /// - [`GroupError::TransparencyGroup`] for a form with `/Group`.
    /// - [`GroupError::OptionalContent`] for a form with `/OC`.
    /// - [`GroupError::Unportable`] for a reference XObject, a form with no
    ///   `/BBox`, content that does not parse or is unbalanced, or an inline
    ///   image naming a colour space the page binds differently.
    /// - [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   and the encryption and certification guards.
    pub fn ungroup_form(
        &mut self,
        page_index: usize,
        object_index: usize,
    ) -> Result<UngroupOutcome, EditError> {
        let (page, content_id) = self.group_target(page_index)?;
        let (stream, model) = self.page_content_and_objects(&page)?;
        let (form, do_ctm, span) = form_at(&model, object_index)?;
        let shape = self.form_shape(form)?;
        let content =
            ContentStream::from_form(&self.view(), form).map_err(|_| GroupError::Unportable {
                reason: "its content does not parse",
            })?;
        let placed = Matrix::new(
            shape.matrix[0],
            shape.matrix[1],
            shape.matrix[2],
            shape.matrix[3],
            shape.matrix[4],
            shape.matrix[5],
        )
        .post_concat(do_ctm)
        .post_concat(model.initial);
        let clipped = !(shape.group && fits(&model, object_index, placed, shape.bbox));
        let (body, bind, renamed) = {
            let graph = self.graph();
            let mut merge = Merge {
                graph: &graph,
                own: shape.resources,
                page: &page.resources,
                bind: BTreeMap::new(),
                renamed: BTreeMap::new(),
            };
            let body = inlined(
                &content,
                shape.matrix,
                clipped.then_some(shape.bbox),
                &mut |u: ResourceUse<'_>| merge.rename(&u),
            )?;
            (body, merge.bind, merge.renamed.len())
        };
        let buf = &stream.buf;
        let mut out = buf.get(..span.start).unwrap_or_default().to_vec();
        out.extend_from_slice(&body);
        out.extend_from_slice(buf.get(span.start + span.len..).unwrap_or_default());
        let prior = self.bind_on_page(&page, bind.into_iter().collect());
        let before = model.objects.len();
        let mut disclosures = Vec::new();
        let (command, _) = self.text_edit_command(
            CommandKind::UngroupForm,
            content_id,
            &page,
            out,
            prior,
            &mut disclosures,
        )?;
        self.commit(command);
        let after = self.page_objects(page_index)?.objects.len();
        Ok(UngroupOutcome {
            indices: object_index..(object_index + after + 1).saturating_sub(before),
            form,
            clipped,
            renamed,
        })
    }

    /// The page and its first content stream, behind the content-edit
    /// guards.
    fn group_target(&mut self, page_index: usize) -> Result<(Page, ObjId), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let pages = self.pages()?;
        let count = pages.len();
        let page = pages.get(page_index).ok_or(EditError::PageOutOfRange {
            index: page_index,
            count,
        })?;
        let content_id = *page
            .contents
            .first()
            .ok_or(EditError::VectorEditNoContents { page_index })?;
        Ok((page.clone(), content_id))
    }

    /// The resources a new group's content names, taken from the page's,
    /// plus the reset `/ExtGState`s made for it.
    fn group_resources(
        &self,
        page: &Dict,
        form_content: &[u8],
        resets: Vec<(Vec<u8>, Dict)>,
    ) -> Result<Dict, GroupError> {
        let graph = self.graph();
        let mut out: BTreeMap<&'static [u8], Dict> = BTreeMap::new();
        for (category, name) in used_resources(form_content)? {
            let value = resets
                .iter()
                .find(|(n, _)| category == b"ExtGState" && *n == name)
                .map(|(_, d)| Object::Dict(d.clone()))
                .or_else(|| {
                    graph
                        .resolve(page.get(category)?)
                        .as_dict()?
                        .get(&name)
                        .cloned()
                });
            if let Some(value) = value {
                out.entry(category).or_default().insert(Name(name), value);
            }
        }
        let mut resources = Dict::new();
        for (category, d) in out {
            resources.insert(Name(category.to_vec()), Object::Dict(d));
        }
        Ok(resources)
    }

    /// Writes binding each `(category, name)` to its value in the page's
    /// resources.
    fn bind_on_page(&self, page: &Page, binds: Vec<(ResourceKey, Object)>) -> Vec<ObjectWrite> {
        let mut scratch: BTreeMap<ObjId, Object> = BTreeMap::new();
        let removed = HashSet::new();
        for ((category, name), value) in binds {
            let pending = PendingGraph {
                session: self,
                scratch: &scratch,
                removed: &removed,
            };
            let (writes, _) = crate::text_edit::addtext::bind_resource(
                &pending, page.id, true, category, &name, value,
            );
            scratch.extend(writes);
        }
        scratch
            .into_iter()
            .map(|(id, after)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            })
            .collect()
    }

    /// What ungrouping needs of a form's dictionary, or why it cannot be
    /// inlined.
    fn form_shape(&self, form: ObjId) -> Result<FormShape, GroupError> {
        let graph = self.graph();
        let unportable = |reason| GroupError::Unportable { reason };
        let Some(Object::Stream(stream)) = graph.value(form) else {
            return Err(unportable("it is not a stream"));
        };
        let dict = &stream.dict;
        if dict.get(b"Group").is_some() {
            return Err(GroupError::TransparencyGroup);
        }
        if dict.get(b"OC").is_some() {
            return Err(GroupError::OptionalContent);
        }
        if dict.get(b"Ref").is_some() {
            return Err(unportable("it is a reference XObject"));
        }
        let numbers = |key: &[u8]| -> Option<Vec<f64>> {
            graph
                .resolve(dict.get(key)?)
                .as_array()?
                .iter()
                .map(|o| graph.resolve(o).as_number())
                .collect()
        };
        let bbox: [f64; 4] = numbers(b"BBox")
            .and_then(|v| v.try_into().ok())
            .ok_or(unportable("it has no /BBox"))?;
        let matrix: [f64; 6] = numbers(b"Matrix")
            .and_then(|v| v.try_into().ok())
            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        Ok(FormShape {
            bbox: [
                bbox[0].min(bbox[2]),
                bbox[1].min(bbox[3]),
                bbox[0].max(bbox[2]),
                bbox[1].max(bbox[3]),
            ],
            matrix,
            resources: dict
                .get(b"Resources")
                .and_then(|o| graph.resolve(o).as_dict().cloned()),
            group: is_group_dict(&graph, dict),
        })
    }
}

struct FormShape {
    bbox: [f64; 4],
    matrix: [f64; 6],
    resources: Option<Dict>,
    group: bool,
}

fn is_group_dict<G: ObjectGraph + ?Sized>(graph: &G, form: &Dict) -> bool {
    form.get(b"PieceInfo")
        .and_then(|o| graph.resolve(o).as_dict())
        .and_then(|p| p.get(MARKER_KEY))
        .and_then(|o| graph.resolve(o).as_dict())
        .is_some_and(is_marker)
}

/// Whether `form` is a group [`EditSession::group_objects`] made, rather
/// than a form XObject that came with the document.
///
/// Read from the form's `/PieceInfo` `/pdfcer` entry (ISO 32000-1 §14.5),
/// so it survives save and reopen; a group inside a group answers for
/// itself. `form` is [`crate::vector::ImageObject::xobject`] of an object
/// whose source is [`ImageSource::Form`].
#[must_use]
pub fn is_group_form<G: ObjectGraph + ?Sized>(graph: &G, form: ObjId) -> bool {
    matches!(graph.value(form), Some(Object::Stream(s)) if is_group_dict(graph, &s.dict))
}

/// The form drawn at `object_index`: its object, the matrix in force at
/// its `Do`, and the `Do`'s bytes.
fn form_at(
    model: &PageObjects,
    object_index: usize,
) -> Result<(ObjId, Matrix, crate::span::ByteSpan), GroupError> {
    let not_a_form = GroupError::NotAForm {
        index: object_index,
    };
    match model.objects.get(object_index) {
        Some(VectorObject::Image(i)) if i.source == ImageSource::Form => {
            Ok((i.xobject.ok_or(not_a_form)?, i.ctm, i.bytes))
        }
        Some(_) => Err(not_a_form),
        None => Err(GroupError::Edit(
            crate::vector::VectorEditError::ObjectOutOfRange {
                index: object_index,
                count: model.objects.len(),
            },
        )),
    }
}

/// Whether everything the form at `object_index` paints lies inside its
/// `bbox`, `placed` being its form-to-page matrix.
fn fits(model: &PageObjects, object_index: usize, placed: Matrix, bbox: [f64; 4]) -> bool {
    let Some(to_form) = placed.inverse() else {
        return false;
    };
    let limit = Bounds {
        min: crate::vector::Point::new(bbox[0], bbox[1]),
        max: crate::vector::Point::new(bbox[2], bbox[3]),
    }
    .inflate(1e-3);
    let seen: BTreeSet<bool> = model
        .leaves
        .iter()
        .filter(|l| l.paint_order == object_index)
        .map(|l| {
            let reach = l.object.page_bbox().inflate(paint_outset(&l.object));
            form_box(reach, to_form).is_some_and(|b| {
                b[0] >= limit.min.x
                    && b[1] >= limit.min.y
                    && b[2] <= limit.max.x
                    && b[3] <= limit.max.y
            })
        })
        .collect();
    seen.len() == 1 && seen.contains(&true)
}
