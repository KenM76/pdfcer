//! PDF/UA fixups: the language and tab-order verbs, and [`EditSession::fix_pdfua`],
//! which runs every fixup in order and re-validates (ISO 14289-1, ISO 14289-2).

use super::{
    Command, CommandKind, EditError, EditSession, ObjectWrite, PageTabs, decode_text_string,
    encode_text_string,
};
use crate::object::{Name, Object};
use crate::preflight::{PdfuaPart, PdfuaReport, lang_well_formed};

/// One PDF/UA fixup, in the order [`EditSession::fix_pdfua`] runs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PdfuaFixup {
    /// Catalog `/Lang` ([`EditSession::set_document_language`]).
    Language,
    /// `/DisplayDocTitle` and `dc:title` ([`EditSession::set_display_doc_title`]).
    DisplayDocTitle,
    /// `/Tabs` on annotated pages ([`EditSession::set_pdfua_tab_order`]).
    TabOrder,
    /// Untagged content as artifacts ([`EditSession::mark_untagged_as_artifacts`]).
    Artifacts,
    /// The `pdfuaid` claim ([`EditSession::write_pdfua_identification`]).
    Identification,
}

impl PdfuaFixup {
    /// A stable lower-case name for reports and JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::DisplayDocTitle => "display-doc-title",
            Self::TabOrder => "tab-order",
            Self::Artifacts => "artifacts",
            Self::Identification => "identification",
        }
    }
}

/// How one fixup ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PdfuaFixupOutcome {
    /// It wrote something.
    Applied,
    /// The document already satisfied it; nothing written.
    AlreadyDone,
    /// It was refused; the detail says why. Nothing written.
    Refused,
    /// It was not attempted; the detail says why (e.g. no language given).
    Skipped,
}

impl PdfuaFixupOutcome {
    /// A stable lower-case name for reports and JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::AlreadyDone => "already-done",
            Self::Refused => "refused",
            Self::Skipped => "skipped",
        }
    }
}

/// One fixup's result in a [`FixPdfuaReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PdfuaFixupResult {
    /// Which fixup.
    pub fixup: PdfuaFixup,
    /// How it ended.
    pub outcome: PdfuaFixupOutcome,
    /// What it did, or why not, in a sentence.
    pub detail: String,
}

/// What [`EditSession::fix_pdfua`] may use that the document cannot say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FixPdfuaOptions {
    /// The document's language as a BCP 47 tag (`en-US`). Without it, and
    /// with no `/Lang` already, the language fixup is skipped: pdfcer does
    /// not guess a language.
    pub lang: Option<String>,
    /// A title to set when the document should show a different one, or has
    /// none. Without it the existing title is used; with none, the title
    /// fixup is refused.
    pub title: Option<String>,
}

impl FixPdfuaOptions {
    /// Set [`Self::lang`].
    #[must_use]
    pub fn with_lang(mut self, tag: impl Into<String>) -> Self {
        self.lang = Some(tag.into());
        self
    }

    /// Set [`Self::title`].
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// What [`EditSession::fix_pdfua`] did and the verdict afterwards.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FixPdfuaReport {
    /// Each fixup, in the order run.
    pub results: Vec<PdfuaFixupResult>,
    /// What pdfcer inferred or could not reach, in sentences.
    pub disclosures: Vec<String>,
    /// [`EditSession::validate_pdfua`] run after the fixups.
    pub validation: PdfuaReport,
}

impl FixPdfuaReport {
    /// Whether the `pdfuaid` claim is now in the document.
    #[must_use]
    pub fn identified(&self) -> bool {
        self.results.iter().any(|r| {
            r.fixup == PdfuaFixup::Identification
                && matches!(
                    r.outcome,
                    PdfuaFixupOutcome::Applied | PdfuaFixupOutcome::AlreadyDone
                )
        })
    }
}

