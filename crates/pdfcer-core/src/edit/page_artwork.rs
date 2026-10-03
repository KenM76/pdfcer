//! Placing one page of another document onto a page of this one, as a Form
//! XObject (§8.10) carrying the source page's content and its own
//! `/Resources`: [`EditSession::place_page_artwork`] as a `/Stamp`
//! annotation's appearance, [`EditSession::place_page_content`] as page
//! content.

use std::collections::BTreeMap;

use super::emf::int;
use super::{AnnotKind, Command, CommandKind, EditError, EditSession, ObjectWrite, PlacedArtwork};
use crate::crypto::PermissionBit;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::Rect;
use crate::view::DocumentView;
use crate::writer::content::ContentBuilder;

/// What [`EditSession::place_page_content`] placed, and what it decided.
///
/// The disclosures are [`PlacedArtwork`]'s, for the same reasons; there is
/// no annotation, and the drawing is a new content stream instead.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PlacedPageContent {
    /// The form XObject holding the imported page.
    pub form_id: ObjId,
    /// The content stream appended to the page's `/Contents`, holding
    /// `q a 0 0 d e f cm /Name Do Q`.
    pub content_id: ObjId,
    /// The rectangle the source page's crop box was mapped onto, normalised.
    pub rect: Rect,
    /// `rect.width / crop_box.width`.
    pub scale_x: f64,
    /// `rect.height / crop_box.height`.
    pub scale_y: f64,
    /// The two factors differ: the artwork is squashed or stretched.
    pub distorted: bool,
    /// Objects copied into this document: the artwork's resource closure.
    pub objects_imported: usize,
    /// Resource names renamed to avoid the target page's: always `0`, since
    /// the form carries its own `/Resources` (§8.10.2 Table 96).
    pub resources_renamed: usize,
    /// Annotations on the source page that were not carried.
    pub source_annotations_ignored: usize,
    /// Form-field widgets on the source page that were not carried.
    pub source_widgets_ignored: usize,
    /// The source page's transparency `/Group` travelled with the artwork.
    pub transparency_group_carried: bool,
}

/// A source page staged as a form XObject, not yet committed.
struct StagedPage {
    form_id: ObjId,
    /// The form, then its imported resource closure.
    objects: Vec<ObjectWrite>,
    bbox: Rect,
    objects_imported: usize,
    carried_group: bool,
    source_annotations_ignored: usize,
    source_widgets: usize,
}

impl StagedPage {
    /// The factors mapping the form's `/BBox` onto `rect`; `1` on a
    /// degenerate box.
    fn scales(&self, rect: Rect) -> (f64, f64) {
        if self.bbox.width() > 0.0 && self.bbox.height() > 0.0 {
            (
                rect.width() / self.bbox.width(),
                rect.height() / self.bbox.height(),
            )
        } else {
            (1.0, 1.0)
        }
    }
}

