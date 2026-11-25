//! One fixup step: run the verb a [`Fixup`] wraps and turn its report into
//! a [`FixupStep`].

use super::apply_fixup::Reach;
use super::{CommandKind, EditSession, FixPdfuaOptions, PdfuaFixup, PdfuaFixupOutcome};
use crate::optimize::{ImagePatch, OptimizeSettings, ResampleMethod, StreamAction};
use crate::preflight::{Fixup, FixupStep, FixupTools, PdfuaPart, PdfxLevel};
use crate::prepress::{
    ColourConversion, ColourDestination, ConvertScope, InkEditReport, InkSelection, PdfxConversion,
};
use crate::transparency::FlattenOptions;

const WHOLE_DOCUMENT: &str = "This fixup acts on the whole document, not only the scope.";

impl EditSession {
    /// Run `fixup` over `reach`.
    pub(super) fn fixup_step(
        &mut self,
        fixup: &Fixup,
        reach: &Reach,
        tools: &FixupTools<'_>,
        rule: Option<&str>,
    ) -> FixupStep {
        let step = FixupStep::new(fixup.name(), rule);
        let pages = match &reach.pages {
            Ok(p) => p.clone(),
            Err(why) => return step.unavailable(format!("Not run: {why}.")),
        };
        let mut step = match fixup {
            Fixup::FlattenTransparency(o) => self.step_flatten(step, &pages, o, tools),
            Fixup::FixHairlines { min_width_pt } => {
                self.step_hairlines(step, &pages, *min_width_pt)
            }
            Fixup::AliasInk { from, to } => {
                let r = self.alias_ink(from, to);
                ink_step(step, r, &format!("Renamed ink {from} to {to}"))
            }
            Fixup::SpotToProcess { keep } => self.step_spot(step, keep),
            Fixup::ConvertColours(c) => self.step_colours(step, &pages, reach.narrowed, c, tools),
            Fixup::ConvertToPdfx { level, conversion } => {
                self.step_pdfx(step, *level, conversion, tools)
            }
            Fixup::FixPdfua {
                part,
                options,
                step: one,
            } => self.step_pdfua(step, *part, options, *one),
            Fixup::DownsampleImages {
                above_ppi,
                to_ppi,
                method,
            } => self.step_downsample(step, reach, (*above_ppi, *to_ppi), *method),
        };
        let document_wide = matches!(
            fixup,
            Fixup::AliasInk { .. }
                | Fixup::SpotToProcess { .. }
                | Fixup::ConvertToPdfx { .. }
                | Fixup::FixPdfua { .. }
        );
        if document_wide && reach.narrowed {
            step.disclosures.push(WHOLE_DOCUMENT.to_owned());
        }
        step
    }

    fn step_flatten(
        &mut self,
        step: FixupStep,
        pages: &[usize],
        options: &FlattenOptions,
        tools: &FixupTools<'_>,
    ) -> FixupStep {
        let Some(flattener) = tools.flattener else {
            return step.unavailable("Not run: no transparency flattener was supplied.");
        };
        let mut step = step;
        match self.flatten_transparency(pages, options, flattener) {
            Ok(r) => {
                for p in r.pages.iter().filter(|p| p.mode.is_some()) {
                    step.wrote(1, format!("Flattened page {}.", p.page + 1));
                    for a in &p.annotations_left {
                        step.not_done.push(format!(
                            "Page {}: annotation {a} still uses transparency.",
                            p.page + 1
                        ));
                    }
                    step.disclosures.extend(p.notes.iter().cloned());
                }
                step.disclosures.extend(r.disclosures);
                step
            }
            Err(e) => step.failed(e.to_string()),
        }
    }

    fn step_hairlines(&mut self, step: FixupStep, pages: &[usize], min: f64) -> FixupStep {
        let mut step = step;
        match self.fix_hairlines(pages, min) {
            Ok(r) => {
                step.wrote(
                    r.changed,
                    format!("Widened {} stroke(s) to {min} pt.", r.changed),
                );
                for (page, why) in r.skipped {
                    step.not_done.push(format!("Page {}: {why}", page + 1));
                }
                step.disclosures.extend(r.disclosures);
                step
            }
            Err(e) => step.failed(e.to_string()),
        }
    }

