//! Associative anchors across a clipboard paste: each pasted ce dimension's
//! anchors are re-pointed at the pasted copy of their target, or dropped and
//! disclosed when the target was not in the clip.

use std::collections::BTreeMap;

use super::dimension_anchor::solve_record;
use super::{Command, CommandKind, EditError, EditSession};
use crate::dimension::{Anchor, DimensionId, Resolved};
use crate::vector::{ClipAnnotation, ObjectClip};

/// Whether some ce dimension in `clip` is anchored.
pub(super) fn clip_carries_anchors(clip: &ObjectClip) -> bool {
    carries_anchors(&clip.annotations)
}

fn carries_anchors(annotations: &[ClipAnnotation]) -> bool {
    annotations
        .iter()
        .any(|a| matches!(a, ClipAnnotation::Dimension { anchors, .. } if !anchors.is_empty()))
}

/// Clear every ce dimension's `source` when no ce dimension in the clip is
/// anchored: the ids exist only to re-point anchors, and an unanchored clip
/// is written below format 6, which has no slot for them.
pub(super) fn drop_unused_sources(annotations: &mut [ClipAnnotation]) {
    if carries_anchors(annotations) {
        return;
    }
    for a in annotations {
        if let ClipAnnotation::Dimension { source, .. } = a {
            *source = None;
        }
    }
}

impl EditSession {
    /// Re-point the anchors of the ce dimensions a paste placed, as ONE
    /// command; the report, one entry per anchored ce dimension.
    ///
    /// `pasted` maps a source id in the clip to the ce dimension placed for
    /// it; `anchored` is each placed ce dimension with its anchors as copied
    /// (targets named by source id). An anchor whose target was placed
    /// follows it; one whose target is not in the clip, or which no longer
    /// passes the anchor checks (a clip read from disk is untrusted), is
    /// dropped, its ce dimension reported detached and a disclosure pushed.
    pub(super) fn attach_pasted_anchors(
        &mut self,
        pasted: &BTreeMap<DimensionId, DimensionId>,
        anchored: &[(DimensionId, Vec<(usize, Anchor)>)],
        disclosures: &mut Vec<String>,
    ) -> Result<Vec<Resolved>, EditError> {
        if anchored.is_empty() {
            return Ok(Vec::new());
        }
        let mut model = self.read_dimension_model();
        let mut report = Vec::new();
        for (id, anchors) in anchored {
            let before_label = self.measured_label(&model, *id);
            let mut kept = Vec::new();
            for &(point, anchor) in anchors {
                let Some(&target) = pasted.get(&anchor.target) else {
                    continue;
                };
                let remapped = Anchor::new(target, anchor.role);
                let Some(record) = model.dimension(*id) else {
                    continue;
                };
                let page = self.dimension_page(record);
                if self
                    .check_anchor(&model, (*id, &record.kind, page), remapped)
                    .is_ok()
                {
                    kept.push((point, remapped));
                }
            }
            let dropped = anchors.len() - kept.len();
            if let Some(record) = model.dimension_mut(*id) {
                record.anchors = kept;
            }
            solve_record(&mut model, *id);
            let after_label = self.measured_label(&model, *id);
            if dropped == 0 {
                report.push(Resolved::new(*id, before_label, after_label));
                continue;
            }
            disclosures.push(format!(
                "paste: ce dimension {} was anchored to centre geometry that was not copied with it, so {dropped} of its {} anchor(s) were dropped; it keeps its pasted position and no longer follows that target.",
                id.0,
                anchors.len()
            ));
            report.push(Resolved::detached(*id, after_label));
        }
        let ids: Vec<DimensionId> = anchored.iter().map(|(id, _)| *id).collect();
        let mut objects = self.regenerate_dimension_writes(&model, &ids)?;
        objects.push(self.catalog_dimension_write(&model)?);
        self.commit(Command {
            kind: CommandKind::PasteObjects,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(report)
    }
}
