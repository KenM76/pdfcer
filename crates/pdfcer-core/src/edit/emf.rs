//! Placing an imported EMF ([`crate::emf_import`]) as vector content:
//! [`EditSession::add_emf`] into the page content, [`EditSession::add_emf_stamp`]
//! as a `/Stamp` annotation's appearance.

use super::{AnnotKind, Command, CommandKind, EditError, EditSession, MarkupOptions, ObjectWrite};
use crate::crypto::PermissionBit;
use crate::emf_import::{EmfImportNotes, ImportedEmf};
use crate::fontdata::std14_base_font_name;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::{PageSlot, Rect};
use crate::writer::content::ContentBuilder;

/// What [`EditSession::add_emf`] or [`EditSession::add_emf_stamp`] wrote.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlacedEmf {
    /// The Form XObject holding the picture (`/BBox [0 0 w h]`, the EMF
    /// frame in points).
    pub form_id: ObjId,
    /// The appended page content stream that invokes the form
    /// ([`EditSession::add_emf`] only).
    pub content_id: Option<ObjId>,
    /// The `/Stamp` annotation ([`EditSession::add_emf_stamp`] only).
    pub annot_id: Option<ObjId>,
    /// The rectangle the picture fills, normalised.
    pub rect: Rect,
    /// Horizontal stretch: `rect.width / frame width`.
    pub scale_x: f64,
    /// Vertical stretch: `rect.height / frame height`.
    pub scale_y: f64,
    /// The two factors differ: the picture is squashed or stretched.
    pub distorted: bool,
    /// New objects written besides the form: image XObjects and their soft
    /// masks.
    pub objects_written: usize,
    /// What the import skipped, approximated or substituted — the
    /// disclosure a shell shows.
    pub notes: EmfImportNotes,
}

impl PlacedEmf {
    /// One line for a status bar or a CLI report: the placement, then
    /// [`EmfImportNotes::summary`] when there is anything to disclose.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut s = format!(
            "placed EMF in [{:.2} {:.2} {:.2} {:.2}]",
            self.rect.llx, self.rect.lly, self.rect.urx, self.rect.ury
        );
        if self.distorted {
            s.push_str(&format!(
                " (stretched: x {:.4}, y {:.4})",
                self.scale_x, self.scale_y
            ));
        }
        if !self.notes.is_empty() {
            s.push_str("; ");
            s.push_str(&self.notes.summary());
        }
        s
    }
}

