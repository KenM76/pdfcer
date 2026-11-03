//! Writing a dynamic XFA form's computed pages into the file
//! ([`EditSession::materialize_xfa`]; decision 204).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use std::collections::HashMap;

use crate::xfa::{
    FontProgramKind, ImageStreams, LayoutPage, PaintNote, PaintOptions, PaintedFont, PaintedPage,
    Template, XfaFonts, XfaImages, form_kind, paint_page_with,
};

/// What [`EditSession::materialize_xfa`] does with a signed form's
/// signatures, which the new pages cannot carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum XfaSignatures {
    /// Refuse with [`EditError::XfaSigned`], or
    /// [`EditError::CertificationForbidsChange`] when a certification
    /// forbids page changes. The default.
    #[default]
    Refuse,
    /// Remove them: the catalog's `/Perms` (certification and usage
    /// rights) and the AcroForm's `/SigFlags` go with the fields. A
    /// signature over the original revision then reports the document
    /// changed after signing, which it was.
    Remove,
}

/// What [`EditSession::materialize_xfa`] wrote and dropped, for the caller
/// to disclose: every page is inferred content (rule 4).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct XfaMaterializeOutcome {
    /// Pages written, one per layout page.
    pub pages_written: usize,
    /// Placeholder pages the new ones replaced.
    pub pages_replaced: usize,
    /// What each page drew differently or left out: (0-based page, note).
    pub notes: Vec<(usize, PaintNote)>,
    /// Images written as image XObjects.
    pub images: usize,
    /// Top-level `/AcroForm /Fields` entries removed with the placeholder
    /// pages.
    pub fields_dropped: usize,
    /// Whether a usage-rights signature (`/Perms /UR3`, §12.8.2.3) was
    /// removed: it signed the XFA form, which is gone.
    pub usage_rights_removed: bool,
    /// Signatures removed under [`XfaSignatures::Remove`], certification
    /// included, usage rights not.
    pub signatures_removed: usize,
    /// Each supplied face drawn, once per `/BaseFont`: embedded when its
    /// [`PaintedFont::program`] is set, else referenced by name (see its
    /// [`PaintedFont::refusal`]).
    pub fonts: Vec<PaintedFont>,
}

