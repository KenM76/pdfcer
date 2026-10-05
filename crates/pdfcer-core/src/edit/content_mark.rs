//! Marking what an add verb just wrote: an optional-content layer
//! (§8.11.3.2) and a hand-signature tag (§14.6, [`crate::hand_sig`]), one
//! mechanism for both.
//!
//! The add verbs append their content as new, unfiltered, self-contained
//! streams, so marking wraps each new stream whole and no pre-existing byte
//! is touched. The overlay save/restore pair a verb may add around the
//! page's original content is not the verb's output and is never wrapped.

use super::{Checkpoint, Command, EditError, EditSession, ObjectWrite};
use crate::hand_sig::HandSignatureError;
use crate::object::{Name, ObjId, Object, Stream};
use crate::view::StreamSource;

/// What an add verb's output is wrapped in. Empty means unmarked.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ContentMarks<'a> {
    /// The optional-content group the output goes on.
    pub(super) layer: Option<ObjId>,
    /// The signature field the output is the hand signature for.
    pub(super) hand_signature: Option<&'a str>,
    /// The `/Engine` of the pdfcer OCR layer the output joins.
    pub(super) ocr_layer: Option<&'a str>,
}

impl ContentMarks<'_> {
    /// Whether nothing is to be marked.
    pub(super) const fn is_empty(&self) -> bool {
        self.layer.is_none() && self.hand_signature.is_none() && self.ocr_layer.is_none()
    }
}

/// What a page held before an add verb ran, so its additions can be found.
#[derive(Debug, Clone)]
pub(super) struct AddedContentSnapshot {
    contents: Vec<ObjId>,
    annots: Vec<ObjId>,
    history: Checkpoint,
}