    fn step_spot(&mut self, step: FixupStep, keep: &[String]) -> FixupStep {
        let converted: Vec<String> = self
            .inks()
            .inks
            .into_iter()
            .filter(|i| i.kind == crate::prepress::InkKind::Spot && !keep.contains(&i.name))
            .map(|i| i.name)
            .collect();
        if converted.is_empty() {
            return step;
        }
        let what = format!("Converted spot ink(s) {} to process", converted.join(", "));
        let r = self.spot_to_process(&InkSelection::Only(converted));
        ink_step(step, r, &what)
    }

    fn step_colours(
        &mut self,
        step: FixupStep,
        pages: &[usize],
        narrowed: bool,
        conversion: &ColourConversion,
        tools: &FixupTools<'_>,
    ) -> FixupStep {
        let Some(converter) = tools.converter else {
            return step.unavailable("Not run: no colour converter was supplied.");
        };
        let mut conversion = conversion.clone();
        if let ColourDestination::Profile(bytes) = &mut conversion.destination
            && bytes.is_empty()
        {
            let Some(icc) = tools.icc_profile else {
                return step.unavailable(
                    "Not run: it converts to a supplied ICC profile and none was supplied.",
                );
            };
            *bytes = icc.to_vec();
        }
        let scope = if narrowed {
            ConvertScope::Pages(pages.to_vec())
        } else {
            ConvertScope::All
        };
        let mut step = step;
        match self.convert_colours(&scope, &conversion, converter) {
            Ok(r) => {
                step.wrote(
                    r.changed.len(),
                    format!("Converted colours in {} object(s).", r.changed.len()),
                );
                for u in r.unconverted {
                    let at = u.object.map(|o| format!("{o}: ")).unwrap_or_default();
                    step.not_done.push(format!("{at}{}", u.reason));
                }
                step.disclosures.extend(r.disclosures);
                step
            }
            Err(e) => step.failed(e.to_string()),
        }
    }

    fn step_pdfx(
        &mut self,
        step: FixupStep,
        level: PdfxLevel,
        conversion: &PdfxConversion,
        tools: &FixupTools<'_>,
    ) -> FixupStep {
        if self.in_undo_group() {
            return step
                .unavailable("Not run: a PDF/X conversion cannot run inside another undo group.");
        }
        let (Some(flattener), Some(converter)) = (tools.flattener, tools.converter) else {
            return step.unavailable("Not run: a PDF/X conversion needs both a transparency flattener and a colour converter.");
        };
        let mut conversion = conversion.clone();
        if conversion.intent.profile.is_empty() {
            let Some(icc) = tools.icc_profile else {
                return step.unavailable("Not run: a PDF/X conversion needs the output intent's ICC profile and none was supplied.");
            };
            conversion.intent.profile = icc.to_vec();
        }
        conversion.at = conversion.at.or(tools.at);
        if conversion.title.is_none() {
            conversion.title = tools.title.map(str::to_owned);
        }
        let start = self.undo_top_serial();
        let r = match self.convert_to_pdfx(level, &conversion, flattener, converter) {
            Ok(r) => r,
            Err(e) => return step.failed(e.to_string()),
        };
        let mut step = step;
        let wrote = self.undo_top_serial() != start;
        step.disclosures.extend(r.disclosures);
        if r.claimed {
            step.wrote(
                1,
                format!(
                    "Converted to PDF/{}; it now {}.",
                    level.as_str().to_ascii_uppercase(),
                    r.after.as_str()
                ),
            );
            step.changed.extend(r.steps);
            return step;
        }
        let left = r.validation.preflight.findings.len();
        if wrote && r.kept_partial {
            step.wrote(1, "Kept the partial conversion.");
            step.changed.extend(r.steps);
            step.not_done.push(format!(
                "No PDF/X claim was written: the file still {} ({left} finding(s)).",
                r.after.as_str()
            ));
            return step;
        }
        step.failed(format!(
            "Not converted: the result {} PDF/{} ({left} finding(s)), so every step was taken back.",
            r.after.as_str(),
            level.as_str().to_ascii_uppercase()
        ))
    }

