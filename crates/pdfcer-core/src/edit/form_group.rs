//! Grouping and ungrouping **inside** a form XObject: groups within groups.

use std::collections::{BTreeMap, HashSet};

use super::group::{GROUP_PREFIX, Merge, group_form_dict, leaves_fit};
use super::restack::ext_gstate;
use super::{CommandKind, EditError, EditSession, ObjectWrite, PendingGraph, PermissionBit};
use crate::content::ContentStream;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::pageops::region::ResourceUse;
use crate::span::ByteSpan;
use crate::vector::{
    Bounds, FormLeaf, GroupError, GsResources, ImageSource, Matrix, Point, ResourceKey,
    VectorObject, form_box, inlined, plan_group,
};

/// Said when a binding went into a resource dictionary held as its own
/// object, which another form or page may also use.
const SHARED_RESOURCES_NOTE: &str = "the form's resources are held in an object of their own, \
     which other content may share; the new entry is visible there too and is unused by it";

/// What [`EditSession::group_objects_in_form`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FormGroupOutcome {
    /// The new group's form XObject.
    pub group: ObjId,
    /// The form the group was made inside.
    pub container: ObjId,
    /// How many objects went into the group.
    pub members: usize,
    /// How many times the container is painted in the document: the number
    /// of places this one edit changed.
    pub invocations: usize,
    /// Across how many pages those fall.
    pub pages: usize,
    /// What pdfcer did that the page does not show.
    pub disclosures: Vec<String>,
}

/// What [`EditSession::ungroup_form_in_form`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FormUngroupOutcome {
    /// The form that was inlined. It is not deleted.
    pub form: ObjId,
    /// The form its content went into; `None` when it went into the page.
    pub container: Option<ObjId>,
    /// Whether the form's `/BBox` was kept as a clip around the objects
    /// ([`super::UngroupOutcome::clipped`]).
    pub clipped: bool,
    /// How many of the form's resources were bound under a new name because
    /// the container already used theirs for something else.
    pub renamed: usize,
    /// How many times the container is painted in the document (`1` for
    /// the page).
    pub invocations: usize,
    /// Across how many pages those fall.
    pub pages: usize,
    /// What pdfcer did that the page does not show.
    pub disclosures: Vec<String>,
}

/// Where a nested form is painted: its parent, the parent's placement on
/// the page, and the `Do`'s bytes in the parent's stream.
struct NestedDo {
    parent: ObjId,
    span: ByteSpan,
}

