//! Grouping markup annotations (ISO 32000-1 §12.5.6.2: `/IRT` to a primary
//! with `/RT /Group`) and dissolving such a group (pdfcer-gui request G280).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, PermissionBit};
use crate::annot::Annotation;
use crate::object::{Dict, Name, ObjId, Object};

/// The annotation types that carry Table 170's markup entries, `/IRT` and
/// `/RT` among them (§12.5.6.1 Table 169, plus PDF 2.0's `Projection`).
const MARKUP: &[&[u8]] = &[
    b"Text",
    b"FreeText",
    b"Line",
    b"Square",
    b"Circle",
    b"Polygon",
    b"PolyLine",
    b"Highlight",
    b"Underline",
    b"Squiggly",
    b"StrikeOut",
    b"Stamp",
    b"Caret",
    b"Ink",
    b"FileAttachment",
    b"Sound",
    b"Redact",
    b"Projection",
];

/// The annotation entry listing the groups an annotation was in before
/// [`EditSession::merge_annotation_groups`] put it under another primary,
/// as references to their primaries, outermost last. Private to pdfcer:
/// a §12.5.6.2 group is flat, so the file format has no place for it.
const NESTING_KEY: &[u8] = b"pdfcer_Groups";

/// What [`EditSession::merge_annotation_groups`] keeps of the groups it
/// merges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GroupNesting {
    /// Each merged group is recorded on its annotations, and
    /// [`EditSession::ungroup_annotations`] restores it.
    #[default]
    Recorded,
    /// Nothing is recorded: ungrouping releases every annotation.
    Flat,
}

/// One annotation [`EditSession::merge_annotation_groups`] rewrites.
struct Absorbed {
    id: ObjId,
    /// The group it leaves, pushed onto its record.
    was: Option<ObjId>,
    /// Whether the record it already carries is still true.
    keep: bool,
}

/// Why annotations could not be grouped or ungrouped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnnotationGroupError {
    /// No member was named.
    #[error("no annotations were given to group with {primary}")]
    NoMembers {
        /// The primary.
        primary: ObjId,
    },
    /// The primary was also named as a member.
    #[error("annotation {id} is the group's primary and cannot also be a member")]
    MemberIsPrimary {
        /// The primary.
        id: ObjId,
    },
    /// `/IRT` and `/RT` are markup-annotation entries; this type has none.
    #[error(
        "annotation {id} is a {subtype}, which is not a markup annotation and cannot be grouped"
    )]
    NotMarkup {
        /// The annotation.
        id: ObjId,
        /// Its `/Subtype`.
        subtype: String,
    },
    /// A ce dimension is grouped through its own dimension group.
    #[error("annotation {id} is a ce dimension; it groups through its dimension group")]
    CeDimension {
        /// The annotation.
        id: ObjId,
    },
    /// The member is on another page than the primary.
    #[error("annotation {id} is not on the primary's page")]
    DifferentPage {
        /// The member.
        id: ObjId,
    },
    /// The annotation already has `/IRT`: it is a reply, or a member of a
    /// group. It has one `/IRT`, so joining would drop that link.
    #[error(
        "annotation {id} is already a reply to, or grouped under, annotation {target}; \
         ungroup it first"
    )]
    AlreadyLinked {
        /// The annotation.
        id: ObjId,
        /// What its `/IRT` names.
        target: ObjId,
    },
    /// The member is the primary of a group of its own; a group's primary
    /// is not a subordinate (§12.5.6.2).
    #[error("annotation {id} is the primary of another group; ungroup that group first")]
    MemberHasGroup {
        /// The member.
        id: ObjId,
    },
    /// The annotation is in no group.
    #[error("annotation {id} is not in a group")]
    NotGrouped {
        /// The annotation.
        id: ObjId,
    },
}

/// What [`EditSession::group_annotations`] and
/// [`EditSession::ungroup_annotations`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnnotationGroupChange {
    /// The group's primary.
    pub primary: ObjId,
    /// The subordinates written: those joined, or those released.
    pub members: Vec<ObjId>,
    /// The primaries of the earlier groups an ungroup put back, in the
    /// order first met. Empty for a group or a merge.
    pub restored: Vec<ObjId>,
    /// What the operator should be told: how a merge recorded the groups
    /// it merged, and any recorded group that could not be put back.
    pub disclosures: Vec<String>,
}

