//! Convert a document to a PDF/X level: the shipped prepress verbs run in
//! order, the claim is written, and the result is validated before the
//! claim is allowed to stay.

use super::{CommandKind, CropBoxEdit, EditError, EditSession, PageBox, PageBoxEdit};
use crate::page_tree::{BoxResolution, Rect};
use crate::preflight::{
    Check, ColourFamily, PdfxLevel, PdfxVerdict, Profile, ProfileRule, Severity, validate_pdfx,
};
use crate::prepress::{
    ColourConversion, ColourConverter, ColourDestination, ConvertScope, PdfxConversion,
    PdfxConversionReport,
};
use crate::transparency::TransparencyFlattener;

impl EditSession {
    /// Convert the document to PDF/X `level` for `conversion`'s printing
    /// condition, and write the claim only when the result passes
    /// [`validate_pdfx`]. One undo entry.
    ///
    /// The steps, each reported in [`PdfxConversionReport::steps`]:
    ///
    /// 1. X-1a and X-3 forbid transparency (ISO 15930-4 §6.13, -6 §6.13):
    ///    every page's content is flattened through `flattener`
    ///    ([`Self::flatten_transparency`]). Annotations using transparency
    ///    are left and disclosed. X-4 keeps transparency.
    /// 2. [`PdfxConversion::spot_to_process`], when set
    ///    ([`Self::spot_to_process`]).
    /// 3. Colours move to the intent's profile through `converter`
    ///    ([`Self::convert_colours`]; pdfcer-core does no colour
    ///    arithmetic, decision 064). X-1a converts `DeviceRGB`, ICC RGB,
    ///    `CalRGB` and `Lab`, and grey too when `CalGray` or ICC grey is
    ///    drawn (ISO 15930-4 §6.2.4); X-3 and X-4 convert `DeviceRGB` only,
    ///    since their colour-managed spaces are allowed. The profile becomes
    ///    the one `/GTS_PDFX` output intent (ISO 32000-2 §14.11.5, Table
    ///    365), with [`crate::prepress::PdfxOutputIntent`]'s condition
    ///    entries; any further `/GTS_PDFX` intent is removed.
    /// 4. Pages with neither a `/TrimBox` nor an `/ArtBox` get a `/TrimBox`
    ///    equal to the crop box, the value Table 30 already defaults it to;
    ///    disclosed as inferred. A page whose written `/BleedBox` does not
    ///    contain its crop box is left and disclosed. `/BleedBox` is never
    ///    written: it defaults to the crop box (§14.11.2).
    /// 5. The claim: Info `GTS_PDFXVersion` (and `GTS_PDFXConformance`
    ///    `PDF/X-1a:2001` for that level), `/Trapped`, `/Title`,
    ///    `/CreationDate` and `/ModDate` as asked; the XMP packet's
    ///    `pdfxid:GTS_PDFXVersion` with `pdfx:` and `pdf:Trapped` mirrors,
    ///    a packet created for X-4, which requires one (spec RAG
    ///    `pdfx__ref__file_identification.md`).
    ///
    /// Then the document is validated. On [`PdfxVerdict::Fails`] (or
    /// anything but `Conforms` with [`PdfxConversion::require_conforms`])
    /// the claim is taken back and, unless [`PdfxConversion::keep_partial`],
    /// every step too: the document is as it was, and
    /// [`PdfxConversionReport::validation`] lists what remains. A claim is
    /// never left on a file that fails. On `Unconfirmed` the claim stays
    /// and the report says which rows pdfcer could not check.
    ///
    /// # Errors
    ///
    /// Any error of the verbs it runs, after which nothing is changed:
    /// [`EditError::DocumentEncrypted`] and the certification gate,
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    /// [`EditError::FlattenPpiOutOfRange`],
    /// [`EditError::TransparencyFlattenFailed`],
    /// [`EditError::ColourConversionRefused`] (an unusable profile).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn demo(
    /// #     session: &mut pdfcer_core::edit::EditSession,
    /// #     flattener: &dyn pdfcer_core::transparency::TransparencyFlattener,
    /// #     converter: &dyn pdfcer_core::prepress::ColourConverter,
    /// #     profile: Vec<u8>,
    /// # ) -> Result<(), pdfcer_core::edit::EditError> {
    /// use pdfcer_core::preflight::PdfxLevel;
    /// use pdfcer_core::prepress::{PdfxConversion, PdfxOutputIntent};
    ///
    /// let intent = PdfxOutputIntent::new(profile).with_condition("FOGRA39");
    /// let conversion = PdfxConversion::new(intent).with_title("Brochure");
    /// let report = session.convert_to_pdfx(PdfxLevel::X4, &conversion, flattener, converter)?;
    /// if !report.claimed {
    ///     for f in &report.validation.preflight.findings {
    ///         println!("{}: {}", f.rule_id, f.message);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn convert_to_pdfx(
        &mut self,
        level: PdfxLevel,
        conversion: &PdfxConversion,
        flattener: &dyn TransparencyFlattener,
        converter: &dyn ColourConverter,
    ) -> Result<PdfxConversionReport, EditError> {
        debug_assert!(
            !self.in_undo_group(),
            "convert_to_pdfx reverts its own entries"
        );
        self.check_ink_edit()?;
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        let mut report = PdfxConversionReport::new(level, validate_pdfx(&self.view(), level));
        let start = self.undo_top_serial();
        let kind = CommandKind::ConvertToPdfx;
        let ran = self.grouped(kind, |s| {
            s.pdfx_steps(level, conversion, flattener, converter, &mut report)
        });
        let stepped = self.undo_top_serial() != start;
        if let Err(e) = ran {
            if stepped {
                self.revert_last();
            }
            return Err(e);
        }
        let before_claim = self.undo_top_serial();
        let claim = self.grouped(kind, |s| s.write_pdfx_claim(level, conversion, &mut report));
        let claimed = self.undo_top_serial() != before_claim;
        if let Err(e) = claim {
            for _ in 0..usize::from(claimed) + usize::from(stepped) {
                self.revert_last();
            }
            return Err(e);
        }
        report.validation = validate_pdfx(&self.view(), level);
        report.after = report.validation.verdict();
        self.settle_pdfx_claim(conversion, stepped, claimed, &mut report);
        Ok(report)
    }

    /// Keep the claim when the validation allows it; otherwise take it,
    /// and the steps unless asked to keep them, back.
    fn settle_pdfx_claim(
        &mut self,
        conversion: &PdfxConversion,
        stepped: bool,
        claimed: bool,
        report: &mut PdfxConversionReport,
    ) {
        let accept = match report.after {
            PdfxVerdict::Conforms => true,
            PdfxVerdict::Unconfirmed => !conversion.require_conforms,
            PdfxVerdict::Fails => false,
        };
        let claim = report.level.claim();
        if accept {
            let entries = usize::from(stepped) + usize::from(claimed);
            self.coalesce_top(entries, CommandKind::ConvertToPdfx);
            report.claimed = claimed;
            if report.after == PdfxVerdict::Unconfirmed {
                report.disclosures.push(format!(
                    "The {claim} claim was written on an unconfirmed validation: nothing \
                     failed, but some requirements were not checked or need a person."
                ));
            }
            return;
        }
        if claimed {
            self.revert_last();
        }
        let failed = report
            .validation
            .preflight
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count();
        let reason = if report.after == PdfxVerdict::Fails {
            format!("{failed} requirement(s) still fail")
        } else {
            "the validation was not confirmed and a confirmed result was required".to_owned()
        };
        report
            .disclosures
            .push(format!("The {claim} claim was NOT written: {reason}."));
        if stepped && conversion.keep_partial {
            report.kept_partial = true;
            report.disclosures.push(
                "The conversion steps were kept without the claim (one undo entry).".to_owned(),
            );
        } else if stepped {
            self.revert_last();
            report.disclosures.push(
                "Every conversion step was taken back; the document is unchanged.".to_owned(),
            );
        }
    }

    /// Steps 1 to 4 of [`Self::convert_to_pdfx`], inside its undo group.
    fn pdfx_steps(
        &mut self,
        level: PdfxLevel,
        conversion: &PdfxConversion,
        flattener: &dyn TransparencyFlattener,
        converter: &dyn ColourConverter,
        report: &mut PdfxConversionReport,
    ) -> Result<(), EditError> {
        if level != PdfxLevel::X4 {
            let pages: Vec<usize> = (0..self.pages()?.len()).collect();
            let flat = self.flatten_transparency(&pages, &conversion.flatten, flattener)?;
            let flattened = flat.pages.iter().filter(|p| p.mode.is_some()).count();
            report.steps.push(format!(
                "Flattened transparency on {flattened} of {} page(s).",
                pages.len()
            ));
            for p in &flat.pages {
                for annot in &p.annotations_left {
                    report.disclosures.push(format!(
                        "Page {}: annotation {annot} R uses transparency and was not flattened.",
                        p.page + 1
                    ));
                }
            }
            report.disclosures.extend(flat.disclosures.iter().cloned());
            report.flatten = Some(flat);
        }
        if let Some(inks) = &conversion.spot_to_process {
            let spot = self.spot_to_process(inks)?;
            report.steps.push(format!(
                "Converted spot inks to process: {} colour space(s) changed.",
                spot.spaces
            ));
            for r in &spot.refused {
                report
                    .disclosures
                    .push(format!("Spot ink \"{}\" left: {}", r.ink, r.reason));
            }
            report.disclosures.extend(spot.disclosures.iter().cloned());
            report.spot_to_process = Some(spot);
        }
        self.pdfx_colours(level, conversion, converter, report)?;
        self.settle_pdfx_intent(&conversion.intent, report)?;
        if conversion.infer_trim_boxes {
            self.infer_trim_boxes(report)?;
        }
        Ok(())
    }

    /// Step 3: convert what `level` forbids to the intent's profile and
    /// embed it as the `/GTS_PDFX` output intent.
    fn pdfx_colours(
        &mut self,
        level: PdfxLevel,
        conversion: &PdfxConversion,
        converter: &dyn ColourConverter,
        report: &mut PdfxConversionReport,
    ) -> Result<(), EditError> {
        let destination = ColourDestination::Profile(conversion.intent.profile.clone());
        let mut colours = ColourConversion::new(destination);
        colours.intent = conversion.rendering_intent;
        colours.embed_output_intent = Some("GTS_PDFX".to_owned());
        if matches!(level, PdfxLevel::X1a2001 | PdfxLevel::X1a2003) {
            colours.grey = self.draws_calibrated_grey();
            if colours.grey {
                report.disclosures.push(
                    "CalGray or ICC grey is drawn, which X-1a forbids, so every grey \
                     (DeviceGray too) was converted to the output profile."
                        .to_owned(),
                );
            }
        } else {
            colours.icc_rgb = false;
            colours.cal_rgb = false;
            colours.lab = false;
        }
        let done = self.convert_colours(&ConvertScope::All, &colours, converter)?;
        let families: Vec<&str> = done.by_source.iter().map(|c| c.source.name()).collect();
        report.steps.push(if families.is_empty() {
            "No colour needed converting.".to_owned()
        } else {
            format!(
                "Converted {} to the output profile ({} object(s) changed).",
                families.join(", "),
                done.changed.len()
            )
        });
        for u in &done.unconverted {
            report.disclosures.push(match u.object {
                Some(id) => format!("Not converted, {id} R: {}", u.reason),
                None => format!("Not converted: {}", u.reason),
            });
        }
        report.disclosures.extend(done.disclosures.iter().cloned());
        report.colours = Some(done);
        Ok(())
    }

    /// Whether anything drawn is `CalGray` or ICC grey.
    fn draws_calibrated_grey(&self) -> bool {
        let profile = Profile {
            name: "calibrated grey".to_owned(),
            description: String::new(),
            rules: vec![ProfileRule::new(
                "grey",
                Check::ColourSpaceUsed {
                    families: vec![ColourFamily::CalGray, ColourFamily::IccGray],
                },
                Severity::Info,
            )],
        };
        !crate::preflight::run(&self.view(), &profile)
            .findings
            .is_empty()
    }

    /// Step 4: a `/TrimBox` equal to the crop box where neither a trim nor
    /// an art box is written.
    fn infer_trim_boxes(&mut self, report: &mut PdfxConversionReport) -> Result<(), EditError> {
        let mut edits = Vec::new();
        for (i, page) in self.pages()?.iter().enumerate() {
            let defaulted = |r: BoxResolution| r == BoxResolution::Defaulted;
            if !defaulted(page.trim_box_resolution) || !defaulted(page.art_box_resolution) {
                continue;
            }
            let crop = page.crop_box;
            if !defaulted(page.bleed_box_resolution) && !contains(page.bleed_box, crop) {
                report.disclosures.push(format!(
                    "Page {}: no trim box was written; the crop box lies outside the \
                     bleed box, so the finished size cannot be determined.",
                    i + 1
                ));
                continue;
            }
            edits.push(PageBoxEdit::new(i, PageBox::Trim, CropBoxEdit::Set(crop)));
            report.trim_boxes.push(i);
        }
        if edits.is_empty() {
            return Ok(());
        }
        self.set_page_boxes(&edits)?;
        let pages: Vec<String> = report
            .trim_boxes
            .iter()
            .map(|i| (i + 1).to_string())
            .collect();
        report.steps.push(format!(
            "Wrote a TrimBox on {} page(s).",
            report.trim_boxes.len()
        ));
        report.disclosures.push(format!(
            "Inferred: the TrimBox of page(s) {} is the crop box, the finished size the \
             file implied (ISO 32000-2 Table 30); check it against the job.",
            pages.join(", ")
        ));
        Ok(())
    }
}

/// `outer` contains `inner`.
fn contains(outer: Rect, inner: Rect) -> bool {
    outer.llx <= inner.llx
        && outer.lly <= inner.lly
        && outer.urx >= inner.urx
        && outer.ury >= inner.ury
}
