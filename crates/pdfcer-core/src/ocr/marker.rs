//! Finding the OCR layers pdfcer wrote, so a re-run can replace them rather
//! than stack a second invisible copy of every word.
//!
//! # The marker
//!
//! [`super::layer::build_layer_content`] wraps each layer in one marked-content
//! sequence (ISO 32000-1 §14.6, Table 320) with an inline property list
//! (§14.6.2: every value is direct, so inline is permitted):
//!
//! ```text
//! /pdfc_OCR << /Producer (pdfcer) /Version 1 /Engine (ocrs) >> BDC
//! q BT 3 Tr … ET Q
//! EMC
//! ```
//!
//! `BDC … EMC` encloses `q … Q`, which encloses `BT … ET`: properly nested as
//! §14.6.1 requires. A reader that does not know the tag ignores it.
//!
//! The tag is a second-class name (Annex E: four-character prefix, `_`,
//! name; the `_` form is valid in both 1.7 and 2.0). The `pdfc` prefix is
//! **not yet registered** with the Adobe names list; registering it is a
//! public filing and the operator's call. No standard or registered OCR tag
//! exists (`iso32000__annex__e.md`).
//!
//! In a Tagged PDF the layer's text is neither structure content nor an
//! artifact; PDF/UA wants one or the other. That is unchanged by the marker.
//!
//! # What counts as a layer
//!
//! A **whole content stream** of the page's `/Contents` whose first operation
//! is that `BDC`, whose property list says `/Producer (pdfcer)`, and whose
//! last operation is the `EMC` that closes it. pdfcer always writes a layer as
//! its own appended stream, so the stream is the unit of identity and of
//! removal. A stream that merely *contains* the tag somewhere is not a layer:
//! removing it would remove content pdfcer did not write.
//!
//! Layers written by other software (unmarked mode-3 text) are not found. They
//! cannot be told apart from clipped or deliberately hidden text, so removing
//! them would be a guess.

use crate::content::{ContentStream, ContentTokenKind};
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::page_tree::{self, Page, PageTreeError};
use crate::view::DocumentView;

/// The marked-content tag around a pdfcer OCR layer.
pub const LAYER_TAG: &[u8] = b"pdfc_OCR";

/// The `/Producer` value that makes a tagged stream pdfcer's own.
pub const LAYER_PRODUCER: &[u8] = b"pdfcer";

/// The `/Version` of the marker this build writes.
pub const LAYER_VERSION: i64 = 1;

/// One OCR layer pdfcer wrote, as found on a page.
///
/// Valid for the revision it was found in. After any edit that rewrites the
/// page, find again rather than reuse it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OcrLayerRef {
    /// Zero-based page index.
    pub page_index: usize,
    /// The layer's content stream (one entry of the page's `/Contents`).
    pub content: ObjId,
    /// The `/Engine` recorded in the marker, if one was.
    pub engine: Option<String>,
    /// The marker's `/Version`.
    pub version: i64,
    /// The `/Resources /Font` names the layer's `Tf` operators use.
    pub font_names: Vec<Vec<u8>>,
}

/// Every pdfcer OCR layer in the document, page by page, in `/Contents` order.
///
/// # Errors
///
/// [`PageTreeError`] if the page tree cannot be walked. A content stream that
/// cannot be decoded or parsed is not a layer and is skipped.
pub fn find_ocr_layers(view: &DocumentView<'_>) -> Result<Vec<OcrLayerRef>, PageTreeError> {
    let pages = page_tree::pages_in(view)?;
    Ok(pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| page_ocr_layers(view, p, i))
        .collect())
}

/// The pdfcer OCR layers on one page — the cheap probe.
///
/// Decodes only this page's own content streams; no text extraction, no
/// form walk.
#[must_use]
pub fn page_ocr_layers(
    view: &DocumentView<'_>,
    page: &Page,
    page_index: usize,
) -> Vec<OcrLayerRef> {
    content_stream_ids(view, page)
        .into_iter()
        .filter_map(|id| {
            let (engine, version, font_names) = read_marker(view, id)?;
            Some(OcrLayerRef {
                page_index,
                content: id,
                engine,
                version,
                font_names,
            })
        })
        .collect()
}

/// The indirect content streams named by the page's `/Contents`.
pub(crate) fn content_stream_ids(view: &DocumentView<'_>, page: &Page) -> Vec<ObjId> {
    let Some(page_dict) = view.resolved(page.id).as_dict() else {
        return Vec::new();
    };
    let Some(contents) = page_dict.get(b"Contents") else {
        return Vec::new();
    };
    let items: Vec<&Object> = match contents {
        Object::Reference(_) => match view.resolve(contents) {
            Object::Array(a) => a.iter().collect(),
            _ => vec![contents],
        },
        Object::Array(a) => a.iter().collect(),
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|o| match o {
            Object::Reference(id) => Some(*id),
            _ => None,
        })
        .collect()
}