impl EditSession {
    /// Group markup annotations under `primary`: each of `members` gets
    /// `/IRT primary` and `/RT /Group` (ISO 32000-1 §12.5.6.2, PDF 1.6) —
    /// one command, one undo entry ([`CommandKind::GroupAnnotations`]).
    /// Repeats in `members` are ignored; a member already grouped under
    /// `primary` is left as it is and not reported.
    ///
    /// Nothing else is written. A conforming reader takes the group's
    /// `Contents`, `T`, `M`, `C`, `Popup`, `CreationDate`, `Subj` and `Open`
    /// from the primary and ignores the members' own, which stay in the
    /// file and are in force again after [`Self::ungroup_annotations`].
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`AnnotationGroupError`] (as [`EditError::AnnotationGroup`]), naming
    ///   the first annotation that cannot take part.
    /// - [`EditError::AnnotationNotFound`] for an id on no page.
    /// - [`EditError::DocumentEncrypted`] and the certification gate.
    pub fn group_annotations(
        &mut self,
        primary: ObjId,
        members: &[ObjId],
    ) -> Result<AnnotationGroupChange, EditError> {
        self.annotation_group_guards()?;
        let pages = self.annotations_by_page()?;
        let (page, head) = find(&pages, primary)?;
        self.groupable(head, primary)?;
        if let Some(target) = head.in_reply_to {
            return Err(AnnotationGroupError::AlreadyLinked {
                id: primary,
                target,
            }
            .into());
        }
        let mut joined = Vec::new();
        for &id in members {
            if id == primary {
                return Err(AnnotationGroupError::MemberIsPrimary { id }.into());
            }
            let (at, member) = find(&pages, id)?;
            self.groupable(member, id)?;
            if at != page {
                return Err(AnnotationGroupError::DifferentPage { id }.into());
            }
            if joined.contains(&id)
                || (member.in_reply_to == Some(primary) && member.is_group_subordinate())
            {
                continue;
            }
            if let Some(target) = member.in_reply_to {
                return Err(AnnotationGroupError::AlreadyLinked { id, target }.into());
            }
            if subordinates(&pages, id).next().is_some() {
                return Err(AnnotationGroupError::MemberHasGroup { id }.into());
            }
            joined.push(id);
        }
        if members.is_empty() {
            return Err(AnnotationGroupError::NoMembers { primary }.into());
        }
        self.write_group_links(CommandKind::GroupAnnotations, &joined, |_, d| {
            link(d, primary);
            d.remove(NESTING_KEY);
        })?;
        Ok(AnnotationGroupChange {
            primary,
            members: joined,
            restored: Vec::new(),
            disclosures: Vec::new(),
        })
    }

    /// Put whole groups under one primary: each of `others` brings the
    /// group it is in, primary and members, or itself when it is in none.
    /// `primary` may be a member too; its group's primary is the one kept.
    /// One command, one undo entry ([`CommandKind::GroupAnnotations`]).
    ///
    /// ISO 32000-1 §12.5.6.2 has no group of groups, so the file holds one
    /// flat group either way. With [`GroupNesting::Recorded`] every
    /// annotation of a merged group, the primary's own included, also
    /// records that group in a pdfcer-private entry, and
    /// [`Self::ungroup_annotations`] puts the groups back instead of
    /// releasing their members; the outcome discloses this. `members` is
    /// every annotation written. Nothing to merge writes nothing.
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`AnnotationGroupError`]: `NoMembers`, `NotMarkup`, `CeDimension`,
    ///   `DifferentPage`, and `AlreadyLinked` for a reply or for a primary
    ///   that is itself a reply.
    /// - [`EditError::AnnotationNotFound`] for an id on no page.
    /// - [`EditError::DocumentEncrypted`] and the certification gate.
    pub fn merge_annotation_groups(
        &mut self,
        primary: ObjId,
        others: &[ObjId],
        nesting: GroupNesting,
    ) -> Result<AnnotationGroupChange, EditError> {
        self.annotation_group_guards()?;
        let pages = self.annotations_by_page()?;
        let (page, head) = self.group_head(&pages, primary)?;
        let mut groups = Vec::new();
        for &id in others {
            let (at, group) = self.group_head(&pages, id)?;
            if at != page {
                return Err(AnnotationGroupError::DifferentPage { id }.into());
            }
            if group != head && !groups.contains(&group) {
                groups.push(group);
            }
        }
        if others.is_empty() {
            return Err(AnnotationGroupError::NoMembers { primary }.into());
        }
        let recorded = nesting == GroupNesting::Recorded;
        let (plan, merged) = merge_plan(&pages, head, &groups, recorded);
        let ids: Vec<ObjId> = plan.iter().map(|a| a.id).collect();
        self.write_group_links(CommandKind::GroupAnnotations, &ids, |id, d| {
            let Some(step) = plan.iter().find(|a| a.id == id) else {
                return;
            };
            let mut record = if step.keep {
                nesting_record(d)
            } else {
                Vec::new()
            };
            record.extend(step.was);
            link(d, head);
            set_nesting_record(d, &record);
        })?;
        Ok(AnnotationGroupChange {
            primary: head,
            members: ids,
            restored: Vec::new(),
            disclosures: merge_disclosure(merged, recorded).into_iter().collect(),
        })
    }

    /// The page of `id` and the primary of the group it is in, itself when
    /// it is in none; refused when either cannot be in a group.
    fn group_head(
        &self,
        pages: &[Vec<Annotation>],
        id: ObjId,
    ) -> Result<(usize, ObjId), EditError> {
        let (page, named) = find(pages, id)?;
        self.groupable(named, id)?;
        let (head_id, head) = match named.in_reply_to {
            Some(target) if named.is_group_subordinate() => {
                let (_, head) = find(pages, target)?;
                self.groupable(head, target)?;
                (target, head)
            }
            _ => (id, named),
        };
        match head.in_reply_to {
            Some(target) => Err(AnnotationGroupError::AlreadyLinked {
                id: head_id,
                target,
            }
            .into()),
            None => Ok((page, head_id)),
        }
    }

    /// Dissolve the annotation group `annotation` belongs to, named by its
    /// primary or any member: `/IRT` and `/RT` are removed from every
    /// subordinate — one command, one undo entry
    /// ([`CommandKind::UngroupAnnotations`]). Replies (`/RT /R`, or `/IRT`
    /// alone) are not group links and are left alone.
    ///
    /// A subordinate that [`Self::merge_annotation_groups`] recorded an
    /// earlier group on goes back under that group's primary instead, so
    /// one call dissolves one level; `restored` names the groups put back.
    /// A recorded primary that is no longer in the document releases its
    /// annotation, and that is disclosed.
    ///
    /// # Errors
    ///
    /// Nothing changes on any error.
    /// - [`AnnotationGroupError::NotGrouped`] when it is in no group.
    /// - [`EditError::AnnotationNotFound`] for an id on no page.
    /// - [`EditError::DocumentEncrypted`] and the certification gate.
    pub fn ungroup_annotations(
        &mut self,
        annotation: ObjId,
    ) -> Result<AnnotationGroupChange, EditError> {
        self.annotation_group_guards()?;
        let pages = self.annotations_by_page()?;
        let (_, named) = find(&pages, annotation)?;
        let primary = match named.in_reply_to {
            Some(target) if named.is_group_subordinate() => target,
            _ => annotation,
        };
        let released: Vec<ObjId> = subordinates(&pages, primary).collect();
        if released.is_empty() {
            return Err(AnnotationGroupError::NotGrouped { id: annotation }.into());
        }
        let (mut restored, mut lost) = (Vec::new(), 0usize);
        let mut back = Vec::with_capacity(released.len());
        for &id in &released {
            let earlier = match self.value(id) {
                Some(Object::Dict(d)) => nesting_record(d).pop(),
                _ => None,
            };
            let target = earlier.filter(|&p| p != id && find(&pages, p).is_ok());
            // A record naming the annotation itself: it was that group's primary.
            lost += usize::from(earlier.is_some_and(|p| p != id) && target.is_none());
            if let Some(p) = target.filter(|p| !restored.contains(p)) {
                restored.push(p);
            }
            back.push((id, target));
        }
        self.write_group_links(CommandKind::UngroupAnnotations, &released, |id, d| {
            let mut record = nesting_record(d);
            record.pop();
            match back.iter().find(|b| b.0 == id).and_then(|b| b.1) {
                Some(earlier) => link(d, earlier),
                None => {
                    d.remove(b"IRT");
                    d.remove(b"RT");
                }
            }
            set_nesting_record(d, &record);
        })?;
        let disclosures = (lost > 0).then(|| {
            format!(
                "{lost} annotation(s) recorded an earlier group whose primary is no longer \
                 in the document; they were released"
            )
        });
        Ok(AnnotationGroupChange {
            primary,
            members: released,
            restored,
            disclosures: disclosures.into_iter().collect(),
        })
    }

    fn annotation_group_guards(&self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()
    }

    fn annotations_by_page(&self) -> Result<Vec<Vec<Annotation>>, EditError> {
        let slots = self.page_slots()?;
        let graph = self.graph();
        Ok(slots
            .iter()
            .map(|slot| crate::annot::page_annotations(&graph, slot.id))
            .collect())
    }

    fn groupable(&self, annot: &Annotation, id: ObjId) -> Result<(), EditError> {
        if !MARKUP.contains(&annot.subtype.as_slice()) {
            return Err(AnnotationGroupError::NotMarkup {
                id,
                subtype: annot.subtype_label(),
            }
            .into());
        }
        match self.value(id) {
            Some(Object::Dict(d)) if self.is_ce_dimension(id, d) => {
                Err(AnnotationGroupError::CeDimension { id }.into())
            }
            _ => Ok(()),
        }
    }

    /// One command applying `edit` to each annotation's dictionary; none
    /// when `ids` is empty.
    fn write_group_links(
        &mut self,
        kind: CommandKind,
        ids: &[ObjId],
        edit: impl Fn(ObjId, &mut Dict),
    ) -> Result<(), EditError> {
        let mut objects = Vec::with_capacity(ids.len());
        for &id in ids {
            let Some(Object::Dict(dict)) = self.value(id) else {
                return Err(EditError::NotADictionary { id, key: "IRT" });
            };
            let mut updated = dict.clone();
            edit(id, &mut updated);
            objects.push(ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(Object::Dict(updated)),
            });
        }
        if !objects.is_empty() {
            self.commit(Command {
                kind,
                objects,
                removals: Vec::new(),
                trailer: None,
            });
        }
        Ok(())
    }
}

