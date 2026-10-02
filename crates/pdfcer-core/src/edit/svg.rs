//! Placing an imported SVG ([`crate::svg_import`]) as vector content:
//! [`EditSession::add_svg`] into the page content, [`EditSession::add_svg_stamp`]
//! as a `/Stamp` annotation's appearance.

use std::collections::HashMap;

use super::{AnnotKind, Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::crypto::PermissionBit;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::Rect;
use crate::svg_import::{ImportedSvg, SvgImportNotes, SvgObject};
use crate::writer::content::ContentBuilder;

/// What [`EditSession::add_svg`] or [`EditSession::add_svg_stamp`] wrote.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlacedSvg {
    /// The Form XObject holding the drawing (`/BBox [0 0 w h]` in SVG px).
    pub form_id: ObjId,
    /// The appended page content stream that invokes the form
    /// ([`EditSession::add_svg`] only).
    pub content_id: Option<ObjId>,
    /// The `/Stamp` annotation ([`EditSession::add_svg_stamp`] only).
    pub annot_id: Option<ObjId>,
    /// The rectangle the drawing fills, normalised.
    pub rect: Rect,
    /// Horizontal stretch: `rect.width / svg width`, in points per SVG px.
    pub scale_x: f64,
    /// Vertical stretch: `rect.height / svg height`.
    pub scale_y: f64,
    /// The two factors differ: the drawing is squashed or stretched.
    pub distorted: bool,
    /// New objects written, the form and page/annotation edits excluded.
    pub objects_written: usize,
    /// What the import skipped or approximated — the disclosure a shell
    /// shows.
    pub notes: SvgImportNotes,
}

impl EditSession {
    /// Draw `svg` on page `page_index`, stretched to fill `rect`, as a
    /// Form XObject (ISO 32000-1 §8.10) invoked from a new content stream
    /// appended to the page (§7.8.2). One undo entry ([`CommandKind::AddSvg`]).
    ///
    /// The drawing stays vector: paths, gradients (shading patterns,
    /// §8.7.4.5), patterns (tiling patterns, §8.7.3), clips, masks and
    /// opacity (transparency groups and soft masks, §11.6). Embedded raster
    /// images become image XObjects. What could not be carried — text,
    /// filters, external images — is listed in [`PlacedSvg::notes`]; the
    /// caller discloses it.
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
    /// # fn demo(doc: Document) -> Result<(), Box<dyn std::error::Error>> {
    /// let svg = pdfcer_core::svg_import::import(
    ///     br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
    ///         <rect width="20" height="10" fill="red"/></svg>"#,
    /// )?;
    /// let mut session = EditSession::new(doc);
    /// let rect = Rect { llx: 72.0, lly: 72.0, urx: 272.0, ury: 172.0 };
    /// let placed = session.add_svg(0, rect, &svg)?;
    /// assert!(!placed.distorted);
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_svg(
        &mut self,
        page_index: usize,
        rect: Rect,
        svg: &ImportedSvg,
    ) -> Result<PlacedSvg, EditError> {
        let rect = Rect::from_corners(rect.llx, rect.lly, rect.urx, rect.ury);
        let (rw, rh) = (rect.width(), rect.height());
        if rw <= 0.0 || rh <= 0.0 || rw.is_nan() || rh.is_nan() {
            return Err(EditError::ImageRectDegenerate { w: rw, h: rh });
        }
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.forbid_hidden_objects()?;
        let slots = self.page_slots()?;
        let page_id = slots
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count: slots.len(),
            })?
            .id;

        let mut objects = Vec::new();
        let form_id = self.stage_svg_objects(svg, &mut objects)?;
        let objects_written = objects.len() - 1;
        let (sx, sy) = scales(svg, rect);

        let name = self.free_xobject_name(page_id, &slots);
        let mut cb = ContentBuilder::new();
        cb.save_state();
        cb.concat_matrix(sx, 0.0, 0.0, sy, rect.llx, rect.lly);
        cb.invoke_xobject(&name);
        cb.restore_state();
        let content = cb.into_bytes();
        let content_id = ObjId::new(self.alloc_number()?, 0);
        let span = self.stage_bytes(&content);
        objects.push(ObjectWrite {
            id: content_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: length_dict(content.len()),
                data_span: span,
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
            kind: CommandKind::AddSvg,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(placed(
            form_id,
            Some(content_id),
            None,
            rect,
            (sx, sy),
            objects_written,
            svg,
        ))
    }

    /// Place `svg` as a `/Stamp` annotation (§12.5.6.12) whose `/AP /N` is
    /// the drawing's Form XObject, stretched to `rect` by the §12.5.5
    /// appearance algorithm. No `/Name`: the face matches no standard stamp.
    /// One undo entry ([`CommandKind::AddAnnotation`]).
    ///
    /// # Errors
    ///
    /// As [`Self::add_svg`], except that the gate is the annotation one
    /// (`/P` 2 certification permits it).
    pub fn add_svg_stamp(
        &mut self,
        page_index: usize,
        rect: Rect,
        svg: &ImportedSvg,
    ) -> Result<PlacedSvg, EditError> {
        let rect = Rect::from_corners(rect.llx, rect.lly, rect.urx, rect.ury);
        let (rw, rh) = (rect.width(), rect.height());
        if rw <= 0.0 || rh <= 0.0 || rw.is_nan() || rh.is_nan() {
            return Err(EditError::ImageRectDegenerate { w: rw, h: rh });
        }
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        self.forbid_hidden_objects()?;
        let slots = self.page_slots()?;
        let page_id = slots
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count: slots.len(),
            })?
            .id;

        let mut objects = Vec::new();
        let form_id = self.stage_svg_objects(svg, &mut objects)?;
        let objects_written = objects.len() - 1;
        let annot_id = ObjId::new(self.alloc_number()?, 0);
        let mut annot = Dict::new();
        annot.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
        annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Stamp")));
        annot.insert(
            Name::from(b"Rect"),
            Object::Array(
                [rect.llx, rect.lly, rect.urx, rect.ury]
                    .map(Object::Real)
                    .to_vec(),
            ),
        );
        let mut ap = Dict::new();
        ap.insert(Name::from(b"N"), Object::Reference(form_id));
        annot.insert(Name::from(b"AP"), Object::Dict(ap));
        annot.insert(
            Name::from(b"F"),
            Object::Integer(i64::from(crate::annot::AnnotFlags::PRINT)),
        );
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
        let s = scales(svg, rect);
        Ok(placed(
            form_id,
            None,
            Some(annot_id),
            rect,
            s,
            objects_written,
            svg,
        ))
    }

    fn forbid_hidden_objects(&self) -> Result<(), EditError> {
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        Ok(())
    }

    /// Number the import's objects into this document and stage them;
    /// returns the root form's id.
    fn stage_svg_objects(
        &mut self,
        svg: &ImportedSvg,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        let mut ids: HashMap<ObjId, ObjId> = HashMap::with_capacity(svg.objects.len());
        let mut numbered = Vec::with_capacity(svg.objects.len());
        for (i, obj) in svg.objects.iter().enumerate() {
            let local = local_id(i);
            let id = match obj {
                SvgObject::Image(img) => self.stage_image_xobject(img, objects)?.0,
                SvgObject::Dict(_) | SvgObject::Stream { .. } => {
                    ObjId::new(self.alloc_number()?, 0)
                }
            };
            ids.insert(local, id);
            numbered.push(id);
        }
        for (obj, id) in svg.objects.iter().zip(numbered) {
            let after = match obj {
                SvgObject::Image(_) => continue,
                SvgObject::Dict(d) => Object::Dict(remap_dict(d, &ids)),
                SvgObject::Stream { dict, data } => {
                    let mut dict = remap_dict(dict, &ids);
                    dict.insert(Name::from(b"Length"), length(data.len()));
                    Object::Stream(Stream {
                        dict,
                        data_span: self.stage_bytes(data),
                    })
                }
            };
            objects.push(ObjectWrite {
                id,
                before: None,
                after: Some(after),
            });
        }
        // The root form goes last so `objects.len() - 1` counts the rest.
        let root = ids.get(&svg.root).copied().unwrap_or(svg.root);
        if let Some(pos) = objects.iter().position(|w| w.id == root) {
            let w = objects.remove(pos);
            objects.push(w);
        }
        Ok(root)
    }
}

