//! Widening hairlines: every stroke below a minimum printed width is raised,
//! in its own user space, to exactly the minimum (ISO 32000-2 §8.4.3.2).

use super::{CommandKind, EditError, EditSession, FormExtras, FormSurgeryOutcome, PermissionBit};
use crate::preflight::FindingTarget;
use crate::preflight::{HAIRLINES_NOT_COVERED, Hairline, hairlines_in};
use crate::vector::{StrokeStyle, StrokeUnits, VectorEditError, VectorObject};

/// What [`EditSession::fix_hairlines`] changed.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct HairlineFixReport {
    /// Strokes widened: page objects plus form leaves, a form's stroke counted
    /// once per invocation it was widened through.
    pub changed: usize,
    /// How many of [`Self::changed`] had line width 0.
    pub zero_width: usize,
    /// `(page index, strokes widened)` for each page with a change, 0-based.
    pub per_page: Vec<(usize, usize)>,
    /// Each form XObject rewritten and how far that one edit reaches.
    pub forms: Vec<FormSurgeryOutcome>,
    /// `(page index, reason)` for each page whose content could not be read
    /// or rewritten; nothing on it changed.
    pub skipped: Vec<(usize, String)>,
    /// What a shell must show: always [`HAIRLINES_NOT_COVERED`], and, when
    /// anything changed, that the widened strokes' lineweights moved.
    pub disclosures: Vec<String>,
}