impl EditSession {
    /// Draw `emf` on page `page_index`, stretched to fill `rect`, as a Form
    /// XObject (ISO 32000-1 §8.10) invoked from a new content stream
    /// appended to the page (§7.8.2). One undo entry ([`CommandKind::AddEmf`]).
    ///
    /// The picture stays vector: paths, clips and text (in a standard-14
    /// substitute face, §9.6.2.2); bitmaps become image XObjects. What could
    /// not be carried is listed in [`PlacedEmf::notes`]; the caller
    /// discloses it. A caller wanting the aspect kept passes a `rect` of the
    /// [`ImportedEmf::natural_size_pt`] proportions.
    ///
    /// # Errors
    ///
    /// As [`Self::add_image`]: [`EditError::ImageRectDegenerate`],
    /// [`EditError::DocumentEncrypted`], the strict certification gate,
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    /// [`EditError::PageOutOfRange`], [`EditError::ObjectNumbersExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{document::Document, edit::EditSession, page_tree::Rect};
    /// # fn demo(doc: Document, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    /// let emf = pdfcer_core::emf_import::import(bytes)?;
    /// let (w, h) = emf.natural_size_pt();
    /// let mut session = EditSession::new(doc);
    /// let rect = Rect { llx: 72.0, lly: 72.0, urx: 72.0 + w, ury: 72.0 + h };
    /// let placed = session.add_emf(0, rect, &emf)?;
    /// println!("{}", placed.summary());
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_emf(
        &mut self,
        page_index: usize,
        rect: Rect,
        emf: &ImportedEmf,
    ) -> Result<PlacedEmf, EditError> {
        let rect = normalised(rect)?;
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.forbid_hidden_objects()?;
        let (page_id, slots) = self.emf_page(page_index)?;
        let mut objects = Vec::new();
        let form_id = self.stage_emf_form(emf, &mut objects)?;
        let objects_written = objects.len() - 1;
        let (sx, sy) = scales(emf, rect);

        let name = self.free_xobject_name(page_id, &slots);
        let mut cb = ContentBuilder::new();
        cb.save_state();
        cb.concat_matrix(sx, 0.0, 0.0, sy, rect.llx, rect.lly);
        cb.invoke_xobject(&name);
        cb.restore_state();
        let content = cb.into_bytes();
        let content_id = ObjId::new(self.alloc_number()?, 0);
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Length"), int(content.len()));
        objects.push(ObjectWrite {
            id: content_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict,
                data_span: self.stage_bytes(&content),
            })),
        });

        let Some(Object::Dict(page_dict)) = self.value(page_id) else {
            return Err(EditError::NotADictionary {
                id: page_id,
                key: "Contents",
            });
        };
        let mut updated = page_dict.clone();
        let mut wrap = None;
        self.append_page_content(&mut updated, content_id, &mut wrap, &mut objects)?;
        self.add_page_xobjects(&mut updated, page_id, &[(name, form_id)], &slots);
        objects.push(ObjectWrite {
            id: page_id,
            before: self.state.get(&page_id).cloned(),
            after: Some(Object::Dict(updated)),
        });
        self.commit(Command {
            kind: CommandKind::AddEmf,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(placed(
            form_id,
            (Some(content_id), None),
            rect,
            objects_written,
            emf,
        ))
    }

    /// Place `emf` as a `/Stamp` annotation (§12.5.6.12) whose `/AP /N` is
    /// the picture's Form XObject, stretched to `rect` by the §12.5.5
    /// appearance algorithm. One undo entry ([`CommandKind::AddAnnotation`]).
    ///
    /// `options` supplies `/CA`, the note (`/Contents`, `/T`, `/M`) and the
    /// layer, as on every markup verb.
    ///
    /// # Errors
    ///
    /// The [`MarkupOptions`] refusals; then as [`Self::add_emf`], except that
    /// the gate is the annotation one (`/P` 2 certification permits it).
    pub fn add_emf_stamp(
        &mut self,
        page_index: usize,
        rect: Rect,
        emf: &ImportedEmf,
        options: &MarkupOptions,
    ) -> Result<PlacedEmf, EditError> {
        options.validate()?;
        self.on_layer_if(page_index, options.layer, |s| {
            s.stamp_emf(page_index, rect, emf, options)
        })
    }

    fn stamp_emf(
        &mut self,
        page_index: usize,
        rect: Rect,
        emf: &ImportedEmf,
        options: &MarkupOptions,
    ) -> Result<PlacedEmf, EditError> {
        let rect = normalised(rect)?;
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        self.forbid_hidden_objects()?;
        let (page_id, slots) = self.emf_page(page_index)?;
        let mut objects = Vec::new();
        let form_id = self.stage_emf_form(emf, &mut objects)?;
        let objects_written = objects.len() - 1;
        let annot_id = ObjId::new(self.alloc_number()?, 0);
        let mut annot = Dict::new();
        annot.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
        annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Stamp")));
        annot.insert(
            Name::from(b"Rect"),
            reals(&[rect.llx, rect.lly, rect.urx, rect.ury]),
        );
        let mut ap = Dict::new();
        ap.insert(Name::from(b"N"), Object::Reference(form_id));
        annot.insert(Name::from(b"AP"), Object::Dict(ap));
        annot.insert(
            Name::from(b"F"),
            Object::Integer(i64::from(crate::annot::AnnotFlags::PRINT)),
        );
        super::insert_markup_entries(&mut annot, options);
        objects.push(ObjectWrite {
            id: annot_id,
            before: None,
            after: Some(Object::Dict(annot)),
        });
        let mut annots = self.annots_append(page_id, &[annot_id], &slots)?;
        objects.append(&mut annots);
        self.commit(Command {
            kind: CommandKind::AddAnnotation {
                kind: AnnotKind::Stamp,
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(placed(
            form_id,
            (None, Some(annot_id)),
            rect,
            objects_written,
            emf,
        ))
    }

    /// Refuse to allocate object numbers while the base document has
    /// objects the reader suppressed: a new number could collide with one.
    pub(super) fn forbid_hidden_objects(&self) -> Result<(), EditError> {
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        Ok(())
    }

    fn emf_page(&self, page_index: usize) -> Result<(ObjId, Vec<PageSlot>), EditError> {
        let slots = self.page_slots()?;
        let page_id = slots
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count: slots.len(),
            })?
            .id;
        Ok((page_id, slots))
    }

    /// Stage the picture's images, then its Form XObject (pushed last, so
    /// `objects.len() - 1` counts the rest); returns the form's id.
    fn stage_emf_form(
        &mut self,
        emf: &ImportedEmf,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        let mut xobjects = Dict::new();
        for (name, img) in &emf.images {
            let (id, _) = self.stage_image_xobject(img, objects)?;
            xobjects.insert(Name::from(name.as_bytes()), Object::Reference(id));
        }
        let mut fonts = Dict::new();
        for (name, face) in &emf.fonts {
            let mut f = Dict::new();
            f.insert(Name::from(b"Type"), Object::Name(Name::from(b"Font")));
            f.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Type1")));
            let base = std14_base_font_name(*face).as_bytes();
            f.insert(Name::from(b"BaseFont"), Object::Name(Name::from(base)));
            f.insert(
                Name::from(b"Encoding"),
                Object::Name(Name::from(b"WinAnsiEncoding")),
            );
            fonts.insert(Name::from(name.as_bytes()), Object::Dict(f));
        }
        let mut resources = Dict::new();
        if !xobjects.is_empty() {
            resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
        }
        if !fonts.is_empty() {
            resources.insert(Name::from(b"Font"), Object::Dict(fonts));
        }
        let (w, h) = emf.natural_size_pt();
        let data = crate::filters::flate::encode(&emf.content);
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
        dict.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Form")));
        dict.insert(Name::from(b"BBox"), reals(&[0.0, 0.0, w, h]));
        dict.insert(Name::from(b"Resources"), Object::Dict(resources));
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );
        dict.insert(Name::from(b"Length"), int(data.len()));
        let form_id = ObjId::new(self.alloc_number()?, 0);
        objects.push(ObjectWrite {
            id: form_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict,
                data_span: self.stage_bytes(&data),
            })),
        });
        Ok(form_id)
    }
}