fn local_id(index: usize) -> ObjId {
    ObjId::new(u32::try_from(index + 1).unwrap_or(u32::MAX), 0)
}

fn scales(svg: &ImportedSvg, rect: Rect) -> (f64, f64) {
    let (w, h) = svg.size_px();
    (rect.width() / w, rect.height() / h)
}

fn placed(
    form_id: ObjId,
    content_id: Option<ObjId>,
    annot_id: Option<ObjId>,
    rect: Rect,
    (scale_x, scale_y): (f64, f64),
    objects_written: usize,
    svg: &ImportedSvg,
) -> PlacedSvg {
    PlacedSvg {
        form_id,
        content_id,
        annot_id,
        rect,
        scale_x,
        scale_y,
        distorted: (scale_x - scale_y).abs() > 1e-6 * scale_x.abs().max(scale_y.abs()),
        objects_written,
        notes: svg.notes().clone(),
    }
}

fn length(n: usize) -> Object {
    Object::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn length_dict(n: usize) -> Dict {
    let mut d = Dict::new();
    d.insert(Name::from(b"Length"), length(n));
    d
}

fn remap_dict(d: &Dict, ids: &HashMap<ObjId, ObjId>) -> Dict {
    let mut out = Dict::new();
    for (k, v) in d.iter() {
        out.insert(k.clone(), remap(v, ids));
    }
    out
}

/// Importer objects nest only a few levels (pattern → shading → function
/// array), so this recursion is bounded by construction.
fn remap(o: &Object, ids: &HashMap<ObjId, ObjId>) -> Object {
    match o {
        Object::Reference(r) => Object::Reference(ids.get(r).copied().unwrap_or(*r)),
        Object::Array(a) => Object::Array(a.iter().map(|v| remap(v, ids)).collect()),
        Object::Dict(d) => Object::Dict(remap_dict(d, ids)),
        other => other.clone(),
    }
}