impl EditSession {
    /// Widen every stroke on `pages` (0-based; duplicates ignored) that prints
    /// thinner than `min_width_pt` points along its thinnest axis, or has line
    /// width 0, so it prints exactly `min_width_pt` wide. Each stroke is set
    /// in its own user space (`q <w> w … Q` around its bytes, the form's own
    /// stream for a form leaf), so nothing else on the page moves.
    ///
    /// A shared form is edited in place, so the widening reaches every place
    /// it is drawn; [`HairlineFixReport::forms`] says how far. Any pdf
    /// dimensions among the strokes now print at the new lineweight, which the
    /// report discloses. Strokes in Type 3 glyphs, tiling patterns,
    /// annotation appearances and stroked text are not changed
    /// ([`HAIRLINES_NOT_COVERED`]).
    ///
    /// The whole run is one undo step.
    ///
    /// # Errors
    ///
    /// [`VectorEditError::InvalidStrokeStyle`] for a minimum that is not
    /// finite and positive; [`EditError::PageOutOfRange`]; the encryption and
    /// certification refusals every content edit has. A page whose content
    /// cannot be read is reported in [`HairlineFixReport::skipped`] instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::edit::EditSession;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let doc = Document::from_bytes(
    ///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
    /// )?;
    /// let mut session = EditSession::new(doc);
    /// let report = session.fix_hairlines(&[0], 0.25)?;
    /// assert!(!report.disclosures.is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn fix_hairlines(
        &mut self,
        pages: &[usize],
        min_width_pt: f64,
    ) -> Result<HairlineFixReport, EditError> {
        if !(min_width_pt.is_finite() && min_width_pt > 0.0) {
            return Err(VectorEditError::InvalidStrokeStyle {
                reason: "the minimum width must be a positive number of points",
            }
            .into());
        }
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let count = self.pages()?.len();
        let mut wanted: Vec<usize> = pages.to_vec();
        wanted.sort_unstable();
        wanted.dedup();
        if let Some(&index) = wanted.iter().find(|&&p| p >= count) {
            return Err(EditError::PageOutOfRange { index, count });
        }
        let mut report = HairlineFixReport::default();
        self.grouped(CommandKind::FixHairlines, |s| {
            for &page in &wanted {
                match s.fix_page_hairlines(page, min_width_pt, &mut report) {
                    Ok(0) => {}
                    Ok(n) => report.per_page.push((page, n)),
                    Err(EditError::VectorEditNoContents { .. }) => {}
                    Err(e) => report.skipped.push((page, e.to_string())),
                }
            }
        });
        report.disclosures.push(HAIRLINES_NOT_COVERED.to_owned());
        if report.changed > 0 {
            report.disclosures.push(format!(
                "{} stroke(s) widened to {min_width_pt} pt ({} of width 0); any pdf \
                 dimensions among them now print at the new lineweight",
                report.changed, report.zero_width
            ));
        }
        Ok(report)
    }

    /// Widen one page's hairlines: its own objects in one command, then each
    /// form invocation in one command each. Returns how many strokes changed.
    fn fix_page_hairlines(
        &mut self,
        page_index: usize,
        min: f64,
        report: &mut HairlineFixReport,
    ) -> Result<usize, EditError> {
        let objs = self.page_objects(page_index)?;
        let top: Vec<Hairline> = hairlines_in(&objs, min)
            .into_iter()
            .filter(|h| matches!(h.target, FindingTarget::PageObject(_)))
            .collect();
        let mut changed = 0;
        if !top.is_empty() {
            self.widen_page_objects(page_index, &top, min, report)?;
            changed += top.len();
        }
        // Every pass widens one invocation, so it cannot be thin again; the
        // bound guards a planner that leaves a stroke unchanged.
        let bound = invocation_count(&*self.page_objects(page_index)?) + 1;
        for _ in 0..bound {
            let objs = self.page_objects(page_index)?;
            let group = first_thin_invocation(&objs, min);
            if group.is_empty() {
                break;
            }
            let leaves: Vec<usize> = group.iter().map(|(i, _)| *i).collect();
            let scales: Vec<f64> = group.iter().map(|(_, h)| h.axis_scale).collect();
            let outcome = self.wrap_leaves(
                CommandKind::FixHairlines,
                page_index,
                &leaves,
                FormExtras::default(),
                |stream, objs| {
                    super::stroke_style::wrap_styled(stream, objs, &style(min), None, &scales)
                },
            )?;
            report.zero_width += group.iter().filter(|(_, h)| h.zero_width).count();
            report.changed += group.len();
            changed += group.len();
            report.forms.push(outcome);
        }
        Ok(changed)
    }

    /// Wrap the page's own thin strokes, as one command.
    fn widen_page_objects(
        &mut self,
        page_index: usize,
        thin: &[Hairline],
        min: f64,
        report: &mut HairlineFixReport,
    ) -> Result<(), EditError> {
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
        let (stream, model) = self.page_content_and_objects(page)?;
        let objs: Vec<&VectorObject> = thin
            .iter()
            .filter_map(|h| match h.target {
                FindingTarget::PageObject(i) => model.objects.get(i),
                _ => None,
            })
            .collect();
        let scales: Vec<f64> = thin.iter().map(|h| h.axis_scale).collect();
        let planned = super::stroke_style::wrap_styled(&stream, &objs, &style(min), None, &scales)?;
        let mut disclosures = Vec::new();
        let (command, _) = self.text_edit_command(
            CommandKind::FixHairlines,
            content_id,
            page,
            planned.content,
            Vec::new(),
            &mut disclosures,
        )?;
        self.commit(command);
        report.changed += thin.len();
        report.zero_width += thin.iter().filter(|h| h.zero_width).count();
        report.disclosures.extend(disclosures);
        Ok(())
    }
}

/// A width of `min` points along the axis each stroke's scale names.
fn style(min: f64) -> StrokeStyle {
    StrokeStyle {
        width: Some(min),
        units: StrokeUnits::Page,
        ..StrokeStyle::default()
    }
}

/// How many distinct `(form, placement)` invocations the page's leaves hold.
fn invocation_count(objs: &crate::vector::PageObjects) -> usize {
    let mut keys: Vec<_> = objs
        .leaves
        .iter()
        .filter_map(|l| l.containment.last().map(|f| (*f, l.placement)))
        .collect();
    keys.dedup();
    keys.len()
}

/// The thin leaves of the first invocation that has any, as
/// `(leaf index, hairline)`.
fn first_thin_invocation(objs: &crate::vector::PageObjects, min: f64) -> Vec<(usize, Hairline)> {
    let mut key = None;
    let mut out = Vec::new();
    for h in hairlines_in(objs, min) {
        let FindingTarget::FormLeaf(i) = h.target else {
            continue;
        };
        let Some(leaf) = objs.leaves.get(i) else {
            continue;
        };
        let Some(&form) = leaf.containment.last() else {
            continue;
        };
        let k = (form, leaf.placement);
        if *key.get_or_insert(k) == k {
            out.push((i, h));
        }
    }
    out
}

#[cfg(test)]
#[path = "hairlines_tests.rs"]
mod tests;