/// Make `dict` a `/RT /Group` subordinate of `primary`.
fn link(dict: &mut Dict, primary: ObjId) {
    dict.insert(Name::from(b"IRT"), Object::Reference(primary));
    dict.insert(Name::from(b"RT"), Object::Name(Name::from(b"Group")));
}

/// The earlier groups recorded on an annotation, outermost last.
fn nesting_record(dict: &Dict) -> Vec<ObjId> {
    let Some(Object::Array(items)) = dict.get(NESTING_KEY) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|o| match o {
            Object::Reference(id) => Some(*id),
            _ => None,
        })
        .collect()
}

fn set_nesting_record(dict: &mut Dict, record: &[ObjId]) {
    if record.is_empty() {
        dict.remove(NESTING_KEY);
    } else {
        let items = record.iter().map(|&id| Object::Reference(id)).collect();
        dict.insert(Name::from(NESTING_KEY), Object::Array(items));
    }
}

/// What merging `groups` under `head` writes, and how many groups that had
/// members of their own it merges, the head's counted.
fn merge_plan(
    pages: &[Vec<Annotation>],
    head: ObjId,
    groups: &[ObjId],
    recorded: bool,
) -> (Vec<Absorbed>, usize) {
    let mut plan = Vec::new();
    let mut merged = 0usize;
    if groups.is_empty() {
        return (plan, merged);
    }
    let own: Vec<ObjId> = subordinates(pages, head).collect();
    merged += usize::from(!own.is_empty());
    if recorded {
        plan.extend(own.into_iter().map(|id| Absorbed {
            id,
            was: Some(head),
            keep: true,
        }));
    }
    for &group in groups {
        let subs: Vec<ObjId> = subordinates(pages, group).collect();
        merged += usize::from(!subs.is_empty());
        let was = (recorded && !subs.is_empty()).then_some(group);
        plan.push(Absorbed {
            id: group,
            was,
            keep: false,
        });
        plan.extend(subs.into_iter().map(|id| Absorbed {
            id,
            was,
            keep: recorded,
        }));
    }
    (plan, merged)
}

