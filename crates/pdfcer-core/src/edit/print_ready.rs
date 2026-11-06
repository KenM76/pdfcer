//! [`EditSession::to_print_ready_bytes`]: one finished PDF for a print
//! service that takes a file and may not draw annotations itself.

use std::collections::BTreeSet;

use super::{
    AnnotBurn, AnnotFlattenOutcome, AnnotFlattenRefusalReason, Command, CommandKind, EditError,
    EditSession, ObjectWrite, PageSlot, Removal,
};
use crate::annot::Annotation;
use crate::annot_author::FreeTextC;
use crate::crypto::PermissionBit;
use crate::document::Document;
use crate::object::{ObjId, Object};
use crate::writer::{self, RebuildOptions, SaveOptions, WriteError};
use crate::xfa::XfaFormKind;

/// The annotation types ISO 32000 Table 171 defines. §12.5.3's Invisible flag
/// applies only to a type outside this set.
const STANDARD_SUBTYPES: [&[u8]; 28] = [
    b"Text",
    b"Link",
    b"FreeText",
    b"Line",
    b"Square",
    b"Circle",
    b"Polygon",
    b"PolyLine",
    b"Highlight",
    b"Underline",
    b"Squiggly",
    b"StrikeOut",
    b"Caret",
    b"Stamp",
    b"Ink",
    b"Popup",
    b"FileAttachment",
    b"Sound",
    b"Movie",
    b"Screen",
    b"Widget",
    b"PrinterMark",
    b"TrapNet",
    b"Watermark",
    b"3D",
    b"Redact",
    b"Projection",
    b"RichMedia",
];

/// What [`EditSession::to_print_ready_bytes`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrintReadyCopy {
    /// A complete PDF with no annotations and no interactive form.
    pub bytes: Vec<u8>,
    /// What went into it and what did not.
    pub report: PrintReadyReport,
}

/// One annotation set to print that has nothing to paint.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrintReadyLoss {
    /// Page index, 0-based.
    pub page_index: usize,
    /// The annotation's `/Subtype`.
    pub subtype: Vec<u8>,
    /// Why nothing could be painted.
    pub reason: AnnotFlattenRefusalReason,
}

/// What [`EditSession::to_print_ready_bytes`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrintReadyReport {
    /// Annotations and form fields now painted as page content.
    pub burned: usize,
    /// Of [`Self::burned`], those a viewer hides on screen (NoView).
    pub printed_not_on_screen: usize,
    /// Of [`Self::burned`], pending redaction marks: the mark prints, and the
    /// content under it is still in the copy.
    pub redaction_marks: usize,
    /// Annotations left out because they are set not to print. Pop-up notes
    /// are not counted: they never print.
    pub not_printing: usize,
    /// Annotations set to print with nothing to paint. A link and a form
    /// field with no appearance paint nothing in any viewer and are not
    /// listed.
    pub lost: Vec<PrintReadyLoss>,
    /// Markup annotations whose look pdfcer drew because the file has none.
    pub appearances_drawn: usize,
    /// Form fields whose look pdfcer generated from their values.
    pub fields_generated: usize,
    /// The source was encrypted and this copy is not.
    pub decrypted: bool,
    /// Signatures and certifications in the source, which this copy does not
    /// carry.
    pub signatures_dropped: usize,
    /// Layers with their own print setting, which this copy leaves to the
    /// print service.
    pub layers_with_print_setting: usize,
    /// The source is a dynamic XFA form, so its pages hold only the
    /// placeholder written for viewers without an XFA engine.
    pub dynamic_xfa: bool,
    /// The above as sentences for the operator, one per fact.
    pub disclosures: Vec<String>,
}

/// Why [`EditSession::to_print_ready_bytes`] gave no copy.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PrintReadyError {
    /// The password that opened the document does not allow printing
    /// (ISO 32000 Table 22 bit 3).
    #[error("the password that opened this document does not allow printing")]
    PrintingForbidden,
    /// The password allows printing at low quality only (Table 22 bit 12).
    /// A PDF cannot hold a print service to that; rasterise the pages for it.
    #[error(
        "the password that opened this document allows low-quality printing only, \
         which a PDF handed to a print service cannot enforce"
    )]
    LowQualityPrintingOnly,
    /// A step on the private copy was refused.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// The copy could not be written, including
    /// [`WriteError::RedactionPending`] while a redaction is staged.
    #[error(transparent)]
    Write(#[from] WriteError),
}

