//! Font work in an optimize run: removing embedded font programs through
//! [`crate::font_unembed`], planned with the rest of the run and committed
//! in its one undo entry.

use std::collections::BTreeSet;

use super::optimize_cleanup::Fold;
use super::{EditError, EditSession};
use crate::font_unembed::{PdfaClaim, UnembedPlan};
use crate::object::ObjId;
use crate::optimize::{OptimizeError, OptimizePlan, OptimizeSettings};

impl EditSession {
    /// Plan [`OptimizeSettings::unembed`] into `plan`. A freed program is
    /// dropped from the stream changes, since nothing is left to recompress.
    pub(super) fn plan_unembed(
        &self,
        plan: &mut OptimizePlan,
        settings: &OptimizeSettings,
        notes: &mut Vec<String>,
    ) {
        let Some(request) = &settings.unembed else {
            return;
        };
        let preview = self.unembed_preview(request);
        if let PdfaClaim::Identified { .. } = preview.pdfa {
            notes.push(
                "fonts kept embedded: the document identifies itself as PDF/A, which \
requires embedded fonts"
                    .to_owned(),
            );
            return;
        }
        notes.extend(unembed_notes(&preview));
        let freed = freed_ids(&preview);
        plan.changes.retain(|c| !freed.contains(&c.id));
        plan.writes.retain(|w| !freed.contains(&w.id));
        plan.skipped.retain(|s| !freed.contains(&s.id));
        plan.unembed = Some(preview);
        plan.unembed_request = Some(request.clone());
    }

    /// [`OptimizeError::StalePlan`] when the fonts the plan unembeds would
    /// now be planned differently.
    pub(super) fn check_unembed_fresh(&self, plan: &OptimizePlan) -> Result<(), OptimizeError> {
        let (Some(request), Some(planned)) = (&plan.unembed_request, &plan.unembed) else {
            return Ok(());
        };
        if self.unembed_preview(request) == *planned {
            return Ok(());
        }
        let id = planned
            .targets
            .first()
            .map(|t| t.id)
            .or_else(|| planned.blocked.iter().find_map(|b| b.id))
            .unwrap_or(ObjId {
                num: 0,
                generation: 0,
            });
        Err(OptimizeError::StalePlan { id })
    }

    /// Unembed the planned fonts into the run's undo entry; the number of
    /// fonts unembedded.
    pub(super) fn apply_unembed(
        &mut self,
        plan: &OptimizePlan,
        fold: &mut Fold,
    ) -> Result<usize, OptimizeError> {
        let (Some(request), Some(planned)) = (&plan.unembed_request, &plan.unembed) else {
            return Ok(0);
        };
        if planned.is_empty() {
            return Ok(0);
        }
        match self.unembed_fonts(request) {
            Ok(done) => {
                fold.absorb(self);
                Ok(done.targets.len())
            }
            Err(EditError::NoFontsToUnembed { .. }) => Ok(0),
            Err(e) => {
                fold.roll_back(self);
                Err(OptimizeError::UnembedFailed {
                    reason: e.to_string(),
                })
            }
        }
    }
}

/// The program and `/CIDSet` objects the plan frees.
fn freed_ids(plan: &UnembedPlan) -> BTreeSet<ObjId> {
    plan.targets
        .iter()
        .flat_map(|t| {
            t.program_id
                .filter(|_| t.program_freed)
                .into_iter()
                .chain(t.cid_set_id)
        })
        .collect()
}

/// The disclosures for `plan`: the appearance change, the renames, shared
/// programs left in the file, and names that matched nothing.
fn unembed_notes(plan: &UnembedPlan) -> Vec<String> {
    let mut notes = Vec::new();
    if !plan.targets.is_empty() {
        notes.push(format!(
            "{} font(s) unembedded: pages using them will look different, drawn with whatever \
face the reader substitutes; each glyph keeps its advance",
            plan.targets.len()
        ));
    }
    if plan.renames_any() {
        notes.push(
            "the six-letter subset tag is removed from each unembedded font's name, so a \
reader can match an installed face"
                .to_owned(),
        );
    }
    let shared = plan.targets.iter().filter(|t| !t.program_freed).count();
    if shared > 0 {
        notes.push(format!(
            "{shared} unembedded font(s) share a program with a font that stays embedded; \
those bytes stay"
        ));
    }
    for name in &plan.unmatched {
        notes.push(format!("no font is named {name:?}"));
    }
    notes
}

#[cfg(test)]
#[path = "optimize_fonts_tests.rs"]
mod tests;