/// `(engine, version, font names)` when stream `id` is exactly one pdfcer
/// OCR layer, else `None`.
fn read_marker(view: &DocumentView<'_>, id: ObjId) -> Option<(Option<String>, i64, Vec<Vec<u8>>)> {
    let Object::Stream(stream) = view.resolved(id) else {
        return None;
    };
    let raw = view.slice(stream.data_span)?;
    let decoded = crate::filters::decode_stream(&stream.dict, raw).ok()?;
    let cs = ContentStream::parse(decoded).ok()?;
    let buf = cs.buf.as_slice();
    let ops: Vec<_> = cs.operations().collect();
    let (first, last) = (ops.first()?, ops.last()?);
    if first.operator_name(buf)? != b"BDC" || last.operator_name(buf)? != b"EMC" {
        return None;
    }
    let operand = |i: usize| match first.operands.get(i).map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(o)) => Some(o),
        _ => None,
    };
    if operand(0)?.as_name()?.as_bytes() != LAYER_TAG {
        return None;
    }
    let props = operand(1)?.as_dict()?;
    match props.get(b"Producer") {
        Some(Object::String(s)) if s.as_slice() == LAYER_PRODUCER => {}
        _ => return None,
    }
    // The opening BDC must be closed by the final EMC, not earlier: a stream
    // that closes the layer and then draws more is not wholly pdfcer's.
    let mut depth = 0_i64;
    let mut font_names: Vec<Vec<u8>> = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        match op.operator_name(buf) {
            Some(b"BDC" | b"BMC") => depth += 1,
            Some(b"EMC") => {
                depth -= 1;
                if depth == 0 && i + 1 != ops.len() {
                    return None;
                }
            }
            Some(b"Tf") => {
                if let Some(ContentTokenKind::Operand(Object::Name(n))) =
                    op.operands.first().map(|t| &t.kind)
                    && !font_names.iter().any(|f| f.as_slice() == n.as_bytes())
                {
                    font_names.push(n.as_bytes().to_vec());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let engine = match props.get(b"Engine") {
        Some(Object::String(s)) => Some(String::from_utf8_lossy(s).into_owned()),
        _ => None,
    };
    let version = props.get(b"Version").and_then(Object::as_int).unwrap_or(0);
    Some((engine, version, font_names))
}

/// What taking some layers off one page removes from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LayerStrip {
    /// Content streams to drop from the page's `/Contents`.
    pub(crate) contents: Vec<ObjId>,
    /// `/Font` names to drop from the page's resources: those the layers used
    /// and no remaining content stream of the page names in a `Tf`.
    pub(crate) font_names: Vec<Vec<u8>>,
}

impl LayerStrip {
    /// Nothing to remove.
    pub(crate) fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }
}

/// The strip that removes `layers` (all on `page`) from it.
///
/// A font name is kept when any content stream left on the page uses it, so a
/// name the page's own content shares with a layer is never orphaned. Streams
/// that cannot be decoded count as using every name: keeping an unused font
/// entry costs bytes, dropping a used one breaks the page.
pub(crate) fn plan_strip(
    view: &DocumentView<'_>,
    page: &Page,
    layers: &[OcrLayerRef],
) -> LayerStrip {
    let contents: Vec<ObjId> = layers.iter().map(|l| l.content).collect();
    let remaining: Vec<ObjId> = content_stream_ids(view, page)
        .into_iter()
        .filter(|id| !contents.contains(id))
        .collect();
    let mut used: Vec<Vec<u8>> = Vec::new();
    let mut opaque = false;
    for id in remaining {
        match tf_names(view, id) {
            Some(names) => used.extend(names),
            None => opaque = true,
        }
    }
    let mut font_names: Vec<Vec<u8>> = Vec::new();
    if !opaque {
        for name in layers.iter().flat_map(|l| &l.font_names) {
            if !used.contains(name) && !font_names.contains(name) {
                font_names.push(name.clone());
            }
        }
    }
    LayerStrip {
        contents,
        font_names,
    }
}

/// Every font name stream `id` selects with `Tf`, or `None` if it cannot be
/// read.
fn tf_names(view: &DocumentView<'_>, id: ObjId) -> Option<Vec<Vec<u8>>> {
    let Object::Stream(stream) = view.resolved(id) else {
        return None;
    };
    let raw = view.slice(stream.data_span)?;
    let decoded = crate::filters::decode_stream(&stream.dict, raw).ok()?;
    let cs = ContentStream::parse(decoded).ok()?;
    let buf = cs.buf.as_slice();
    let mut out: Vec<Vec<u8>> = Vec::new();
    for op in cs.operations() {
        if op.operator_name(buf) == Some(b"Tf")
            && let Some(ContentTokenKind::Operand(Object::Name(n))) =
                op.operands.first().map(|t| &t.kind)
        {
            out.push(n.as_bytes().to_vec());
        }
    }
    Some(out)
}

/// `contents` (a page's `/Contents` value) without the streams in `drop`.
///
/// Unchanged input is returned as-is, so a reference to a shared array stays a
/// reference. A changed array is written direct. `None` when nothing is left.
pub(crate) fn contents_without<G: ObjectGraph + ?Sized>(
    graph: &G,
    contents: Option<&Object>,
    drop: &[ObjId],
) -> Option<Object> {
    let contents = contents?;
    let dropped = |o: &Object| matches!(o, Object::Reference(id) if drop.contains(id));
    match graph.resolve(contents) {
        Object::Array(items) => {
            if !items.iter().any(dropped) {
                return Some(contents.clone());
            }
            let kept: Vec<Object> = items.iter().filter(|o| !dropped(o)).cloned().collect();
            (!kept.is_empty()).then_some(Object::Array(kept))
        }
        _ if dropped(contents) => None,
        _ => Some(contents.clone()),
    }
}
