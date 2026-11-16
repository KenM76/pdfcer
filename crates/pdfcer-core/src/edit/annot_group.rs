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
        self.write_group_links(CommandKind::GroupAnnotations, &joined, |d| {
            d.insert(Name::from(b"IRT"), Object::Reference(primary));
            d.insert(Name::from(b"RT"), Object::Name(Name::from(b"Group")));
        })?;
        Ok(AnnotationGroupChange {
            primary,
            members: joined,
        })
    }

    /// Dissolve the annotation group `annotation` belongs to, named by its
    /// primary or any member: `/IRT` and `/RT` are removed from every
    /// subordinate — one command, one undo entry
    /// ([`CommandKind::UngroupAnnotations`]). Replies (`/RT /R`, or `/IRT`
    /// alone) are not group links and are left alone.
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
        self.write_group_links(CommandKind::UngroupAnnotations, &released, |d| {
            d.remove(b"IRT");
            d.remove(b"RT");
        })?;
        Ok(AnnotationGroupChange {
            primary,
            members: released,
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
        edit: impl Fn(&mut Dict),
    ) -> Result<(), EditError> {
        let mut objects = Vec::with_capacity(ids.len());
        for &id in ids {
            let Some(Object::Dict(dict)) = self.value(id) else {
                return Err(EditError::NotADictionary { id, key: "IRT" });
            };
            let mut updated = dict.clone();
            edit(&mut updated);
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
