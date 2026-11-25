//! Associative anchors on the edit session (pdfcer-gui request G332): bind a
//! ce dimension's measured point to another ce dimension's centre geometry,
//! and re-solve dependents whenever a target's geometry changes. The model
//! and solver are [`crate::dimension::anchor`].

use super::{Command, CommandKind, EditError, EditSession, VertexEdit};
use crate::crypto::PermissionBit;
use crate::dimension::{
    Anchor, AnchorOutcome, AnchorState, DimensionId, DimensionKind, DimensionModel,
    DimensionRecord, GroupId, Resolved, solve_anchors, would_cycle,
};
use crate::object::{ObjId, Object};

impl EditSession {
    /// Anchor measured point `point` of `dimension` to `anchor`, or clear its
    /// anchor with `None`, and solve it at once. ONE undoable command.
    ///
    /// The point moves onto the target and from then on follows it: a
    /// move, rotate or vertex edit of the target re-solves every ce
    /// dimension anchored to it in the same undo entry, and
    /// [`Self::last_anchor_resolutions`] lists them with their before and
    /// after labels. Anchorable points are
    /// [`DimensionKind::anchor_point_count`]; a perimeter vertex or angular
    /// apex anchors too. Clearing leaves the point where it was.
    ///
    /// An anchor that cannot be solved (a nearest rim point with the other
    /// end on the centre) is still stored, the point keeps its position, and
    /// the outcome's state says why.
    ///
    /// # Errors
    ///
    /// [`EditError::DimensionNotFound`] for an unknown ce dimension or
    /// target; [`EditError::AnchorPointOutOfRange`];
    /// [`EditError::AnchorRoleNotOnTarget`];
    /// [`EditError::AnchorConditionNeedsLinear`] for a nearest or farthest
    /// rim point on a non-linear ce dimension; [`EditError::AnchorCycle`];
    /// [`EditError::AnchorAcrossPages`]; plus the locked, encryption,
    /// enforced-certification and newer-sidecar guards. Every refusal
    /// happens before anything changes.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::dimension::{Anchor, AnchorRole, DimensionId};
    /// # use pdfcer_core::edit::EditSession;
    /// # fn demo(session: &mut EditSession) -> Result<(), pdfcer_core::edit::EditError> {
    /// // Point 0 of ce dimension 3 follows the centre of mark 1.
    /// let anchor = Anchor::new(DimensionId(1), AnchorRole::MarkCentre);
    /// let outcome = session.set_dimension_anchor(DimensionId(3), 0, Some(anchor))?;
    /// println!("{} -> {}", outcome.before_label, outcome.after_label);
    /// # Ok(()) }
    /// ```
    pub fn set_dimension_anchor(
        &mut self,
        dimension: DimensionId,
        point: usize,
        anchor: Option<Anchor>,
    ) -> Result<AnchorOutcome, EditError> {
        self.anchor_guards()?;
        let mut model = self.read_dimension_model();
        let record = model
            .dimension(dimension)
            .ok_or(EditError::DimensionNotFound { id: dimension.0 })?;
        self.refuse_locked_dimension(record.annot)?;
        let page = self.dimension_page(record);
        let count = record.kind.anchor_point_count();
        if point >= count {
            return Err(EditError::AnchorPointOutOfRange {
                id: dimension.0,
                point,
                count,
            });
        }
        if let Some(a) = anchor {
            self.check_anchor(&model, (dimension, &record.kind, page), a)?;
        }
        let before_label = self.measured_label(&model, dimension);
        if let Some(d) = model.dimension_mut(dimension) {
            d.anchors.retain(|(p, _)| *p != point);
            if let Some(a) = anchor {
                d.anchors.push((point, a));
                d.anchors.sort_by_key(|(p, _)| *p);
            }
        }
        let state = solve_record(&mut model, dimension).and_then(|states| {
            let r = model.dimension(dimension)?;
            r.anchors
                .iter()
                .position(|(p, _)| *p == point)
                .and_then(|i| states.get(i).copied())
        });
        let mut objects = self.regenerate_dimension_writes(&model, &[dimension])?;
        objects.push(self.catalog_dimension_write(&model)?);
        self.commit(Command {
            kind: CommandKind::SetDimensionAnchor {
                attached: anchor.is_some(),
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        let after_label = self.measured_label(&model, dimension);
        Ok(AnchorOutcome::new(before_label, after_label, state))
    }

    /// [`Self::add_dimension`] with `anchors` bound and solved before the
    /// first bake, as ONE undoable command, so a shell that placed the ce
    /// dimension on snapped centre geometry gets it associative in one step.
    /// `kind`'s anchored points are replaced by their solved positions.
    ///
    /// # Errors
    ///
    /// Everything [`Self::add_dimension`] and
    /// [`Self::set_dimension_anchor`] refuse, before anything is written.
    pub fn add_dimension_anchored(
        &mut self,
        page_index: usize,
        group: GroupId,
        kind: DimensionKind,
        anchors: &[(usize, Anchor)],
    ) -> Result<(ObjId, DimensionId), EditError> {
        if anchors.is_empty() {
            return self.add_dimension(page_index, group, kind);
        }
        self.anchor_guards()?;
        let model = self.read_dimension_model();
        let id = model.next_dimension_id();
        let page = self.page_slots()?.get(page_index).map(|s| s.id);
        let mut bound: Vec<(usize, Anchor)> = Vec::new();
        for &(point, a) in anchors {
            let count = kind.anchor_point_count();
            if point >= count {
                return Err(EditError::AnchorPointOutOfRange {
                    id: id.0,
                    point,
                    count,
                });
            }
            self.check_anchor(&model, (id, &kind, page), a)?;
            bound.retain(|(p, _)| *p != point);
            bound.push((point, a));
        }
        bound.sort_by_key(|(p, _)| *p);
        let probe = DimensionRecord {
            id,
            group,
            kind,
            annot: None,
            ap: None,
            style: crate::dimension::StyleOverrides::default(),
            label_override: None,
            anchors: bound.clone(),
        };
        let (solved, _) = solve_anchors(&model, &probe);

        let (annot, new_id) = self.add_dimension(page_index, group, solved)?;
        let mut model = self.read_dimension_model();
        if let Some(d) = model.dimension_mut(new_id) {
            d.anchors = bound;
        }
        let objects = vec![self.catalog_dimension_write(&model)?];
        self.commit(Command {
            kind: CommandKind::SetDimensionAnchor { attached: true },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        self.coalesce_last(2, CommandKind::AddDimension);
        Ok((annot, new_id))
    }

    /// Each anchor of `dimension` as `(point, anchor, state)`, solved
    /// against the document as it stands; empty for an unknown id.
    #[must_use]
    pub fn dimension_anchors(&self, dimension: DimensionId) -> Vec<(usize, Anchor, AnchorState)> {
        let model = self.read_dimension_model();
        let Some(record) = model.dimension(dimension) else {
            return Vec::new();
        };
        let (_, states) = solve_anchors(&model, record);
        record
            .anchors
            .iter()
            .zip(states)
            .map(|(&(p, a), s)| (p, a, s))
            .collect()
    }

    /// Every ce dimension with a point anchored to `target`.
    #[must_use]
    pub fn anchor_dependents(&self, target: DimensionId) -> Vec<DimensionId> {
        self.read_dimension_model().anchor_dependents(target)
    }

    /// The ce dimensions the last command re-solved through their anchors
    /// (a target moved, rotated or had a vertex edited) or detached (a
    /// target was deleted). Cleared by every commit, undo and redo, so it
    /// always describes the newest command only. Shells show it off-canvas:
    /// these are values that changed on ce dimensions the operator did not
    /// select.
    #[must_use]
    pub fn last_anchor_resolutions(&self) -> &[Resolved] {
        &self.anchor_report
    }

    /// After `edited`'s geometry changed in `model`: re-solve its own
    /// anchors (an anchored point stays on its target) and every ce
    /// dimension anchored to it. The ids whose drawing must be regenerated,
    /// `edited` first, and the report for the dependents. Targets cannot
    /// carry anchors, so one level is the whole chain.
    pub(super) fn follow_anchors(
        &self,
        model: &mut DimensionModel,
        edited: DimensionId,
    ) -> (Vec<DimensionId>, Vec<Resolved>) {
        solve_record(model, edited);
        let mut ids = vec![edited];
        let mut report = Vec::new();
        for dep in model.anchor_dependents(edited) {
            if dep == edited {
                continue;
            }
            let before = model.dimension(dep).map(|r| r.kind.clone());
            let before_label = self.measured_label(model, dep);
            solve_record(model, dep);
            if model.dimension(dep).map(|r| &r.kind) != before.as_ref() {
                ids.push(dep);
            }
            let after_label = self.measured_label(model, dep);
            report.push(Resolved::new(dep, before_label, after_label));
        }
        (ids, report)
    }

    /// Clear every anchor to `target` before it is deleted; the report.
    pub(super) fn detach_for_delete(
        &self,
        model: &mut DimensionModel,
        target: DimensionId,
    ) -> Vec<Resolved> {
        model
            .detach_anchors_to(target)
            .into_iter()
            .map(|id| Resolved::detached(id, self.measured_label(model, id)))
            .collect()
    }

    fn anchor_guards(&self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.check_dimension_sidecar()
    }

    /// Refuse `anchor` for the ce dimension `(id, kind, page)`.
    fn check_anchor(
        &self,
        model: &DimensionModel,
        (id, kind, page): (DimensionId, &DimensionKind, Option<ObjId>),
        anchor: Anchor,
    ) -> Result<(), EditError> {
        let target = model
            .dimension(anchor.target)
            .ok_or(EditError::DimensionNotFound {
                id: anchor.target.0,
            })?;
        if would_cycle(model, id, anchor.target) {
            return Err(EditError::AnchorCycle {
                id: id.0,
                target: anchor.target.0,
            });
        }
        if !anchor.role.fits(&target.kind) {
            return Err(EditError::AnchorRoleNotOnTarget {
                target: anchor.target.0,
            });
        }
        if anchor.role.needs_other_end() && !matches!(kind, DimensionKind::Linear { .. }) {
            return Err(EditError::AnchorConditionNeedsLinear { id: id.0 });
        }
        match (page, self.dimension_page(target)) {
            (Some(a), Some(b)) if a != b => Err(EditError::AnchorAcrossPages {
                id: id.0,
                target: anchor.target.0,
            }),
            _ => Ok(()),
        }
    }

    /// The page a wired ce dimension is on, from its annotation's `/P`.
    fn dimension_page(&self, record: &DimensionRecord) -> Option<ObjId> {
        self.value(record.annot?)?
            .as_dict()?
            .get(b"P")
            .and_then(Object::as_reference)
    }

    /// The caption `id` prints for its measurement (empty for centre
    /// geometry or an unknown id), from the same producer the `/AP` uses.
    fn measured_label(&self, model: &DimensionModel, id: DimensionId) -> String {
        let Some(record) = model.dimension(id) else {
            return String::new();
        };
        let Some(group) = model.group(record.group) else {
            return String::new();
        };
        if record.kind.is_centre_geometry() {
            return String::new();
        }
        let style = crate::dimension::resolve_style(group, &record.style)
            .on_user_unit(self.dimension_user_unit(record));
        record
            .kind
            .display_on(style.user_unit, style.scale, style.format)
            .text
    }
}

/// Solve `id`'s anchors in place; their states, or `None` for an unknown id.
fn solve_record(model: &mut DimensionModel, id: DimensionId) -> Option<Vec<AnchorState>> {
    let record = model.dimension(id)?;
    if record.anchors.is_empty() {
        return Some(Vec::new());
    }
    let (kind, states) = solve_anchors(model, record);
    if let Some(d) = model.dimension_mut(id) {
        d.kind = kind;
    }
    Some(states)
}

/// Keep `anchors` on the right points across a vertex edit: dragging an
/// anchored point detaches it (the operator moved it off the target), and
/// an inserted or removed vertex shifts the later indices.
pub(super) fn reindex_anchors(anchors: &mut Vec<(usize, Anchor)>, edit: VertexEdit) {
    match edit {
        VertexEdit::Move { index, .. } => anchors.retain(|(p, _)| *p != index),
        VertexEdit::Insert { after, .. } => {
            for (p, _) in anchors.iter_mut() {
                if *p > after {
                    *p += 1;
                }
            }
        }
        VertexEdit::Remove { index } => {
            anchors.retain(|(p, _)| *p != index);
            for (p, _) in anchors.iter_mut() {
                if *p > index {
                    *p -= 1;
                }
            }
        }
    }
}