impl EditSession {
    /// Place one page's **artwork** onto a page of this document as a
    /// `/Stamp` annotation whose appearance is that page, imported as a form
    /// XObject.
    ///
    /// # Why an annotation rather than page content
    ///
    /// This is what Acrobat does, and it is what an operator expects a stamp
    /// to be: selectable, movable, deletable, flattenable, and **the page's
    /// own content stream is never touched** (R47). Placing the same artwork
    /// as page content is [`Self::place_page_content`].
    ///
    /// # Why the artwork stays VECTOR
    ///
    /// Rendering the source page and placing a raster through
    /// [`Self::add_image`] was rejected: a bitmap is not what Acrobat writes,
    /// the operator's documents are CAD drawings where a raster does not
    /// survive zooming, and a resolution would have been chosen for them.
    ///
    /// # What is imported, and what is deliberately not
    ///
    /// **Imported:** the source page's content streams (decoded and
    /// concatenated as Table 30 concatenates them) and its resolved
    /// `/Resources`, deep-copied at fresh object numbers through the same
    /// importer `insert_pages` uses. Its `/Group` travels too when it has one
    /// (§8.10.2 Table 96).
    ///
    /// **Not imported:** the source page's ANNOTATIONS and form-field
    /// widgets. They live beside the content, and Adobe's `Dynamic.pdf`
    /// stamps carry AcroForm calculation machinery whose widgets would arrive
    /// as orphans. Both are COUNTED in [`PlacedArtwork`].
    ///
    /// ⚠ **A dynamic stamp therefore places its DESIGN-TIME text.** Its words
    /// come from JavaScript Acrobat runs at placement.
    /// `stamp_file::StampEntry::dynamic` tells a caller which stamps those
    /// are.
    ///
    /// # Resource-name collisions: there are none, by construction
    ///
    /// A form XObject carries **its own** `/Resources` (§8.10.2 Table 96), so
    /// the imported artwork's `/F1` and the target page's `/F1` never meet.
    /// [`PlacedArtwork::resources_renamed`] is always `0`, reported rather
    /// than omitted so "none happened" is distinguishable from "not
    /// reported".
    ///
    /// # Scaling is the annotation appearance algorithm's, and it is disclosed
    ///
    /// The form's `/BBox` is the source page's **crop box** with an identity
    /// `/Matrix`, and §12.5.5's appearance algorithm maps it onto `rect` with
    /// **independent** horizontal and vertical factors, so a non-matching
    /// aspect ratio stretches by definition (Acrobat's drag-placement lands
    /// in the same algorithm). [`PlacedArtwork::scale_x`],
    /// [`PlacedArtwork::scale_y`] and [`PlacedArtwork::distorted`] disclose
    /// it (rule 4).
    ///
    /// # No `/Name`, and the gap that is deliberately still open
    ///
    /// §12.5.6.12's `/Name` is a closed vocabulary of standard stamp names
    /// and imported artwork matches none of them, so none is written (Table
    /// 181 marks it optional). ⚠ Whether Acrobat writes something there for
    /// a CUSTOM stamp is an open GAP (R250): settling it needs a PDF with a
    /// custom stamp placed and saved by Acrobat (Reader can make one).
    ///
    /// # Errors
    ///
    /// - [`EditError::DocumentEncrypted`] and the annotation certification
    ///   gate, exactly as every other annotation-authoring verb.
    /// - [`EditError::PageOutOfRange`] — `page_index` is not a page of THIS
    ///   document.
    /// - [`EditError::SourcePageOutOfRange`] — `source_page` is not a page of
    ///   the source; its own variant because a shell shows the two mistakes
    ///   in different places.
    /// - [`EditError::PageTree`] — the source's page tree will not walk.
    /// - [`EditError::VectorEditContent`] — the source page's content streams
    ///   will not decode.
    pub fn place_page_artwork(
        &mut self,
        source: &DocumentView<'_>,
        source_page: usize,
        page_index: usize,
        rect: Rect,
    ) -> Result<PlacedArtwork, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        let (page_id, slots) = self.target_page(page_index)?;
        let staged = self.stage_source_page(source, source_page)?;
        let rect = Rect::from_corners(rect.llx, rect.lly, rect.urx, rect.ury);
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
        ap.insert(Name::from(b"N"), Object::Reference(staged.form_id));
        annot.insert(Name::from(b"AP"), Object::Dict(ap));
        annot.insert(
            Name::from(b"F"),
            Object::Integer(i64::from(crate::annot::AnnotFlags::PRINT)),
        );
        let (scale_x, scale_y) = staged.scales(rect);
        let mut objects = staged.objects;
        objects.push(ObjectWrite {
            id: annot_id,
            before: None,
            after: Some(Object::Dict(annot)),
        });
        objects.append(&mut self.annots_append(page_id, &[annot_id], &slots)?);
        self.commit(Command {
            kind: CommandKind::AddAnnotation {
                kind: AnnotKind::Stamp,
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(PlacedArtwork {
            annot_id,
            form_id: staged.form_id,
            rect,
            scale_x,
            scale_y,
            distorted: (scale_x - scale_y).abs() > 1e-6,
            objects_imported: staged.objects_imported,
            resources_renamed: 0,
            source_annotations_ignored: staged.source_annotations_ignored,
            source_widgets_ignored: staged.source_widgets,
            transparency_group_carried: staged.carried_group,
        })
    }

    /// Place one page of `source` into page `page_index`'s **content**: the
    /// same form XObject [`Self::place_page_artwork`] builds, drawn by a new
    /// content stream `q a 0 0 d e f cm /Name Do Q` appended to `/Contents`
    /// (§7.8.2) instead of becoming a stamp's appearance. One undo entry
    /// ([`CommandKind::PlacePageContent`]). Additive: the page's existing
    /// content streams stay byte-verbatim, and the form's own `/Resources`
    /// keep its names apart from the page's.
    ///
    /// The `cm` maps the source page's crop box onto `rect` with independent
    /// factors, as the stamp's §12.5.5 mapping does, so the result draws the
    /// same and [`PlacedPageContent::distorted`] discloses a stretch.
    /// Annotations and widgets on the source page are counted, not carried.
    ///
    /// # Errors
    ///
    /// - [`EditError::ImageRectDegenerate`] — `rect` has no area.
    /// - [`EditError::DocumentEncrypted`], the certification gate and
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`], as every
    ///   content-authoring verb.
    /// - The source-page errors of [`Self::place_page_artwork`]:
    ///   [`EditError::PageOutOfRange`], [`EditError::SourcePageOutOfRange`],
    ///   [`EditError::PageTree`], [`EditError::VectorEditContent`].
    pub fn place_page_content(
        &mut self,
        source: &DocumentView<'_>,
        source_page: usize,
        page_index: usize,
        rect: Rect,
    ) -> Result<PlacedPageContent, EditError> {
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
        let (page_id, slots) = self.target_page(page_index)?;
        let staged = self.stage_source_page(source, source_page)?;
        let (sx, sy) = staged.scales(rect);
        let name = self.free_xobject_name(page_id, &slots);
        let mut cb = ContentBuilder::new();
        cb.save_state();
        let (e, f) = (
            rect.llx - staged.bbox.llx * sx,
            rect.lly - staged.bbox.lly * sy,
        );
        cb.concat_matrix(sx, 0.0, 0.0, sy, e, f);
        cb.invoke_xobject(&name);
        cb.restore_state();
        let content = cb.into_bytes();
        let content_id = ObjId::new(self.alloc_number()?, 0);
        let span = self.stage_bytes(&content);
        let mut objects = staged.objects;
        let mut length = Dict::new();
        length.insert(Name::from(b"Length"), int(content.len()));
        objects.push(ObjectWrite {
            id: content_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: length,
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
        self.add_page_xobjects(&mut updated, page_id, &[(name, staged.form_id)], &slots);
        objects.push(ObjectWrite {
            id: page_id,
            before: self.state.get(&page_id).cloned(),
            after: Some(Object::Dict(updated)),
        });
        self.commit(Command {
            kind: CommandKind::PlacePageContent,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(PlacedPageContent {
            form_id: staged.form_id,
            content_id,
            rect,
            scale_x: sx,
            scale_y: sy,
            distorted: (sx - sy).abs() > 1e-6,
            objects_imported: staged.objects_imported,
            resources_renamed: 0,
            source_annotations_ignored: staged.source_annotations_ignored,
            source_widgets_ignored: staged.source_widgets,
            transparency_group_carried: staged.carried_group,
        })
    }

    fn target_page(
        &self,
        page_index: usize,
    ) -> Result<(ObjId, Vec<crate::page_tree::PageSlot>), EditError> {
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

    /// Import `source_page` as a form XObject: `/BBox` its crop box, its
    /// content streams concatenated (`ContentStream::from_page`, with the
    /// §7.3.10 degradation for a missing stream), its `/Resources` and any
    /// `/Group` deep-copied at fresh numbers. Annotations are counted first,
    /// so the disclosure describes the source rather than what survived.
    fn stage_source_page(
        &mut self,
        source: &DocumentView<'_>,
        source_page: usize,
    ) -> Result<StagedPage, EditError> {
        let source_pages =
            crate::page_tree::pages_in(source.graph()).map_err(EditError::PageTree)?;
        let src = source_pages
            .get(source_page)
            .ok_or(EditError::SourcePageOutOfRange {
                index: source_page,
                count: source_pages.len(),
            })?
            .clone();
        let content = crate::content::ContentStream::from_page(source, &src)
            .map_err(EditError::VectorEditContent)?;
        let artwork = content.buf.clone();
        let source_annots = crate::annot::page_annotations(source.graph(), src.id);
        let source_widgets = source_annots.iter().filter(|a| a.is_widget()).count();

        let mut mapping: BTreeMap<ObjId, ObjId> = BTreeMap::new();
        let mut scratch: BTreeMap<ObjId, Object> = BTreeMap::new();
        let resources = self.import_value(
            source,
            &Object::Dict(src.resources.clone()),
            &mut mapping,
            &mut scratch,
        )?;
        let group = source
            .graph()
            .value(src.id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Group"))
            .cloned();
        let group = match group {
            Some(g) => Some(self.import_value(source, &g, &mut mapping, &mut scratch)?),
            None => None,
        };

        let bbox = src.crop_box;
        let form_id = ObjId::new(self.alloc_number()?, 0);
        let span = self.stage_bytes(&artwork);
        let carried_group = group.is_some();
        let form = form_dict(bbox, resources, group, artwork.len());

        let objects_imported = scratch.len();
        let mut objects = vec![ObjectWrite {
            id: form_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: form,
                data_span: span,
            })),
        }];
        objects.extend(scratch.into_iter().map(|(id, value)| ObjectWrite {
            id,
            before: None,
            after: Some(value),
        }));
        Ok(StagedPage {
            form_id,
            objects,
            bbox,
            objects_imported,
            carried_group,
            source_annotations_ignored: source_annots.len() - source_widgets,
            source_widgets,
        })
    }
}

/// The form XObject dictionary for an imported page (§8.10.2 Table 95):
/// identity `/Matrix`, `/BBox` the page's crop box.
fn form_dict(bbox: Rect, resources: Object, group: Option<Object>, len: usize) -> Dict {
    let mut form = Dict::new();
    form.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
    form.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Form")));
    form.insert(
        Name::from(b"BBox"),
        Object::Array(
            [bbox.llx, bbox.lly, bbox.urx, bbox.ury]
                .map(Object::Real)
                .to_vec(),
        ),
    );
    form.insert(Name::from(b"Resources"), resources);
    if let Some(group) = group {
        form.insert(Name::from(b"Group"), group);
    }
    form.insert(Name::from(b"Length"), int(len));
    form
}
