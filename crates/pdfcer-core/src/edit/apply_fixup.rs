//! Preflight fixups on a session: one fixup over a scope, or every rule of
//! a profile that found something, as one undo step.

use std::collections::{BTreeMap, BTreeSet};

use super::{CommandKind, EditSession};
use crate::object::ObjId;
use crate::preflight::{
    Finding, FindingTarget, Fixup, FixupReport, FixupScope, FixupStep, FixupTools, PreflightReport,
    Profile,
};
use crate::vector::{self, ImageSource, Matrix, VectorObject};

/// What a fixup reaches, resolved before anything is written (a later
/// write can shift the object indices findings carry).
pub(super) struct Reach {
    /// 0-based pages for a page-scoped verb; `Err` when the scope holds
    /// nothing a fixup can answer.
    pub(super) pages: Result<Vec<usize>, String>,
    /// For a narrowed downsample: the image XObjects the findings point at,
    /// and what they pointed at that cannot be resampled. `None`: every image.
    pub(super) images: Option<(BTreeSet<ObjId>, Vec<String>)>,
    /// Whether the scope is narrower than the whole document.
    pub(super) narrowed: bool,
}

impl EditSession {
    /// Run one preflight fixup over `scope` as ONE undo step
    /// ([`CommandKind::ApplyFixup`]), and say what it changed and what it
    /// could not.
    ///
    /// The scope narrows page-scoped fixups (flatten, hairlines, colour
    /// conversion) to the pages of its violations, and a downsample to the
    /// images they point at; a document-level violation widens to every
    /// page. Ink, PDF/UA and PDF/X fixups always act on the whole document,
    /// and the step says so when the scope was narrower. A scope with no
    /// violation runs nothing and says so. Needs-a-person and incomplete
    /// findings never select anything.
    ///
    /// Never fails: a verb's refusal, a missing engine or ICC profile, and
    /// anything the verb left undone are in [`FixupReport::steps`]. A failed
    /// verb writes nothing. [`Fixup::ConvertToPdfx`] is reported unavailable
    /// when this runs inside an outer undo group, because that verb takes
    /// back its own entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::edit::EditSession;
    /// use pdfcer_core::preflight::{Fixup, FixupScope, FixupTools};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let doc = Document::from_bytes(
    ///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
    /// )?;
    /// let mut session = EditSession::new(doc);
    /// let fixup = Fixup::FixHairlines { min_width_pt: 0.25 };
    /// let report = session.apply_fixup(&fixup, &FixupScope::All, &FixupTools::new());
    /// print!("{}", report.to_text());
    /// # Ok(())
    /// # }
    /// ```
    pub fn apply_fixup(
        &mut self,
        fixup: &Fixup,
        scope: &FixupScope<'_>,
        tools: &FixupTools<'_>,
    ) -> FixupReport {
        let rule = match scope {
            FixupScope::Rule { rule_id, .. } => Some(*rule_id),
            _ => None,
        };
        let reach = self.fixup_reach(fixup, scope);
        self.fixup_run(|s| vec![s.fixup_step(fixup, &reach, tools, rule)])
    }

    /// Run the fixups of every rule of `profile` that has a violation in
    /// `report`, in profile order, as ONE undo step. A rule's own
    /// [`crate::preflight::ProfileRule::fixups`] are used; with `suggest`, a
    /// rule that names none uses [`Fixup::suggested`]. A rule with
    /// violations and no fixup gets an unavailable `none` step, and a fixup
    /// two rules share runs once with both rule ids. Every reach is resolved
    /// before the first write.
    ///
    /// `report` should come from this session as it stands; findings point
    /// at objects by index. Re-run preflight afterwards for what remains.
    pub fn apply_profile_fixups(
        &mut self,
        profile: &Profile,
        report: &PreflightReport,
        tools: &FixupTools<'_>,
        suggest: bool,
    ) -> FixupReport {
        let mut planned: Vec<(Fixup, Reach, Vec<String>)> = Vec::new();
        let mut none = Vec::new();
        for rule in &profile.rules {
            let scope = FixupScope::Rule {
                rule_id: &rule.id,
                findings: &report.findings,
            };
            let hits = scope.findings().map_or(0, |f| f.len());
            if hits == 0 {
                continue;
            }
            let mut fixups = rule.fixups.clone();
            if fixups.is_empty() && suggest {
                fixups.extend(Fixup::suggested(&rule.check));
            }
            if fixups.is_empty() {
                none.push(FixupStep::new("none", Some(&rule.id)).unavailable(format!(
                    "{hits} violation(s) of rule {} were not fixed: it names no fixup{}.",
                    rule.id,
                    if suggest {
                        " and pdfcer suggests none"
                    } else {
                        ""
                    }
                )));
                continue;
            }
            for fixup in fixups {
                if let Some(p) = planned.iter_mut().find(|p| p.0 == fixup) {
                    p.2.push(rule.id.clone());
                    continue;
                }
                let reach = self.fixup_reach(&fixup, &scope);
                planned.push((fixup, reach, vec![rule.id.clone()]));
            }
        }
        let mut out = self.fixup_run(|s| {
            let mut steps = Vec::new();
            for (fixup, reach, rules) in &planned {
                let mut step = s.fixup_step(fixup, reach, tools, None);
                step.rule_ids.clone_from(rules);
                steps.push(step);
            }
            steps
        });
        out.steps.extend(none);
        out
    }