impl EditSession {
    /// Group objects that are **inside a form XObject** into a new form
    /// painted from that form — a group within a group. One command, one
    /// undo entry ([`CommandKind::GroupObjects`]).
    ///
    /// `leaf_indices` index [`crate::vector::PageObjects::leaves`] and must
    /// all lie in one invocation of one form, as for
    /// [`Self::move_objects_in_form`]. The new group is built exactly as
    /// [`Self::group_objects`] builds one and is bound in the containing
    /// form's own `/Resources`. The members stay leaves; their
    /// [`FormLeaf::containment`] gains the new group as its last entry.
    ///
    /// The edit is in place: it changes every place the containing form is
    /// drawn, and the outcome says how many (decision 076;
    /// [`Self::unshare_form`] is the operator's option). Every one of them
    /// renders as before.
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`GroupError::NoObjects`] for an empty list.
    /// - [`EditError::FormLeafOutOfRange`] and
    ///   [`EditError::FormLeafSelectionSpansForms`].
    /// - [`EditError::FormInheritsResources`] when the containing form has
    ///   no `/Resources` of its own to bind the group in.
    /// - [`GroupError::TooDeep`] when the members are already
    ///   [`crate::content::MAX_FORM_DEPTH`] forms deep.
    /// - [`GroupError::MemberCannotJoin`] and [`GroupError::NotInvertible`],
    ///   as [`Self::group_objects`]; the index is the member's index in the
    ///   form's own content.
    /// - [`EditError::PageOutOfRange`] and the encryption and certification
    ///   guards.
    pub fn group_objects_in_form(
        &mut self,
        page_index: usize,
        leaf_indices: &[usize],
    ) -> Result<FormGroupOutcome, EditError> {
        self.form_group_guards()?;
        let Some(&first) = leaf_indices.first() else {
            return Err(GroupError::NoObjects.into());
        };
        let members = self.leaf_siblings(page_index, leaf_indices)?;
        let (leaf, container) = self.form_leaf_at(page_index, first)?;
        let limit = crate::content::MAX_FORM_DEPTH;
        if leaf.containment.len() >= limit {
            return Err(GroupError::TooDeep { limit }.into());
        }
        let (stream, form_dict, resources) = self.form_with_own_resources(container)?;
        let model = self.form_model(page_index, &stream, &form_dict, &leaf);
        let free_names = self.free_names(&resources, members.len());
        let name = self.free_name_in(&resources, b"XObject", GROUP_PREFIX);
        let plan = {
            let graph = self.graph();
            let lookup = |n: &[u8]| ext_gstate(&graph, &resources, n);
            let res = GsResources {
                ext_gstate: &lookup,
                free_names: &free_names,
            };
            plan_group(
                &stream,
                &model.objects,
                model.initial,
                &members,
                &res,
                &name,
            )?
        };
        let inner = self.group_resources(&resources, &plan.form_content, plan.bind)?;
        let mut writes = Vec::new();
        // The stroke state the members inherit at the container's `Do` is
        // not known in here, so a box measured from them could clip a wide
        // stroke. The container's own box, carried into the group's space,
        // cannot clip more than is clipped already.
        let bbox = self
            .form_bbox(&form_dict)
            .and_then(|b| form_box(b, leaf.placement.post_concat(plan.to_form)))
            .unwrap_or(plan.bbox);
        let group = self.new_stream(
            group_form_dict(bbox, inner),
            &plan.form_content,
            &mut writes,
        )?;
        let binds = vec![((&b"XObject"[..], name), Object::Reference(group))];
        let (bound, form_dict, shared) = self.bind_in_form(container, &form_dict, binds);
        writes.extend(bound);
        let reach = self.form_invocation_reach(container);
        let mut command = self.form_edit_command_kind(
            CommandKind::GroupObjects,
            container,
            &form_dict,
            plan.content,
        );
        command.objects.extend(writes);
        self.commit(command);
        Ok(FormGroupOutcome {
            group,
            container,
            members: plan.members,
            invocations: reach.invocations,
            pages: reach.pages,
            disclosures: shared_note(shared),
        })
    }

    /// **Ungroup inside a form**: dissolve the form the leaf at `leaf_index`
    /// is directly inside, writing its content into the form that paints
    /// it. One command, one undo entry ([`CommandKind::UngroupForm`]).
    ///
    /// The content is written as the `Do` executed it, as
    /// [`Self::ungroup_form`] writes it, and the resources it names are
    /// bound in the containing form's `/Resources`. When the leaf's form is
    /// painted by the page itself this *is* [`Self::ungroup_form`] at that
    /// object, and [`FormUngroupOutcome::container`] is `None`.
    ///
    /// The edit is in place: it changes every place the containing form is
    /// drawn, and the outcome says how many (decision 076). A form painted
    /// more than once inside its container is inlined at this `Do` only.
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`EditError::FormLeafOutOfRange`].
    /// - [`GroupError::TransparencyGroup`], [`GroupError::OptionalContent`]
    ///   and [`GroupError::Unportable`], as [`Self::ungroup_form`]; also
    ///   `Unportable` when the form's `Do` cannot be found in its container.
    /// - [`EditError::FormInheritsResources`] when the containing form has
    ///   no `/Resources` of its own.
    /// - [`EditError::PageOutOfRange`] and the encryption and certification
    ///   guards.
    pub fn ungroup_form_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
    ) -> Result<FormUngroupOutcome, EditError> {
        self.form_group_guards()?;
        let (leaf, form) = self.form_leaf_at(page_index, leaf_index)?;
        if leaf.containment.len() < 2 {
            let done = self.ungroup_form(page_index, leaf.paint_order)?;
            return Ok(FormUngroupOutcome {
                form: done.form,
                container: None,
                clipped: done.clipped,
                renamed: done.renamed,
                invocations: 1,
                pages: 1,
                disclosures: Vec::new(),
            });
        }
        let at = self
            .nested_do(page_index, &leaf)?
            .ok_or(GroupError::Unportable {
                reason: "its Do was not found in the form that paints it",
            })?;
        let shape = self.form_shape(form)?;
        let content =
            ContentStream::from_form(&self.view(), form).map_err(|_| GroupError::Unportable {
                reason: "its content does not parse",
            })?;
        let (stream, form_dict, resources) = self.form_with_own_resources(at.parent)?;
        let clipped = !(shape.group && self.invocation_fits(page_index, &leaf, shape.bbox)?);
        let (body, bind, renamed) = {
            let graph = self.graph();
            let mut merge = Merge {
                graph: &graph,
                own: shape.resources,
                page: &resources,
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
        let mut out = buf.get(..at.span.start).unwrap_or_default().to_vec();
        out.extend_from_slice(&body);
        out.extend_from_slice(buf.get(at.span.start + at.span.len..).unwrap_or_default());
        let (writes, form_dict, shared) =
            self.bind_in_form(at.parent, &form_dict, bind.into_iter().collect());
        let reach = self.form_invocation_reach(at.parent);
        let mut command =
            self.form_edit_command_kind(CommandKind::UngroupForm, at.parent, &form_dict, out);
        command.objects.extend(writes);
        self.commit(command);
        Ok(FormUngroupOutcome {
            form,
            container: Some(at.parent),
            clipped,
            renamed,
            invocations: reach.invocations,
            pages: reach.pages,
            disclosures: shared_note(shared),
        })
    }

    fn form_group_guards(&mut self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification().map(|_| ())
    }

    /// A form's current content, its dictionary and its own `/Resources`.
    fn form_with_own_resources(
        &self,
        form: ObjId,
    ) -> Result<(ContentStream, Dict, Dict), EditError> {
        let stream =
            ContentStream::from_form(&self.view(), form).map_err(EditError::VectorEditContent)?;
        let Some(Object::Stream(s)) = self.value(form) else {
            return Err(EditError::NotADictionary {
                id: form,
                key: "Subtype",
            });
        };
        let dict = s.dict.clone();
        let resources = match dict.get(b"Resources") {
            Some(r) => self.resolve_value(r).as_dict().cloned().unwrap_or_default(),
            None => return Err(EditError::FormInheritsResources { form }),
        };
        Ok((stream, dict, resources))
    }

    /// Writes binding each `(category, name)` in `form`'s own `/Resources`,
    /// the form's dictionary with any binding made inside it, and whether a
    /// binding went into an object other content may share.
    fn bind_in_form(
        &self,
        form: ObjId,
        form_dict: &Dict,
        binds: Vec<(ResourceKey, Object)>,
    ) -> (Vec<ObjectWrite>, Dict, bool) {
        let mut scratch: BTreeMap<ObjId, Object> = BTreeMap::new();
        let removed = HashSet::new();
        let mut any_shared = false;
        for ((category, name), value) in binds {
            let pending = PendingGraph {
                session: self,
                scratch: &scratch,
                removed: &removed,
            };
            let (writes, shared) = crate::text_edit::addtext::bind_resource(
                &pending, form, false, category, &name, value,
            );
            any_shared |= shared;
            scratch.extend(writes);
        }
        let mut dict = form_dict.clone();
        // A direct `/Resources` is rebuilt inside the form's own dictionary,
        // which travels with the form's new content.
        if let Some(Object::Dict(bound)) = scratch.remove(&form)
            && let Some(r) = bound.get(b"Resources")
        {
            dict.insert(crate::object::Name::from(b"Resources"), r.clone());
        }
        let writes = scratch
            .into_iter()
            .map(|(id, after)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            })
            .collect();
        (writes, dict, any_shared)
    }

    /// The form's `/BBox` in the form's own space, when it is four numbers.
    fn form_bbox(&self, form_dict: &Dict) -> Option<Bounds> {
        let graph = self.graph();
        let numbers: Option<Vec<f64>> = graph
            .resolve(form_dict.get(b"BBox")?)
            .as_array()?
            .iter()
            .map(|o| graph.resolve(o).as_number())
            .collect();
        let b: [f64; 4] = numbers?.try_into().ok()?;
        Some(
            Bounds::EMPTY
                .union_point(Point::new(b[0], b[1]))
                .union_point(Point::new(b[2], b[3])),
        )
    }

    /// Where the form `leaf` is directly inside is painted in its parent's
    /// stream: the `Do` whose placement is the leaf's.
    fn nested_do(
        &mut self,
        page_index: usize,
        leaf: &FormLeaf,
    ) -> Result<Option<NestedDo>, EditError> {
        let model = self.page_objects(page_index)?;
        let Some(VectorObject::Image(root)) = model.objects.get(leaf.paint_order) else {
            return Ok(None);
        };
        if root.xobject != leaf.containment.first().copied() {
            return Ok(None);
        }
        Ok(self.descend(page_index, leaf, 0, root.ctm))
    }

    /// Search `leaf.containment[depth]`, placed by `placement`, for the
    /// chain's next form, down to the `Do` that placed the leaf's own.
    /// The chain is at most `MAX_FORM_DEPTH` long and names no form twice.
    fn descend(
        &self,
        page_index: usize,
        leaf: &FormLeaf,
        depth: usize,
        placement: Matrix,
    ) -> Option<NestedDo> {
        let parent = *leaf.containment.get(depth)?;
        let next = *leaf.containment.get(depth + 1)?;
        let last = depth + 2 == leaf.containment.len();
        let stream = ContentStream::from_form(&self.view(), parent).ok()?;
        let Some(Object::Stream(s)) = self.value(parent) else {
            return None;
        };
        let at = FormLeaf {
            placement,
            ..leaf.clone()
        };
        let model = self.form_model(page_index, &stream, &s.dict, &at);
        for object in &model.objects {
            let VectorObject::Image(i) = object else {
                continue;
            };
            if i.source != ImageSource::Form || i.xobject != Some(next) {
                continue;
            }
            if last {
                if i.ctm == leaf.placement {
                    return Some(NestedDo {
                        parent,
                        span: i.bytes,
                    });
                }
            } else if let Some(found) = self.descend(page_index, leaf, depth + 1, i.ctm) {
                return Some(found);
            }
        }
        None
    }

    /// Whether everything this invocation of the leaf's form paints lies
    /// inside `bbox`. Leaves of another invocation of the same form are
    /// measured through this one's placement, so they can only answer no.
    fn invocation_fits(
        &mut self,
        page_index: usize,
        leaf: &FormLeaf,
        bbox: [f64; 4],
    ) -> Result<bool, EditError> {
        let model = self.page_objects(page_index)?;
        Ok(leaves_fit(
            model.leaves.iter().filter(|l| {
                l.paint_order == leaf.paint_order && l.containment.starts_with(&leaf.containment)
            }),
            leaf.placement,
            bbox,
        ))
    }
}

fn shared_note(shared: bool) -> Vec<String> {
    if shared {
        vec![SHARED_RESOURCES_NOTE.to_owned()]
    } else {
        Vec::new()
    }
}