impl EditSession {
    /// The hand signatures still painting on page `page_index` in the edited
    /// state, in content order ([`crate::hand_sig`]).
    ///
    /// A page with no content has none. A field signed by more than one
    /// verb call, or by one call that wrote several streams, appears once
    /// per sequence.
    ///
    /// ```
    /// use pdfcer_core::annot_author::{Color, MarkupSpec};
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::edit::{EditSession, MarkupOptions};
    /// use pdfcer_core::page_tree::Rect;
    ///
    /// let bytes = std::fs::read(concat!(
    ///     env!("CARGO_MANIFEST_DIR"),
    ///     "/../../fixtures/synthetic/hello.pdf"
    /// ))?;
    /// let mut s = EditSession::new(Document::from_bytes(bytes)?);
    /// let spec = MarkupSpec::Ink {
    ///     strokes: vec![vec![(100.0, 100.0), (150.0, 120.0), (200.0, 100.0)]],
    ///     color: Color::Rgb(0.0, 0.0, 0.5),
    ///     width: 1.5,
    /// };
    /// let options = MarkupOptions {
    ///     hand_signature: Some("Approver.Signature".to_owned()),
    ///     ..MarkupOptions::default()
    /// };
    /// s.add_markup_as_content(0, &spec, &options)?;
    /// let found = s.hand_signatures(0)?;
    /// assert_eq!(found[0].field, "Approver.Signature");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`EditError::PageOutOfRange`] for an unknown page;
    /// [`EditError::VectorEditContent`] if the content cannot be decoded.
    pub fn hand_signatures(
        &mut self,
        page_index: usize,
    ) -> Result<Vec<crate::hand_sig::HandSignatureMark>, EditError> {
        let pages = self.pages()?;
        let count = pages.len();
        let page = pages
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count,
            })?
            .clone();
        if page.contents.is_empty() {
            return Ok(Vec::new());
        }
        let (stream, objects) = self.page_content_and_objects(&page)?;
        Ok(crate::hand_sig::marks_in(&stream, &objects))
    }

    /// Run the add verb `add`, then put what it added to page `page_index`
    /// on `layer` when there is one.
    pub(super) fn on_layer_if<T>(
        &mut self,
        page_index: usize,
        layer: Option<ObjId>,
        add: impl FnOnce(&mut Self) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        let marks = ContentMarks {
            layer,
            hand_signature: None,
            ocr_layer: None,
        };
        self.marked_if(page_index, marks, add)
    }

    /// Run the add verb `add`, then wrap what it added to page `page_index`
    /// in `marks`, as one undo entry labelled as the add.
    pub(super) fn marked_if<T>(
        &mut self,
        page_index: usize,
        marks: ContentMarks<'_>,
        add: impl FnOnce(&mut Self) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        if marks.is_empty() {
            return add(self);
        }
        let snapshot = self.added_content_snapshot(page_index, marks)?;
        let out = add(self)?;
        self.mark_added_content(page_index, &snapshot, marks)?;
        Ok(out)
    }

    /// Refuse marks that cannot be honoured, then record what page
    /// `page_index` holds before an add verb runs.
    pub(super) fn added_content_snapshot(
        &mut self,
        page_index: usize,
        marks: ContentMarks<'_>,
    ) -> Result<AddedContentSnapshot, EditError> {
        if let Some(layer) = marks.layer
            && !self.is_registered_layer(layer)
        {
            return Err(EditError::LayerNotFound { id: layer });
        }
        if let Some(field) = marks.hand_signature {
            crate::hand_sig::validate_field_name(field)?;
        }
        let pages = self.pages()?;
        let (contents, annots) = pages.get(page_index).map_or_else(Default::default, |page| {
            (page.contents.clone(), self.page_annot_refs(page.id))
        });
        Ok(AddedContentSnapshot {
            contents,
            annots,
            history: self.checkpoint(),
        })
    }

    /// The indirect annotations in page `page_id`'s `/Annots`.
    fn page_annot_refs(&self, page_id: ObjId) -> Vec<ObjId> {
        let annots = self
            .value(page_id)
            .and_then(Object::as_dict)
            .and_then(|d| self.deref_value(d.get(b"Annots")));
        match annots {
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_reference).collect(),
            _ => Vec::new(),
        }
    }

    /// Wrap what an add verb just added to page `page_index` in `marks`,
    /// relative to `before`.
    ///
    /// Every new content stream is wrapped whole: the layer's
    /// `/OC /name BDC … EMC` outermost, the hand-signature sequence inside
    /// it. Every new `/Annots` entry gets the layer's `/OC` (§12.5.2 Table
    /// 164); a hand signature marks content only. All of it, with any new
    /// `/Properties` binding, is folded with the add's own undo entries into
    /// one. On a failure the add is undone, so the verb lands marked or not
    /// at all.
    pub(super) fn mark_added_content(
        &mut self,
        page_index: usize,
        before: &AddedContentSnapshot,
        marks: ContentMarks<'_>,
    ) -> Result<(), EditError> {
        let result = self.wrap_added_content(page_index, before, marks);
        if result.is_err() {
            self.abandon(before.history.clone());
        }
        result
    }

    fn wrap_added_content(
        &mut self,
        page_index: usize,
        before: &AddedContentSnapshot,
        marks: ContentMarks<'_>,
    ) -> Result<(), EditError> {
        let added = self.pushed_since(&before.history);
        // The gesture is labelled by its first command: a paste's own entry,
        // not the last annotation it placed.
        let first = self.undo.len() - added;
        let Some(kind) = self.undo.get(first).filter(|_| added > 0).map(|c| c.kind) else {
            return Ok(());
        };
        let pages = self.pages()?;
        let count = pages.len();
        let page = pages
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count,
            })?
            .clone();
        let binding = marks
            .layer
            .map(|layer| (layer, self.layer_property_binding(&page, layer)));
        let (open, close) = sequence_brackets(binding.as_ref().map(|(_, (n, _))| n), marks);
        let new_streams: Vec<ObjId> = page
            .contents
            .iter()
            .filter(|id| !before.contents.contains(id))
            .copied()
            .collect();
        let mut objects = self.wrap_streams(&new_streams, &open, &close)?;
        let wrapped_any = !objects.is_empty();
        if marks.hand_signature.is_some() && !wrapped_any {
            return Err(HandSignatureError::NothingToMark.into());
        }
        if let Some(layer) = marks.layer {
            objects.extend(self.layer_new_annotations(page.id, before, layer));
        }
        if objects.is_empty() {
            return Ok(());
        }
        if let Some((layer, (name, true))) = &binding
            && wrapped_any
        {
            objects.extend(self.layer_binding_writes(page.id, name, *layer));
        }
        self.commit(Command {
            kind,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        self.coalesce_last(added + 1, kind);
        Ok(())
    }

    /// The writes wrapping each of `ids` whole in `open … close`, skipping
    /// the overlay save/restore pair.
    fn wrap_streams(
        &mut self,
        ids: &[ObjId],
        open: &[u8],
        close: &[u8],
    ) -> Result<Vec<ObjectWrite>, EditError> {
        let mut objects = Vec::new();
        for &id in ids {
            let Some(Object::Stream(stream)) = self.value(id).cloned() else {
                continue;
            };
            let data = StreamSource::Split {
                base: self.base.bytes(),
                staged: &self.staging,
            }
            .slice(stream.data_span)
            .map(<[u8]>::to_vec);
            let (None, Some(data)) = (stream.dict.get(b"Filter"), data) else {
                return Err(EditError::LayerContentNotRewritable {
                    stream: id,
                    reason: "the added content stream is filtered or unreadable",
                });
            };
            if data == crate::page_tree::WRAP_SAVE || data == crate::page_tree::WRAP_RESTORE {
                continue;
            }
            let mut wrapped = open.to_vec();
            wrapped.extend_from_slice(&data);
            wrapped.extend_from_slice(close);
            let mut dict = stream.dict.clone();
            dict.insert(
                Name::from(b"Length"),
                Object::Integer(i64::try_from(wrapped.len()).unwrap_or(i64::MAX)),
            );
            let data_span = self.stage_bytes(&wrapped);
            objects.push(ObjectWrite {
                id,
                before: Some(Object::Stream(stream)),
                after: Some(Object::Stream(Stream { dict, data_span })),
            });
        }
        Ok(objects)
    }

    /// The writes putting every annotation added since `before` on `layer`.
    fn layer_new_annotations(
        &self,
        page_id: ObjId,
        before: &AddedContentSnapshot,
        layer: ObjId,
    ) -> Vec<ObjectWrite> {
        self.page_annot_refs(page_id)
            .into_iter()
            .filter(|id| !before.annots.contains(id))
            .filter_map(|id| {
                let Some(Object::Dict(annot)) = self.value(id).cloned() else {
                    return None;
                };
                let mut updated = annot.clone();
                updated.insert(Name::from(b"OC"), Object::Reference(layer));
                Some(ObjectWrite {
                    id,
                    before: Some(Object::Dict(annot)),
                    after: Some(Object::Dict(updated)),
                })
            })
            .collect()
    }
}

