//! Font re-subsetting in an optimize run ([`crate::optimize::resubset`]):
//! planned with the rest of the run, written in its one undo entry.

use std::collections::BTreeSet;

use super::{EditSession, ObjectWrite};
use crate::font_unembed::{PdfaClaim, detect_pdfa};
use crate::fontinfo::split_subset_tag;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::optimize::resubset::{
    ResubsetFonts, ResubsetPlan, ResubsetRefusal, ResubsetRefused, ResubsetTarget,
};
use crate::optimize::{OptimizeError, OptimizePlan, OptimizeSettings};

impl EditSession {
    /// Plan [`OptimizeSettings::resubset`] into `plan`. A program another
    /// step of the run unembeds is left; a cut program is dropped from the
    /// stream changes, since the cut replaces it.
    pub(super) fn plan_resubset(
        &self,
        plan: &mut OptimizePlan,
        settings: &OptimizeSettings,
        notes: &mut Vec<String>,
    ) {
        let Some(request) = settings.resubset else {
            return;
        };
        if let PdfaClaim::Identified { .. } = detect_pdfa(&self.view()) {
            notes.push(
                "fonts not re-subset: the document identifies itself as PDF/A, and pdfcer does not yet rewrite what PDF/A requires of a subset"
                    .to_owned(),
            );
            return;
        }
        let cut = self.resubset_preview(request, plan);
        let programs: BTreeSet<ObjId> = cut.targets.iter().map(|t| t.program).collect();
        plan.changes.retain(|c| !programs.contains(&c.id));
        plan.writes.retain(|w| !programs.contains(&w.id));
        plan.skipped.retain(|s| !programs.contains(&s.id));
        notes.extend(resubset_notes(&cut));
        plan.resubset = Some(cut);
        plan.resubset_request = Some(request);
    }

    /// The re-subset `request` plans now, leaving what `plan` unembeds.
    fn resubset_preview(&self, request: ResubsetFonts, plan: &OptimizePlan) -> ResubsetPlan {
        let live = self.live_objects();
        let mut cut = crate::optimize::resubset_plan::plan(&self.view(), &live, request.source);
        let unembedded: BTreeSet<ObjId> = plan
            .unembed
            .iter()
            .flat_map(|u| u.targets.iter().map(|t| t.id))
            .collect();
        exclude_unembedded(&mut cut, &unembedded);
        cut
    }

    /// [`OptimizeError::StalePlan`] when the cut would now be planned
    /// differently: an object it rewrites changed, or the document draws
    /// other glyphs with its fonts.
    pub(super) fn check_resubset_fresh(&self, plan: &OptimizePlan) -> Result<(), OptimizeError> {
        let (Some(request), Some(cut)) = (plan.resubset_request, &plan.resubset) else {
            return Ok(());
        };
        if self.resubset_preview(request, plan) == *cut {
            return Ok(());
        }
        let id = cut
            .targets
            .iter()
            .flat_map(|t| &t.before)
            .find(|(id, before)| self.value(*id) != Some(before))
            .or_else(|| cut.targets.first().and_then(|t| t.before.first()))
            .map_or(
                ObjId {
                    num: 0,
                    generation: 0,
                },
                |(id, _)| *id,
            );
        Err(OptimizeError::StalePlan { id })
    }

    /// The cut programs and their re-tagged names, as writes.
    pub(super) fn resubset_writes(&mut self, plan: &OptimizePlan) -> Vec<ObjectWrite> {
        let Some(cut) = &plan.resubset else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for t in &cut.targets {
            let tag = t.name_after.split_once('+').map_or("", |(tag, _)| tag);
            for (id, before) in &t.before {
                let after = if *id == t.program {
                    self.cut_program(t, before)
                } else {
                    retagged(before, tag)
                };
                if let Some(after) = after {
                    out.push(ObjectWrite {
                        id: *id,
                        before: self.state.get(id).cloned(),
                        after: Some(after),
                    });
                }
            }
        }
        out
    }

    fn cut_program(&mut self, t: &ResubsetTarget, before: &Object) -> Option<Object> {
        let Object::Stream(stream) = before else {
            return None;
        };
        let mut dict = stream.dict.clone();
        for key in [
            b"DecodeParms".as_slice(),
            b"DP",
            b"F",
            b"FFilter",
            b"FDecodeParms",
        ] {
            dict.remove(key);
        }
        let int = |n: usize| Object::Integer(i64::try_from(n).unwrap_or(i64::MAX));
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );
        dict.insert(Name::from(b"Length"), int(t.data.len()));
        dict.insert(Name::from(b"Length1"), int(t.length1));
        let data_span = self.stage_bytes(&t.data);
        Some(Object::Stream(Stream { dict, data_span }))
    }
}

/// `before`, a font or descriptor dictionary, with its name under `tag`:
/// `/FontName` on a descriptor, `/BaseFont` on a font.
fn retagged(before: &Object, tag: &str) -> Option<Object> {
    let dict: &Dict = before.as_dict()?;
    let key: &[u8] = if dict.get(b"FontName").is_some() {
        b"FontName"
    } else {
        b"BaseFont"
    };
    let name = dict.get(key).and_then(Object::as_name)?;
    let old = String::from_utf8_lossy(name.as_bytes()).into_owned();
    let (_, stem) = split_subset_tag(&old);
    let mut out = dict.clone();
    let new = format!("{tag}+{stem}");
    out.insert(Name::from(key), Object::Name(Name::from(new.as_bytes())));
    Some(Object::Dict(out))
}

/// Move every target with a font in `unembedded` to the refusals.
fn exclude_unembedded(cut: &mut ResubsetPlan, unembedded: &BTreeSet<ObjId>) {
    let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut cut.targets)
        .into_iter()
        .partition(|t| t.fonts.iter().any(|f| unembedded.contains(f)));
    cut.targets = kept;
    for t in gone {
        cut.refused.extend(t.fonts.iter().map(|&f| ResubsetRefused {
            font: Some(f),
            name: t.name_before.clone(),
            why: ResubsetRefusal::new(
                "unembedded-this-run",
                "the run removes the font program instead",
            ),
        }));
    }
}

/// The disclosures for `cut`.
fn resubset_notes(cut: &ResubsetPlan) -> Vec<String> {
    let mut notes = Vec::new();
    if !cut.targets.is_empty() {
        let kept: u64 = cut.targets.iter().map(|t| u64::from(t.glyphs_kept)).sum();
        let total: u64 = cut.targets.iter().map(|t| u64::from(t.glyphs_total)).sum();
        notes.push(format!(
            "{} font program(s) cut to the glyphs the document draws ({kept} of {total} \
glyphs kept), saving {} B; each gets a new subset tag, and text typed later in these fonts \
needs the missing glyphs embedded again",
            cut.targets.len(),
            cut.saving()
        ));
    }
    if !cut.refused.is_empty() {
        notes.push(format!(
            "{} font(s) not re-subset; each is listed with why",
            cut.refused.len()
        ));
    }
    notes
}

#[cfg(test)]
#[path = "optimize_resubset_tests.rs"]
mod tests;