fn normalised(rect: Rect) -> Result<Rect, EditError> {
    let rect = Rect::from_corners(rect.llx, rect.lly, rect.urx, rect.ury);
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 || w.is_nan() || h.is_nan() {
        return Err(EditError::ImageRectDegenerate { w, h });
    }
    Ok(rect)
}

fn scales(emf: &ImportedEmf, rect: Rect) -> (f64, f64) {
    let (w, h) = emf.natural_size_pt();
    (rect.width() / w, rect.height() / h)
}

fn placed(
    form_id: ObjId,
    (content_id, annot_id): (Option<ObjId>, Option<ObjId>),
    rect: Rect,
    objects_written: usize,
    emf: &ImportedEmf,
) -> PlacedEmf {
    let (scale_x, scale_y) = scales(emf, rect);
    PlacedEmf {
        form_id,
        content_id,
        annot_id,
        rect,
        scale_x,
        scale_y,
        distorted: (scale_x - scale_y).abs() > 1e-6 * scale_x.abs().max(scale_y.abs()),
        objects_written,
        notes: emf.notes().clone(),
    }
}

/// A count or length as a PDF integer, saturating instead of wrapping.
pub(super) fn int(n: usize) -> Object {
    Object::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn reals(v: &[f64]) -> Object {
    Object::Array(v.iter().map(|&x| Object::Real(x)).collect())
}