/// The bytes opening and closing `marks` around one stream: a pdfcer OCR
/// marker outermost (it must open the stream, [`crate::ocr::marker`]), then
/// the layer's section, bound to the page's `/Properties` name `layer_name`.
fn sequence_brackets(layer_name: Option<&Name>, marks: ContentMarks<'_>) -> (Vec<u8>, Vec<u8>) {
    let mut open = Vec::new();
    let mut close = b"\n".to_vec();
    if let Some(engine) = marks.ocr_layer {
        crate::ocr::marker::open_marker(&mut open, Some(engine));
    }
    if let Some(name) = layer_name {
        open.extend_from_slice(b"/OC ");
        crate::writer::serialize::write_object(
            &mut open,
            &Object::Name(name.clone()),
            ObjId::new(0, 0),
            &[],
            &crate::writer::IdentityEncoder,
        );
        open.extend_from_slice(b" BDC\n");
    }
    if let Some(field) = marks.hand_signature {
        open.extend_from_slice(&crate::hand_sig::open_sequence(field));
        close.extend_from_slice(b"EMC\n");
    }
    if layer_name.is_some() {
        close.extend_from_slice(b"EMC\n");
    }
    if marks.ocr_layer.is_some() {
        close.extend_from_slice(b"EMC\n");
    }
    (open, close)
}