    /// Run `body`, then fold every undo entry it left into one.
    fn fixup_run(&mut self, body: impl FnOnce(&mut Self) -> Vec<FixupStep>) -> FixupReport {
        let start = self.undo_top_serial();
        let nested = self.in_undo_group();
        let steps = body(self);
        let own = self
            .undo
            .iter()
            .rev()
            .take_while(|e| e.serial > start)
            .count();
        let undo_entries = if nested {
            0
        } else {
            self.coalesce_top(own, CommandKind::ApplyFixup);
            own.min(1)
        };
        FixupReport {
            steps,
            undo_entries,
        }
    }

    /// Resolve what `fixup` reaches under `scope`, reading only.
    pub(super) fn fixup_reach(&self, fixup: &Fixup, scope: &FixupScope<'_>) -> Reach {
        let all = self.view_page_count();
        let Some(findings) = scope.findings() else {
            return Reach {
                pages: Ok((0..all).collect()),
                images: None,
                narrowed: false,
            };
        };
        if findings.is_empty() {
            return Reach {
                pages: Err("the scope holds no violation a fixup can answer".into()),
                images: None,
                narrowed: true,
            };
        }
        let pages = if findings.iter().any(|f| f.page.is_none()) {
            (0..all).collect()
        } else {
            let set: BTreeSet<usize> = findings
                .iter()
                .filter_map(|f| f.page)
                .map(|p| p - 1)
                .collect();
            set.into_iter().collect()
        };
        let images =
            matches!(fixup, Fixup::DownsampleImages { .. }).then(|| self.finding_images(&findings));
        Reach {
            pages: Ok(pages),
            images,
            narrowed: true,
        }
    }

    fn view_page_count(&self) -> usize {
        crate::page_tree::pages_in(self.view().graph()).map_or(0, |p| p.len())
    }

    /// The image XObjects `findings` point at, and a sentence for each one
    /// that points at something a downsample cannot reach.
    fn finding_images(&self, findings: &[&Finding]) -> (BTreeSet<ObjId>, Vec<String>) {
        let view = self.view();
        let pages = crate::page_tree::pages_in(view.graph()).unwrap_or_default();
        let mut decomposed = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut not_done = Vec::new();
        for f in findings {
            let index = match f.target {
                FindingTarget::Image(id) => {
                    ids.insert(id);
                    continue;
                }
                FindingTarget::PageObject(i) => (i, false),
                FindingTarget::FormLeaf(i) => (i, true),
                _ => {
                    not_done.push(format!("{}: it is not about an image.", where_(f)));
                    continue;
                }
            };
            let Some((n, page)) = f
                .page
                .and_then(|n| Some((n, pages.get(n.checked_sub(1)?)?)))
            else {
                not_done.push(format!("{}: its page could not be found.", where_(f)));
                continue;
            };
            let objs = decomposed
                .entry(n)
                .or_insert_with(|| vector::decompose_page(&view, page, Matrix::IDENTITY));
            let object = objs.as_ref().ok().and_then(|o| match index {
                (i, false) => o.objects.get(i),
                (i, true) => o.leaves.get(i).map(|l| &l.object),
            });
            match object {
                Some(VectorObject::Image(img)) if img.source == ImageSource::XObject => {
                    match img.xobject {
                        Some(id) => {
                            ids.insert(id);
                        }
                        None => not_done.push(format!(
                            "{}: the image is a direct stream with no object of its own.",
                            where_(f)
                        )),
                    }
                }
                Some(VectorObject::Image(img)) if img.source == ImageSource::Inline => not_done
                    .push(format!(
                        "{}: an inline image cannot be resampled.",
                        where_(f)
                    )),
                _ => not_done.push(format!(
                    "{}: the image it points at is no longer there.",
                    where_(f)
                )),
            }
        }
        (ids, not_done)
    }
}

/// `Page 3 (rule-id)` for a finding.
fn where_(f: &Finding) -> String {
    match f.page {
        Some(n) => format!("Page {n} ({})", f.rule_id),
        None => format!("Document ({})", f.rule_id),
    }
}

#[cfg(test)]
#[path = "apply_fixup_tests.rs"]
mod tests;