    fn step_pdfua(
        &mut self,
        step: FixupStep,
        part: PdfuaPart,
        options: &FixPdfuaOptions,
        one: Option<PdfuaFixup>,
    ) -> FixupStep {
        let mut disclosures = Vec::new();
        let results = match one {
            None => match self.fix_pdfua(part, options) {
                Ok(r) => {
                    disclosures = r.disclosures;
                    r.results
                }
                Err(e) => return step.failed(e.to_string()),
            },
            Some(s) => {
                if let Err(e) = self.check_metadata_edit() {
                    return step.failed(e.to_string());
                }
                vec![self.grouped(CommandKind::FixPdfua, |me| {
                    me.run_pdfua_step(s, part, options, &mut disclosures)
                })]
            }
        };
        let mut step = step;
        for r in results {
            let line = format!("{}: {}", r.fixup.as_str(), r.detail);
            match r.outcome {
                PdfuaFixupOutcome::Applied => step.wrote(1, line),
                PdfuaFixupOutcome::AlreadyDone => {}
                _ => step.not_done.push(line),
            }
        }
        if step.count == 0 && !step.not_done.is_empty() {
            step.outcome = crate::preflight::FixupOutcome::Failed;
        }
        step.disclosures.extend(disclosures);
        step
    }

    fn step_downsample(
        &mut self,
        step: FixupStep,
        reach: &Reach,
        (above, to): (u16, u16),
        method: ResampleMethod,
    ) -> FixupStep {
        let mut patch = ImagePatch::new();
        patch.ppi = Some(to);
        patch.above = Some(above.max(to));
        patch.method = Some(method);
        let mut settings = OptimizeSettings::new();
        let mut step = step;
        let targets = match &reach.images {
            None => {
                settings.images = patch;
                None
            }
            Some((ids, not_done)) => {
                step.not_done.extend(not_done.iter().cloned());
                if ids.is_empty() {
                    return step.unavailable("Not run: the scope points at no image object.");
                }
                for id in ids {
                    settings.objects.insert(*id, StreamAction::Image(patch));
                }
                Some(ids)
            }
        };
        let plan = match self.plan_optimize(&settings) {
            Ok(p) => p,
            Err(e) => return step.failed(e.to_string()),
        };
        let resized: Vec<_> = plan
            .changes
            .iter()
            .filter_map(|c| c.image.as_ref().map(|i| (c.id, i)))
            .filter(|(_, i)| i.size_before != i.size_after)
            .collect();
        for s in &plan.skipped {
            step.not_done.push(format!("Image {}: {}", s.id, s.reason));
        }
        if let Some(ids) = targets {
            for id in ids {
                let planned = plan.changes.iter().any(|c| c.id == *id);
                let skipped = plan.skipped.iter().any(|s| s.id == *id);
                if !planned && !skipped {
                    step.not_done.push(format!(
                        "Image {id}: not resampled; it is drawn at or below {above} ppi where it is drawn largest."
                    ));
                }
            }
        }
        if plan.changes.is_empty() {
            return step;
        }
        match self.apply_optimize(&plan) {
            Ok(_) => {
                for (id, i) in resized {
                    step.wrote(
                        1,
                        format!(
                            "Resampled image {id} from {} x {} to {} x {} pixels.",
                            i.size_before.0, i.size_before.1, i.size_after.0, i.size_after.1
                        ),
                    );
                }
                step.disclosures.extend(plan.notes);
                step
            }
            Err(e) => step.failed(e.to_string()),
        }
    }
}

fn ink_step(
    step: FixupStep,
    result: Result<InkEditReport, super::EditError>,
    what: &str,
) -> FixupStep {
    let r = match result {
        Ok(r) => r,
        Err(e) => return step.failed(e.to_string()),
    };
    let mut step = step;
    step.wrote(
        r.changed.len(),
        format!("{what} in {} object(s).", r.changed.len()),
    );
    for refusal in r.refused {
        let at = refusal
            .object
            .map(|o| format!(" in {o}"))
            .unwrap_or_default();
        step.not_done
            .push(format!("Ink {}{at}: {}", refusal.ink, refusal.reason));
    }
    for o in r.overprint_lost {
        step.disclosures.push(format!(
            "{o} paints a converted ink under overprint and no longer overprints."
        ));
    }
    step.disclosures.extend(r.disclosures);
    step
}