/// What a merge of `merged` existing groups tells the operator.
fn merge_disclosure(merged: usize, recorded: bool) -> Option<String> {
    if merged == 0 {
        return None;
    }
    Some(if recorded {
        format!(
            "the file holds one flat group (ISO 32000-1 12.5.6.2 has no group of groups); \
             {merged} earlier group(s) are recorded in a pdfcer-private entry that other \
             applications ignore, and ungrouping puts them back"
        )
    } else {
        format!("{merged} earlier group(s) were merged flat; ungrouping releases every member")
    })
}

/// The page index and read model of annotation `id`.
fn find(pages: &[Vec<Annotation>], id: ObjId) -> Result<(usize, &Annotation), EditError> {
    pages
        .iter()
        .enumerate()
        .find_map(|(i, annots)| Some((i, annots.iter().find(|a| a.id == Some(id))?)))
        .ok_or(EditError::AnnotationNotFound { id })
}

/// The `/RT /Group` subordinates of `primary`, in page then `/Annots` order.
fn subordinates(pages: &[Vec<Annotation>], primary: ObjId) -> impl Iterator<Item = ObjId> + '_ {
    pages
        .iter()
        .flatten()
        .filter(move |a| a.in_reply_to == Some(primary) && a.is_group_subordinate())
        .filter_map(|a| a.id)
}
