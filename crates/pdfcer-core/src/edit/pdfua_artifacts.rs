//! Mark untagged page content as artifacts (ISO 14289-1 §7.1 ¶2, ISO
//! 14289-2 §8.2.2; Matterhorn 01-005). The planning lives in
//! `crate::preflight::pdfua::artifact`; this is the verb that writes it.

use super::{CommandKind, EditError, EditSession, PermissionBit};
use crate::object::{ObjId, Object};
use crate::preflight::{claimed_mcids, plan_page};

/// What [`EditSession::mark_untagged_as_artifacts`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ArtifactMarkReport {
    /// `(page index, content items wrapped)` for each page rewritten, 0-based.
    pub marked: Vec<(usize, usize)>,
    /// `/Artifact` sequences inserted, over all pages.
    pub sequences: usize,
    /// Content items they cover, over all pages.
    pub items: usize,
    /// `(page index, reason)` for each requested page left alone.
    pub skipped: Vec<(usize, String)>,
    /// What pdfcer inferred or could not reach, in sentences.
    pub disclosures: Vec<String>,
}

impl EditSession {
    /// Wrap content that is neither tagged nor an artifact in
    /// `/Artifact BMC … EMC` (ISO 32000-1 §14.8.2.2.2) on the tagged pages
    /// in `pages` (0-based; empty means every page). One undo step; only
    /// the content streams that change are rewritten.
    ///
    /// This is an **inference**: it treats every untagged item as
    /// decorative, which hides it from assistive technology. The report's
    /// `disclosures` says so, and names what was left: sequences whose
    /// `/MCID` the structure tree does not claim, and forms mixing tagged
    /// and untagged content. Annotation appearances are not page content
    /// and are not touched.
    ///
    /// A page is *tagged* when it has `/StructParents` or the structure
    /// tree claims one of its MCIDs; a page that is not is skipped, since
    /// marking all of it as artifacts would hide a whole page. Wraps never
    /// split a text object, a `q`/`Q` pair or a compatibility section, and
    /// never enclose tagged content (§14.6).
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentNotTagged`] with no `/StructTreeRoot`;
    /// [`EditError::PageOutOfRange`]; the encryption and certification
    /// refusals; the content-write errors.
    pub fn mark_untagged_as_artifacts(
        &mut self,
        pages: &[usize],
    ) -> Result<ArtifactMarkReport, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let claimed = claimed_mcids(&self.view()).ok_or(EditError::DocumentNotTagged)?;
        let count = self.pages()?.len();
        let wanted: Vec<usize> = if pages.is_empty() {
            (0..count).collect()
        } else {
            pages.to_vec()
        };
        if let Some(&index) = wanted.iter().find(|&&i| i >= count) {
            return Err(EditError::PageOutOfRange { index, count });
        }
        let mut report = ArtifactMarkReport::default();
        self.grouped(CommandKind::MarkUntaggedAsArtifacts, |s| {
            wanted
                .iter()
                .try_for_each(|&i| s.mark_page_artifacts(i, &claimed, &mut report))
        })?;
        if report.sequences > 0 {
            report.disclosures.insert(
                0,
                format!(
                    "Treated {} untagged content item(s) as decorative and marked them as \
                     artifacts; assistive technology will skip them. Tag any that carry meaning.",
                    report.items
                ),
            );
        }
        Ok(report)
    }

    fn mark_page_artifacts(
        &mut self,
        index: usize,
        claimed: &std::collections::HashSet<(ObjId, i64)>,
        report: &mut ArtifactMarkReport,
    ) -> Result<(), EditError> {
        let pages = self.pages()?;
        let Some(page) = pages.get(index) else {
            return Err(EditError::PageOutOfRange {
                index,
                count: pages.len(),
            });
        };
        let struct_parents = matches!(
            self.value(page.id),
            Some(Object::Dict(d)) if d.contains_key(b"StructParents")
        );
        if !struct_parents && !claimed.iter().any(|(p, _)| *p == page.id) {
            let why = "the page has no tagged content, so marking it all as artifacts would \
                       hide it from assistive technology";
            report.skipped.push((index, why.to_owned()));
            return Ok(());
        }
        let plan = match plan_page(&self.view(), page, claimed) {
            Ok(plan) => plan,
            Err(why) => {
                report.skipped.push((index, why));
                return Ok(());
            }
        };
        let n = index + 1;
        if plan.orphan_marks > 0 {
            report.disclosures.push(format!(
                "Page {n}: {} marked-content sequence(s) carry an /MCID the structure tree does \
                 not claim; they were left as they are.",
                plan.orphan_marks
            ));
        }
        for form in &plan.mixed_forms {
            report.disclosures.push(format!(
                "Page {n}: the form XObject {form} mixes tagged and untagged content; its \
                 untagged content was left, since a wrap around the form would enclose its tags."
            ));
        }
        if !plan.unread_forms.is_empty() {
            report.disclosures.push(format!(
                "Page {n}: left {} form XObject(s) that could not be read in full; untagged content may remain inside them.",
                plan.unread_forms.len()
            ));
        }
        if plan.sequences == 0 {
            return Ok(());
        }
        let Some(&content_id) = page.contents.first() else {
            return Ok(());
        };
        let mut disclosures = Vec::new();
        let (command, _) = self.text_edit_command(
            CommandKind::MarkUntaggedAsArtifacts,
            content_id,
            page,
            plan.content,
            Vec::new(),
            &mut disclosures,
        )?;
        self.commit(command);
        report.marked.push((index, plan.items));
        report.sequences += plan.sequences;
        report.items += plan.items;
        report.disclosures.extend(disclosures);
        Ok(())
    }
}