impl EditSession {
    /// **Print-ready copy**: the document as it stands now, unsaved edits
    /// included, as one PDF in which every annotation that prints is page
    /// content and everything else interactive is gone.
    ///
    /// By ISO 32000-1 §12.5.3 Table 165, an annotation prints when Hidden is
    /// clear and Print is set, whatever NoView says, unless Invisible applies
    /// to its non-standard type. Those burn in `/Annots` order over the page
    /// content, keeping `/CA` and `/OC`. That covers every type, including a
    /// 3D poster, form fields, media, and locked or action-carrying
    /// annotations. A markup annotation or form field with no look gets the
    /// one pdfcer would draw (`free_text_c` as for
    /// [`Self::bake_missing_appearances`]); that is reported.
    ///
    /// The copy has no `/Annots`, `/AcroForm` or `/Perms`, and is rebuilt from
    /// scratch without object streams or anything unreachable, so an embedded
    /// 3D model, a removed annotation and earlier revisions are not in it.
    /// Numbering changes. Decoded page content is otherwise unchanged. `self`
    /// is only borrowed and gains nothing, undo entries included.
    ///
    /// **Encryption.** An encrypted source yields an unencrypted copy, since a
    /// print service cannot take a password. It is for the print job: hand it
    /// over, then drop it. The permissions travelled with the encryption, so
    /// that is disclosed. Permission to print is honoured as the one check.
    /// Layers with their own print setting are disclosed, not resolved.
    ///
    /// # Errors
    ///
    /// [`PrintReadyError::PrintingForbidden`],
    /// [`PrintReadyError::LowQualityPrintingOnly`];
    /// [`PrintReadyError::Edit`] from a step, such as
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`];
    /// [`PrintReadyError::Write`], including [`WriteError::RedactionPending`],
    /// and [`WriteError::HybridFullRewrite`] for an encrypted hybrid-reference
    /// file.
    pub fn to_print_ready_bytes(
        &self,
        free_text_c: FreeTextC,
    ) -> Result<PrintReadyCopy, PrintReadyError> {
        if self.redaction_pending {
            return Err(WriteError::RedactionPending.into());
        }
        check_print_permission(&self.base)?;
        let mut report = PrintReadyReport::default();
        let mut fork = self.fork_for_print();
        report.signatures_dropped = {
            let found = crate::signature::census(&fork.graph());
            found.signatures.max(found.certifications)
        };
        report.dynamic_xfa = crate::xfa::form_kind(&fork.graph()) == XfaFormKind::Dynamic;
        report.layers_with_print_setting = crate::layers::read_layers(&fork.graph())
            .layers
            .iter()
            .filter(|l| l.print.is_some())
            .count();
        if self.base.encryption().is_some() {
            std::sync::Arc::make_mut(&mut fork.base).clear_encryption();
            fork.trailer.remove(b"Encrypt");
            report.decrypted = true;
        }
        fork.catalog_without(&[b"Perms"])?;
        fork.draw_missing_looks(free_text_c, &mut report)?;
        fork.burn_every_page(&mut report)?;
        fork.catalog_without(&[b"AcroForm"])?;
        report.disclosures = disclosures(&report);
        let bytes = fork.print_ready_file(report.decrypted)?;
        Ok(PrintReadyCopy { bytes, report })
    }

    /// Commit the catalog with `keys` removed, if any is present.
    fn catalog_without(&mut self, keys: &[&[u8]]) -> Result<(), EditError> {
        let Some(id) = self.trailer.get(b"Root").and_then(Object::as_reference) else {
            return Ok(());
        };
        let Some(Object::Dict(catalog)) = self.value(id) else {
            return Ok(());
        };
        if !keys.iter().any(|k| catalog.contains_key(k)) {
            return Ok(());
        }
        let mut updated = catalog.clone();
        for key in keys {
            updated.remove(key);
        }
        let write = ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(Object::Dict(updated)),
        };
        self.commit_print_step(vec![write], Vec::new());
        Ok(())
    }

    /// Give markup annotations and form fields with no look the one pdfcer
    /// draws. A form the generator refuses keeps the looks it has; a field
    /// left without one is a field that paints nothing, as in a viewer.
    fn draw_missing_looks(
        &mut self,
        free_text_c: FreeTextC,
        report: &mut PrintReadyReport,
    ) -> Result<(), EditError> {
        report.appearances_drawn = self.bake_missing_appearances(free_text_c)?.baked.len();
        if crate::forms::parse_acroform(&self.graph()).is_some()
            && let Ok(regen) = self.regenerate_appearances()
        {
            report.fields_generated = regen.regenerated;
        }
        Ok(())
    }

    fn burn_every_page(&mut self, report: &mut PrintReadyReport) -> Result<(), EditError> {
        let slots = self.page_slots()?;
        let rotations: Vec<u16> = self.pages()?.iter().map(|p| p.rotate).collect();
        let mut removing: BTreeSet<ObjId> = BTreeSet::new();
        for (index, slot) in slots.iter().enumerate() {
            let rotate = rotations.get(index).copied().unwrap_or(0);
            let on_page = crate::annot::page_annotations(&self.graph(), slot.id);
            if on_page.is_empty() {
                continue;
            }
            let mut burns = Vec::new();
            for a in &on_page {
                removing.extend(a.id);
                if let Some(burn) = self.print_burn(a, rotate, index, report) {
                    burns.push(burn);
                }
            }
            self.burn_page(slot.id, &slots, burns)?;
        }
        let removing: Vec<ObjId> = removing.into_iter().collect();
        self.refuse_if_in_page_tree("the annotation", &removing)?;
        let removals = removing
            .into_iter()
            .filter(|id| self.base.get(*id).is_some() || self.state.contains_key(id))
            .map(|id| Removal {
                id,
                was_deleted: self.deleted.contains(&id),
                is_deleted: true,
            })
            .collect();
        self.commit_print_step(Vec::new(), removals);
        Ok(())
    }

    /// The burn for `a` if it prints, recording what became of it either way.
    fn print_burn(
        &self,
        a: &Annotation,
        rotate: u16,
        page_index: usize,
        report: &mut PrintReadyReport,
    ) -> Option<AnnotBurn> {
        if a.is_popup || a.subtype == b"Popup" {
            return None;
        }
        if !prints(a) {
            report.not_printing += 1;
            return None;
        }
        match self.annot_burn_placement(a, rotate) {
            Ok((ap, cm)) => {
                report.burned += 1;
                report.printed_not_on_screen += usize::from(a.flags.no_view());
                report.redaction_marks += usize::from(a.subtype == b"Redact");
                Some(self.annot_burn_of(a, ap, cm))
            }
            Err(reason) => {
                if !matches!(a.subtype.as_slice(), b"Link" | b"Widget") {
                    report.lost.push(PrintReadyLoss {
                        page_index,
                        subtype: a.subtype.clone(),
                        reason,
                    });
                }
                None
            }
        }
    }

    /// Paint `burns` over the page's content and drop its `/Annots`.
    fn burn_page(
        &mut self,
        page_id: ObjId,
        slots: &[PageSlot],
        burns: Vec<AnnotBurn>,
    ) -> Result<(), EditError> {
        let Some(Object::Dict(page_dict)) = self.value(page_id) else {
            return Err(EditError::NotADictionary {
                id: page_id,
                key: "Annots",
            });
        };
        let mut updated = page_dict.clone();
        updated.remove(b"Annots");
        let mut objects = Vec::new();
        if !burns.is_empty() {
            let mut unused = AnnotFlattenOutcome::default();
            let (overlay, xobjects) =
                self.burn_overlay(burns, page_id, slots, &mut objects, &mut unused)?;
            let mut wrap = None;
            self.append_page_content(&mut updated, overlay, &mut wrap, &mut objects)?;
            self.add_page_xobjects(&mut updated, page_id, &xobjects, slots);
        }
        objects.push(self.page_write(page_id, updated));
        self.commit_print_step(objects, Vec::new());
        Ok(())
    }

    fn commit_print_step(&mut self, objects: Vec<ObjectWrite>, removals: Vec<Removal>) {
        self.commit(Command {
            kind: CommandKind::FlattenAnnotations { count: 0 },
            objects,
            removals,
            trailer: None,
        });
    }

    /// A full save, reloaded and rebuilt so nothing unreachable survives.
    /// `decrypted`: the bytes under the base are ciphertext, so they are
    /// re-serialised rather than copied.
    fn print_ready_file(&self, decrypted: bool) -> Result<Vec<u8>, WriteError> {
        let options = SaveOptions::default();
        let dirty = self.save_dirty_set(&options);
        let (full, _) = if decrypted {
            writer::save_full_decrypted(&self.base, &dirty, &options)?
        } else {
            writer::save_full(&self.base, &dirty, &options)?
        };
        let doc = Document::from_bytes(full).map_err(|e| WriteError::RewriteReload(Box::new(e)))?;
        let structure = RebuildOptions {
            remove_unreferenced: true,
            merge_duplicates: false,
            object_streams: false,
            level: 6,
            linearize: false,
        };
        let (bytes, _) = writer::rebuild(&doc, &structure, &options)?;
        Ok(bytes)
    }
}

/// Table 22, for a document opened with a user password. The owner password
/// allows everything.
fn check_print_permission(doc: &Document) -> Result<(), PrintReadyError> {
    let Some(enc) = doc.encryption() else {
        return Ok(());
    };
    if !enc.grants(PermissionBit::Print) {
        return Err(PrintReadyError::PrintingForbidden);
    }
    // `None` is a revision with no such bit, where printing is printing.
    if enc.auth != crate::crypto::AuthKind::Owner
        && enc
            .config
            .permissions()
            .granted(PermissionBit::PrintHighQuality)
            == Some(false)
    {
        return Err(PrintReadyError::LowQualityPrintingOnly);
    }
    Ok(())
}

/// §12.5.3 Table 165.
fn prints(a: &Annotation) -> bool {
    if a.flags.hidden() || !a.flags.print() {
        return false;
    }
    !a.flags.invisible() || STANDARD_SUBTYPES.contains(&a.subtype.as_slice())
}

fn disclosures(report: &PrintReadyReport) -> Vec<String> {
    let mut out = Vec::new();
    let mut say = |n: usize, text: &str| {
        if n > 0 {
            out.push(format!("{n} {text}"));
        }
    };
    say(
        report.not_printing,
        "annotation(s) set not to print were left out",
    );
    say(
        report.printed_not_on_screen,
        "annotation(s) hidden on screen but set to print were burned in",
    );
    say(
        report.redaction_marks,
        "pending redaction mark(s) were burned in; the content under them is still in this copy",
    );
    say(
        report.appearances_drawn,
        "annotation(s) with no stored look were drawn by pdfcer",
    );
    say(
        report.fields_generated,
        "form field(s) with no stored look were generated by pdfcer from their values",
    );
    say(
        report.signatures_dropped,
        "signature(s) do not carry over to this copy",
    );
    say(
        report.layers_with_print_setting,
        "layer(s) have their own print setting, which the print service decides",
    );
    for loss in &report.lost {
        out.push(format!(
            "page {}: a /{} set to print has nothing to paint: {}",
            loss.page_index + 1,
            String::from_utf8_lossy(&loss.subtype),
            loss.reason
        ));
    }
    if report.dynamic_xfa {
        out.push(
            "this is a dynamic XFA form: the copy holds its placeholder pages, not the form"
                .to_owned(),
        );
    }
    if report.decrypted {
        out.push(
            "this copy is not encrypted, and the document's permissions do not travel with it"
                .to_owned(),
        );
    }
    out
}