impl EditSession {
    /// Replace a dynamic XFA form's placeholder pages with `pages`, laid out
    /// from `template` (as [`crate::xfa::layout_template_with`] returns them
    /// for `fonts`) and drawn per `paint` in `fonts`, and remove the XFA
    /// layer. A signed form is refused
    /// unless `signatures` is [`XfaSignatures::Remove`]. One undo entry; committed at
    /// Save, never on open.
    ///
    /// Each page is drawn by [`crate::xfa::paint_page_with`] with
    /// images as XObjects (ISO 32000-1 §8.9.7 limits inline images to
    /// 4 KB) and the document's `/XFAImages` resolved. It gets its own
    /// `/MediaBox`, `/CropBox`, `/Rotate 0` and `/Resources`, so nothing is
    /// inherited from the page tree. The root `/Pages` node's `/Kids` become
    /// the new pages; the catalog loses `/NeedsRendering` and `/Perms
    /// /UR3`; the AcroForm loses `/XFA` and `/NeedsRendering` and gets an
    /// empty `/Fields`. The pages are flat: field values are drawn, not
    /// fillable. A supplied face is written once as an indirect simple font
    /// shared by every page, its whole program embedded per
    /// [`PaintOptions::font_embedding`] (`/FontFile2` for TrueType outlines,
    /// `/FontFile3 /OpenType` for CFF; ISO 32000-1 §9.9). Everything is
    /// reported in [`XfaMaterializeOutcome`].
    ///
    /// # Errors
    ///
    /// - [`EditError::DocumentEncrypted`]: the permissions forbid assembly
    ///   or modification.
    /// - [`EditError::CertificationForbidsChange`]: a certification
    ///   forbids it, under [`XfaSignatures::Refuse`].
    /// - [`EditError::XfaNotDynamic`]: not a dynamic XFA form (a static
    ///   one's pages are already the document; decision 020).
    /// - [`EditError::XfaSigned`]: the file carries signatures, which
    ///   replacing the pages would orphan, under [`XfaSignatures::Refuse`].
    /// - [`EditError::NotADictionary`]: no catalog or root `/Pages`.
    /// - [`EditError::ObjectNumbersExhausted`].
    pub fn materialize_xfa(
        &mut self,
        template: &Template,
        pages: &[LayoutPage],
        paint: &PaintOptions,
        fonts: &XfaFonts,
        signatures: XfaSignatures,
    ) -> Result<XfaMaterializeOutcome, EditError> {
        use crate::crypto::PermissionBit;
        use crate::encryption_gate::forbids;
        if forbids(
            &self.base,
            &[PermissionBit::Assemble, PermissionBit::ModifyContents],
        ) {
            return Err(EditError::DocumentEncrypted);
        }
        let remove = signatures == XfaSignatures::Remove;
        let census = if remove {
            self.signature_census()
        } else {
            self.check_certification()?
        };
        if !form_kind(&self.graph()).is_dynamic() {
            return Err(EditError::XfaNotDynamic);
        }
        // A usage-rights signature signs the XFA form being removed; it is
        // dropped and disclosed, not refused.
        let signed = census
            .signatures
            .saturating_sub(usize::from(self.has_usage_rights()));
        if signed > 0 && !remove {
            return Err(EditError::XfaSigned { signatures: signed });
        }
        let painted = self.paint_xfa_pages(template, pages, paint, fonts);
        let mut out = XfaMaterializeOutcome {
            pages_written: pages.len(),
            pages_replaced: self.page_slots().map_or(0, |s| s.len()),
            signatures_removed: if remove { signed } else { 0 },
            ..XfaMaterializeOutcome::default()
        };
        let mut objects = Vec::new();
        let (catalog_id, root_id) = self.catalog_and_root()?;
        let mut kids = Vec::with_capacity(painted.len());
        let mut shared = HashMap::new();
        for (i, (page, mut p)) in pages.iter().zip(painted).enumerate() {
            out.notes.extend(p.notes.iter().map(|n| (i, n.clone())));
            out.images += p.images.len();
            self.share_fonts(&mut p, &mut shared, &mut out, &mut objects)?;
            let id = self.write_xfa_page(page, p, root_id, &mut objects)?;
            kids.push(Object::Reference(id));
        }
        let mut root = self.dict_of(root_id);
        root.insert(
            Name::from(b"Count"),
            Object::Integer(i64::try_from(kids.len()).unwrap_or(i64::MAX)),
        );
        root.insert(Name::from(b"Kids"), Object::Array(kids));
        self.push_write(root_id, Object::Dict(root), &mut objects);
        self.strip_xfa(catalog_id, remove, &mut out, &mut objects);
        self.commit(Command {
            kind: CommandKind::MaterializeXfa {
                pages: out.pages_written,
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(out)
    }

    fn paint_xfa_pages(
        &self,
        template: &Template,
        pages: &[LayoutPage],
        paint: &PaintOptions,
        fonts: &XfaFonts,
    ) -> Vec<PaintedPage> {
        let images = XfaImages::from_view(&self.view());
        let opts = paint.with_image_streams(ImageStreams::XObject);
        pages
            .iter()
            .map(|p| paint_page_with(template, p, &opts, &images, fonts))
            .collect()
    }

    /// Point `painted`'s supplied faces at one indirect font dictionary per
    /// `/BaseFont`, writing it, and its program when embedded, on first use.
    fn share_fonts(
        &mut self,
        painted: &mut PaintedPage,
        shared: &mut HashMap<String, ObjId>,
        out: &mut XfaMaterializeOutcome,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(), EditError> {
        if !painted.fonts.iter().any(|f| f.supplied) {
            return Ok(());
        }
        let Some(Object::Dict(mut fonts)) = painted.resources.remove(b"Font") else {
            return Ok(());
        };
        for f in painted.fonts.iter().filter(|f| f.supplied) {
            let key = Name::from(f.name.as_bytes());
            let id = if let Some(id) = shared.get(&f.base_font) {
                *id
            } else {
                let Some(Object::Dict(mut dict)) = fonts.get(&key.0).cloned() else {
                    continue;
                };
                if let Some(program) = &f.program {
                    let file = self.font_file(program.kind, &program.bytes, objects)?;
                    if let Some(Object::Dict(mut desc)) = dict.remove(b"FontDescriptor") {
                        let slot = match program.kind {
                            FontProgramKind::TrueType => &b"FontFile2"[..],
                            _ => &b"FontFile3"[..],
                        };
                        desc.insert(Name::from(slot), Object::Reference(file));
                        dict.insert(Name::from(b"FontDescriptor"), Object::Dict(desc));
                    }
                }
                let id = ObjId::new(self.alloc_number()?, 0);
                objects.push(ObjectWrite {
                    id,
                    before: None,
                    after: Some(Object::Dict(dict)),
                });
                shared.insert(f.base_font.clone(), id);
                out.fonts.push(f.clone());
                id
            };
            fonts.insert(key, Object::Reference(id));
        }
        painted
            .resources
            .insert(Name::from(b"Font"), Object::Dict(fonts));
        Ok(())
    }

    /// A font program stream: `/Length1` for TrueType (§9.9 Table 125),
    /// `/Subtype /OpenType` for CFF outlines; Flate-compressed.
    fn font_file(
        &mut self,
        kind: FontProgramKind,
        bytes: &[u8],
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        let mut dict = Dict::new();
        match kind {
            FontProgramKind::TrueType => {
                let len = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
                dict.insert(Name::from(b"Length1"), Object::Integer(len));
            }
            _ => {
                dict.insert(
                    Name::from(b"Subtype"),
                    Object::Name(Name::from(b"OpenType")),
                );
            }
        }
        match crate::image_import::flate_encode(bytes) {
            Ok(z) => {
                dict.insert(
                    Name::from(b"Filter"),
                    Object::Name(Name::from(b"FlateDecode")),
                );
                self.new_stream(dict, &z, objects)
            }
            Err(_) => self.new_stream(dict, bytes, objects),
        }
    }

    fn catalog_and_root(&self) -> Result<(ObjId, ObjId), EditError> {
        let graph = self.graph();
        let missing = |id, key| EditError::NotADictionary { id, key };
        let catalog_id = graph
            .catalog_id()
            .ok_or_else(|| missing(ObjId::new(0, 0), "Root"))?;
        let root = graph
            .catalog_dict()
            .and_then(|c| c.get(b"Pages").cloned())
            .ok_or_else(|| missing(catalog_id, "Pages"))?;
        match root {
            Object::Reference(id) if graph.resolved(id).as_dict().is_some() => Ok((catalog_id, id)),
            _ => Err(missing(catalog_id, "Pages")),
        }
    }

    /// Write one page: its image XObjects, content stream and dictionary.
    fn write_xfa_page(
        &mut self,
        page: &LayoutPage,
        painted: PaintedPage,
        parent: ObjId,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        let PaintedPage {
            content,
            mut resources,
            images,
            ..
        } = painted;
        if !images.is_empty() {
            let mut xobjects = Dict::new();
            for img in images {
                let id = self.new_stream(img.dict, &img.data, objects)?;
                xobjects.insert(Name::from(img.name.as_bytes()), Object::Reference(id));
            }
            resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
        }
        let mut dict = Dict::new();
        let (contents, filter) = match crate::image_import::flate_encode(&content) {
            Ok(z) => (z, true),
            Err(_) => (content, false),
        };
        if filter {
            dict.insert(
                Name::from(b"Filter"),
                Object::Name(Name::from(b"FlateDecode")),
            );
        }
        let contents = self.new_stream(dict, &contents, objects)?;
        let bbox = Object::Array(
            [0.0, 0.0, page.width, page.height]
                .into_iter()
                .map(Object::Real)
                .collect(),
        );
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Page")));
        d.insert(Name::from(b"Parent"), Object::Reference(parent));
        d.insert(Name::from(b"MediaBox"), bbox.clone());
        d.insert(Name::from(b"CropBox"), bbox);
        d.insert(Name::from(b"Rotate"), Object::Integer(0));
        d.insert(Name::from(b"Resources"), Object::Dict(resources));
        d.insert(Name::from(b"Contents"), Object::Reference(contents));
        let id = ObjId::new(self.alloc_number()?, 0);
        objects.push(ObjectWrite {
            id,
            before: None,
            after: Some(Object::Dict(d)),
        });
        Ok(id)
    }

    /// Drop `/NeedsRendering`, `/Perms /UR3` (all of `/Perms` when
    /// `remove_signatures`) and the AcroForm's XFA layer and fields.
    fn strip_xfa(
        &self,
        catalog_id: ObjId,
        remove_signatures: bool,
        out: &mut XfaMaterializeOutcome,
        objects: &mut Vec<ObjectWrite>,
    ) {
        let mut catalog = self.dict_of(catalog_id);
        catalog.remove(b"NeedsRendering");
        if remove_signatures {
            out.usage_rights_removed = self.has_usage_rights();
            catalog.remove(b"Perms");
        }
        if let Some(perms) = self.deref_dict(catalog.get(b"Perms"))
            && perms.contains_key(b"UR3")
        {
            let mut perms = perms;
            perms.remove(b"UR3");
            out.usage_rights_removed = true;
            if perms.is_empty() {
                catalog.remove(b"Perms");
            } else {
                catalog.insert(Name::from(b"Perms"), Object::Dict(perms));
            }
        }
        let acro_ref = match catalog.get(b"AcroForm") {
            Some(Object::Reference(id)) => Some(*id),
            _ => None,
        };
        if let Some(mut acro) = self.deref_dict(catalog.get(b"AcroForm")) {
            out.fields_dropped = self.deref_array(acro.get(b"Fields")).map_or(0, |f| f.len());
            acro.remove(b"XFA");
            acro.remove(b"NeedsRendering");
            if remove_signatures {
                acro.remove(b"SigFlags");
            }
            acro.insert(Name::from(b"Fields"), Object::Array(Vec::new()));
            match acro_ref {
                Some(id) => self.push_write(id, Object::Dict(acro), objects),
                None => {
                    catalog.insert(Name::from(b"AcroForm"), Object::Dict(acro));
                }
            }
        }
        self.push_write(catalog_id, Object::Dict(catalog), objects);
    }

    fn has_usage_rights(&self) -> bool {
        let graph = self.graph();
        graph
            .catalog_dict()
            .and_then(|c| c.get(b"Perms"))
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
            .is_some_and(|p| p.contains_key(b"UR3"))
    }

    fn dict_of(&self, id: ObjId) -> Dict {
        self.graph()
            .resolved(id)
            .as_dict()
            .cloned()
            .unwrap_or_default()
    }

    fn deref_array(&self, o: Option<&Object>) -> Option<Vec<Object>> {
        let graph = self.graph();
        o.map(|o| graph.resolve(o))
            .and_then(Object::as_array)
            .map(<[Object]>::to_vec)
    }

    fn push_write(&self, id: ObjId, after: Object, objects: &mut Vec<ObjectWrite>) {
        objects.push(ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(after),
        });
    }
}