impl EditSession {
    /// Set the catalog's `/Lang`, the document's default natural language
    /// (ISO 32000-1 §14.9.2.1; ISO 14289-1 §7.2 ¶3, ISO 14289-2 §8.4.4).
    /// `tag` is a BCP 47 tag such as `en-US`, checked for shape only.
    /// Returns whether anything was written (one undo step).
    ///
    /// # Errors
    ///
    /// [`EditError::LanguageTagMalformed`]; the encryption and
    /// certification refusals.
    pub fn set_document_language(&mut self, tag: &str) -> Result<bool, EditError> {
        self.check_metadata_edit()?;
        let tag = tag.trim();
        if !lang_well_formed(tag) {
            return Err(EditError::LanguageTagMalformed {
                tag: tag.to_owned(),
            });
        }
        let (id, mut catalog) = self.catalog_entry()?;
        if self.document_language().as_deref() == Some(tag) {
            return Ok(false);
        }
        catalog.insert(Name::from(b"Lang"), Object::String(encode_text_string(tag)));
        self.commit(Command {
            kind: CommandKind::SetDocumentLanguage,
            objects: vec![ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(Object::Dict(catalog)),
            }],
            removals: Vec::new(),
            trailer: None,
        });
        Ok(true)
    }

    /// The catalog's `/Lang`, decoded, when it is a string.
    #[must_use]
    pub fn document_language(&self) -> Option<String> {
        let (_, catalog) = self.catalog_entry().ok()?;
        let value = match catalog.get(b"Lang")? {
            Object::Reference(id) => self.value(*id)?,
            direct => direct,
        };
        match value {
            Object::String(bytes) => Some(decode_text_string(bytes).text),
            _ => None,
        }
    }

    /// Give every page with annotations a tab order PDF/UA permits:
    /// `/Tabs /S` (ISO 14289-1 §7.18.3; ISO 14289-2 §8.9.3.3 also permits
    /// `/A` and `/W`, which are kept). Returns the 0-based pages changed
    /// (one undo step).
    ///
    /// # Errors
    ///
    /// The encryption and certification refusals; page-tree errors.
    pub fn set_pdfua_tab_order(&mut self, part: PdfuaPart) -> Result<Vec<usize>, EditError> {
        self.check_metadata_edit()?;
        let permitted: &[PageTabs] = match part {
            PdfuaPart::Ua1 => &[PageTabs::Structure],
            PdfuaPart::Ua2 => &[
                PageTabs::Structure,
                PageTabs::ArrayOrder,
                PageTabs::WidgetOrder,
            ],
        };
        let mut wrong = Vec::new();
        for (index, slot) in self.page_slots()?.iter().enumerate() {
            let Some(Object::Dict(page)) = self.value(slot.id) else {
                continue;
            };
            let tabs = PageTabs::from_entry(page.get(b"Tabs"));
            if self.page_has_annotations(page) && !permitted.contains(&tabs) {
                wrong.push(index);
            }
        }
        self.grouped(CommandKind::SetPdfuaTabOrder, |s| {
            wrong
                .iter()
                .try_for_each(|&i| s.set_page_tabs(i, PageTabs::Structure).map(|_| ()))
        })?;
        Ok(wrong)
    }

    /// Run every PDF/UA fixup in order, as one undo step, then re-validate:
    /// language, title, tab order, artifacts, identification. A fixup that
    /// is refused or skipped is recorded and the rest still run; the
    /// identification is written only when nothing else fails (see
    /// [`Self::write_pdfua_identification`]).
    ///
    /// # Errors
    ///
    /// Only the encryption and certification refusals; every fixup's own
    /// refusal lands in the report.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::edit::{EditSession, FixPdfuaOptions};
    /// use pdfcer_core::preflight::PdfuaPart;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let doc = Document::from_bytes(
    ///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
    /// )?;
    /// let mut session = EditSession::new(doc);
    /// let options = FixPdfuaOptions::default().with_lang("en-US");
    /// let report = session.fix_pdfua(PdfuaPart::Ua1, &options)?;
    /// // An untagged file is not made conformant, so no claim is written.
    /// assert!(!report.identified());
    /// # Ok(())
    /// # }
    /// ```
    pub fn fix_pdfua(
        &mut self,
        part: PdfuaPart,
        options: &FixPdfuaOptions,
    ) -> Result<FixPdfuaReport, EditError> {
        self.check_metadata_edit()?;
        let mut results = Vec::new();
        let mut disclosures = Vec::new();
        self.grouped(CommandKind::FixPdfua, |s| {
            results.push(s.fixup_language(options.lang.as_deref()));
            results.push(s.fixup_title(options.title.as_deref(), &mut disclosures));
            results.push(s.fixup_tabs(part));
            results.push(s.fixup_artifacts(&mut disclosures));
            results.push(s.fixup_identification(part));
        });
        Ok(FixPdfuaReport {
            results,
            disclosures,
            validation: self.validate_pdfua(part),
        })
    }

    fn fixup_language(&mut self, lang: Option<&str>) -> PdfuaFixupResult {
        let fixup = PdfuaFixup::Language;
        let Some(tag) = lang else {
            return match self.document_language() {
                Some(l) if lang_well_formed(&l) => result(
                    fixup,
                    PdfuaFixupOutcome::AlreadyDone,
                    format!("/Lang is {l}."),
                ),
                _ => result(
                    fixup,
                    PdfuaFixupOutcome::Skipped,
                    "No language was given and the document states none; pdfcer does not \
                     guess a language."
                        .to_owned(),
                ),
            };
        };
        match self.set_document_language(tag) {
            Ok(true) => result(
                fixup,
                PdfuaFixupOutcome::Applied,
                format!("Set /Lang to {tag}."),
            ),
            Ok(false) => result(
                fixup,
                PdfuaFixupOutcome::AlreadyDone,
                format!("/Lang is {tag}."),
            ),
            Err(e) => refused(fixup, &e),
        }
    }

    fn fixup_title(&mut self, title: Option<&str>, out: &mut Vec<String>) -> PdfuaFixupResult {
        use super::DocTitleSource as Src;
        let fixup = PdfuaFixup::DisplayDocTitle;
        let outcome = match self.set_display_doc_title(title) {
            Ok(o) => o,
            Err(e) => return refused(fixup, &e),
        };
        if outcome.source == Src::CopiedFromInfo && outcome.changed {
            out.push("Copied the /Info /Title into the XMP dc:title.".to_owned());
        }
        if outcome.packet_created {
            out.push("Created an XMP metadata stream to hold dc:title.".to_owned());
        }
        if outcome.changed {
            let detail = "Set /DisplayDocTitle true and the title it shows.";
            result(fixup, PdfuaFixupOutcome::Applied, detail.to_owned())
        } else {
            let detail = "/DisplayDocTitle is true and dc:title is present.";
            result(fixup, PdfuaFixupOutcome::AlreadyDone, detail.to_owned())
        }
    }

    fn fixup_tabs(&mut self, part: PdfuaPart) -> PdfuaFixupResult {
        let fixup = PdfuaFixup::TabOrder;
        match self.set_pdfua_tab_order(part) {
            Ok(pages) if pages.is_empty() => {
                let detail = "Every page with annotations has a permitted tab order.";
                result(fixup, PdfuaFixupOutcome::AlreadyDone, detail.to_owned())
            }
            Ok(pages) => {
                let list: Vec<String> = pages.iter().map(|p| (p + 1).to_string()).collect();
                let detail = format!("Set /Tabs /S on page(s) {}.", list.join(", "));
                result(fixup, PdfuaFixupOutcome::Applied, detail)
            }
            Err(e) => refused(fixup, &e),
        }
    }

    fn fixup_artifacts(&mut self, out: &mut Vec<String>) -> PdfuaFixupResult {
        let fixup = PdfuaFixup::Artifacts;
        let report = match self.mark_untagged_as_artifacts(&[]) {
            Ok(r) => r,
            Err(e) => return refused(fixup, &e),
        };
        out.extend(report.disclosures);
        for (page, why) in &report.skipped {
            out.push(format!("Page {}: not marked: {why}.", page + 1));
        }
        if report.sequences == 0 {
            let detail = "No untagged content was found that could be marked.";
            return result(fixup, PdfuaFixupOutcome::AlreadyDone, detail.to_owned());
        }
        let detail = format!(
            "Marked {} item(s) on {} page(s) as artifacts.",
            report.items,
            report.marked.len()
        );
        result(fixup, PdfuaFixupOutcome::Applied, detail)
    }

    fn fixup_identification(&mut self, part: PdfuaPart) -> PdfuaFixupResult {
        let fixup = PdfuaFixup::Identification;
        let n = part.number();
        match self.write_pdfua_identification(part) {
            Ok(true) => {
                let detail = format!("Wrote the PDF/UA-{n} identification.");
                result(fixup, PdfuaFixupOutcome::Applied, detail)
            }
            Ok(false) => {
                let detail = format!("The PDF/UA-{n} identification is present.");
                result(fixup, PdfuaFixupOutcome::AlreadyDone, detail)
            }
            Err(e) => refused(fixup, &e),
        }
    }
}

fn result(fixup: PdfuaFixup, outcome: PdfuaFixupOutcome, detail: String) -> PdfuaFixupResult {
    PdfuaFixupResult {
        fixup,
        outcome,
        detail,
    }
}

fn refused(fixup: PdfuaFixup, e: &EditError) -> PdfuaFixupResult {
    result(fixup, PdfuaFixupOutcome::Refused, e.to_string())
}

#[cfg(test)]
#[path = "pdfua_fix_tests.rs"]
mod tests;
