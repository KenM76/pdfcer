//! Hand signatures: page content tagged as the mark a person drew or typed
//! for a signature field, findable again after save and reopen.
//!
//! # The tag
//!
//! The content-authoring verbs ([`EditSession::add_markup_as_content`],
//! [`EditSession::add_text`], [`EditSession::add_image`]) wrap what they write
//! in one marked-content sequence (ISO 32000-1 §14.6, Table 320) when asked:
//!
//! ```text
//! /pdfc_HandSig <</Field (fully.qualified.name)>> BDC
//! … the added content …
//! EMC
//! ```
//!
//! The property list is inline: every value is direct (§14.6.2). The tag is a
//! second-class name in the `pdfc_` family the OCR layer marker already uses
//! (Annex E; `crate::ocr::marker`). `/Field` is a text string (§7.9.2.2). A
//! reader that does not know the tag ignores it, so the mark renders and
//! prints as ordinary content.
//!
//! The mark is content, never a signature: no `/Sig` field, `/V` or signature
//! dictionary is read or written, and a later certificate signature of the
//! same field is unaffected.
//!
//! # What counts as present
//!
//! [`hand_signatures`] scans the page's whole `/Contents` (one stream for
//! marked-content purposes, §14.6.1 NOTE 4), not stream by stream, so a mark
//! survives a later edit that folds the page's streams together. A sequence
//! is reported only while it still **paints** something: deleting the mark's
//! objects leaves an empty `BDC … EMC` behind, and that box is unsigned again.
//!
//! [`EditSession::add_markup_as_content`]: crate::edit::EditSession::add_markup_as_content
//! [`EditSession::add_text`]: crate::edit::EditSession::add_text
//! [`EditSession::add_image`]: crate::edit::EditSession::add_image

use crate::content::{ContentError, ContentStream, ContentTokenKind};
use crate::object::Object;
use crate::page_tree::{Page, Rect};
use crate::vector::{Bounds, Matrix, PageObjects};
use crate::view::DocumentView;

/// The marked-content tag around a hand signature.
pub const HAND_SIGNATURE_TAG: &[u8] = b"pdfc_HandSig";

/// The property-list key naming the signature field.
pub const HAND_SIGNATURE_FIELD_KEY: &[u8] = b"Field";

/// One hand signature found on a page.
///
/// Valid for the revision it was found in; find again after any edit.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct HandSignatureMark {
    /// The field name the mark was written for, decoded from `/Field`.
    pub field: String,
    /// The page-space extent of what the sequence paints (a conservative
    /// superset, as [`crate::vector::VectorObject::page_bbox`]).
    pub bounds: Rect,
    /// The indices of the objects the sequence encloses, ascending: into
    /// `EditSession::page_objects(page)` when found by
    /// [`crate::edit::EditSession::hand_signatures`], or into
    /// `decompose_page(view, page, Matrix::IDENTITY).objects` when found by
    /// [`hand_signatures`]. Pass them to `transform_objects` or
    /// `delete_objects` to act on exactly the mark. Never empty.
    pub objects: Vec<usize>,
}

/// Why a hand-signature mark was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HandSignatureError {
    /// The field name is empty; a mark must name the field it signs.
    #[error("a hand signature must name its field; the field name is empty")]
    EmptyFieldName,
    /// The verb wrote no page content of its own to wrap, so nothing would
    /// carry the mark. Nothing was added.
    #[error("the verb added no page content to mark as a hand signature")]
    NothingToMark,
    /// The verb authors an annotation, not page content; a hand signature
    /// is page content by definition.
    #[error("a hand signature is page content; the annotation route cannot carry one")]
    NotPageContent,
}

/// Refuse a field name no mark can carry.
///
/// # Errors
///
/// [`HandSignatureError::EmptyFieldName`].
pub fn validate_field_name(field: &str) -> Result<(), HandSignatureError> {
    if field.is_empty() {
        return Err(HandSignatureError::EmptyFieldName);
    }
    Ok(())
}

/// The `BDC` line that opens a hand-signature sequence for `field`.
pub(crate) fn open_sequence(field: &str) -> Vec<u8> {
    let mut props = crate::object::Dict::new();
    props.insert(
        crate::object::Name(HAND_SIGNATURE_FIELD_KEY.to_vec()),
        Object::String(crate::textstring::encode_text_string(field)),
    );
    let mut out = b"/".to_vec();
    out.extend_from_slice(HAND_SIGNATURE_TAG);
    out.push(b' ');
    crate::writer::serialize::write_object(
        &mut out,
        &Object::Dict(props),
        crate::object::ObjId::new(0, 0),
        &[],
        &crate::writer::IdentityEncoder,
    );
    out.extend_from_slice(b" BDC\n");
    out
}

/// The hand signatures still painting on `page`, in content order.
///
/// `view` carries its usual meaning: `&doc.view()` reads the file as loaded,
/// `&session.view()` the edited state. In a session prefer
/// [`EditSession::hand_signatures`](crate::edit::EditSession::hand_signatures),
/// which reuses the session's page model.
///
/// # Errors
///
/// [`ContentError`] when the page's content cannot be decoded or parsed.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::hand_sig::hand_signatures;
/// use pdfcer_core::page_tree::pages;
///
/// let bytes = std::fs::read(concat!(
///     env!("CARGO_MANIFEST_DIR"),
///     "/../../fixtures/synthetic/hello.pdf"
/// ))?;
/// let doc = Document::from_bytes(bytes)?;
/// let page = &pages(&doc)?[0];
/// assert!(hand_signatures(&doc.view(), page)?.is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn hand_signatures(
    view: &DocumentView<'_>,
    page: &Page,
) -> Result<Vec<HandSignatureMark>, ContentError> {
    let cs = ContentStream::from_page(view, page)?;
    if sequences(&cs).is_empty() {
        return Ok(Vec::new());
    }
    let objects = crate::vector::decompose_page(view, page, Matrix::IDENTITY)?;
    Ok(marks_in(&cs, &objects))
}

/// The marks in `cs`, bounded by the objects of `objects` (decomposed from
/// the same buffer) that each sequence encloses.
pub(crate) fn marks_in(cs: &ContentStream, objects: &PageObjects) -> Vec<HandSignatureMark> {
    sequences(cs)
        .into_iter()
        .filter_map(|(field, start, end)| {
            let enclosed: Vec<usize> = objects
                .objects
                .iter()
                .enumerate()
                .filter(|(_, o)| {
                    let span = o.bytes();
                    span.start >= start && span.start + span.len <= end
                })
                .map(|(i, _)| i)
                .collect();
            let bounds = enclosed
                .iter()
                .filter_map(|&i| objects.objects.get(i))
                .fold(Bounds::EMPTY, |acc, o| acc.union(o.page_bbox()));
            (!bounds.is_empty()).then_some(HandSignatureMark {
                field,
                bounds: Rect {
                    llx: bounds.min.x,
                    lly: bounds.min.y,
                    urx: bounds.max.x,
                    ury: bounds.max.y,
                },
                objects: enclosed,
            })
        })
        .collect()
}

/// Every balanced hand-signature sequence in `cs`: `(field, start, end)`,
/// the byte range from its `BDC` to the end of its `EMC`. An unbalanced
/// sequence is not one.
fn sequences(cs: &ContentStream) -> Vec<(String, usize, usize)> {
    let buf = cs.buf.as_slice();
    let mut open: Vec<Option<(String, usize)>> = Vec::new();
    let mut found = Vec::new();
    for op in cs.operations() {
        match op.operator_name(buf) {
            Some(b"BDC") => {
                let start = op
                    .operands
                    .first()
                    .map_or(op.operator.span.start, |t| t.span.start);
                open.push(field_of(op.operands).map(|f| (f, start)));
            }
            Some(b"BMC") => open.push(None),
            Some(b"EMC") => {
                if let Some(Some((field, start))) = open.pop() {
                    let end = op.operator.span.start + op.operator.span.len;
                    found.push((field, start, end));
                }
            }
            _ => {}
        }
    }
    found.sort_by_key(|(_, start, _)| *start);
    found
}

/// The `/Field` of a `BDC`'s operands when its tag is the hand-signature tag
/// and its property list is inline.
fn field_of(operands: &[crate::content::ContentToken]) -> Option<String> {
    let operand = |i: usize| match operands.get(i).map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(o)) => Some(o),
        _ => None,
    };
    if operand(0)?.as_name()?.as_bytes() != HAND_SIGNATURE_TAG {
        return None;
    }
    match operand(1)?.as_dict()?.get(HAND_SIGNATURE_FIELD_KEY) {
        Some(Object::String(s)) => Some(crate::textstring::decode_text_string(s).text),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test assertions
mod tests {
    use super::*;
    use crate::vector::{NoFonts, NoXObjects, decompose_with_fonts};

    fn marks(content: impl AsRef<[u8]>) -> Vec<HandSignatureMark> {
        let cs = ContentStream::parse(content.as_ref().to_vec()).expect("parses");
        let objects = decompose_with_fonts(&cs, Matrix::IDENTITY, &NoXObjects, &NoFonts);
        marks_in(&cs, &objects)
    }

    #[test]
    fn open_sequence_round_trips_through_the_reader() {
        let mut content = open_sequence("sig.one (a)");
        content.extend_from_slice(b"0 0 m 10 20 l S\nEMC\n");
        let found = marks(&content);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].field, "sig.one (a)");
        assert_eq!(
            found[0].bounds,
            Rect {
                llx: 0.0,
                lly: 0.0,
                urx: 10.0,
                ury: 20.0
            }
        );
    }

    #[test]
    fn a_non_ascii_field_name_survives() {
        let mut content = open_sequence("Unterschrift.Müller");
        content.extend_from_slice(b"0 0 m 1 1 l S\nEMC\n");
        let found = marks(&content);
        assert_eq!(found[0].field, "Unterschrift.Müller");
    }

    #[test]
    fn an_empty_sequence_is_not_a_signature() {
        assert!(marks("/pdfc_HandSig <</Field (f)>> BDC EMC 0 0 m 5 5 l S").is_empty());
    }

    #[test]
    fn content_outside_the_sequence_does_not_widen_it() {
        let found = marks("0 0 m 100 100 l S /pdfc_HandSig <</Field (f)>> BDC 1 1 m 2 2 l S EMC");
        assert_eq!(found[0].bounds.urx, 2.0);
    }

    #[test]
    fn nested_sequences_and_other_tags_are_told_apart() {
        let found = marks(
            "/OC /OC1 BDC /pdfc_HandSig <</Field (a)>> BDC /Span <</MCID 0>> BDC \
             1 1 m 2 2 l S EMC EMC EMC /pdfc_OCR <</Field (b)>> BDC 3 3 m 4 4 l S EMC",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].field, "a");
    }

    #[test]
    fn an_unclosed_or_fieldless_sequence_is_not_reported() {
        assert!(marks("/pdfc_HandSig <</Field (a)>> BDC 1 1 m 2 2 l S").is_empty());
        assert!(marks("/pdfc_HandSig <</Other (a)>> BDC 1 1 m 2 2 l S EMC").is_empty());
        assert!(marks("/pdfc_HandSig /P0 BDC 1 1 m 2 2 l S EMC").is_empty());
    }

    #[test]
    fn an_empty_field_name_is_refused() {
        assert_eq!(
            validate_field_name(""),
            Err(HandSignatureError::EmptyFieldName)
        );
        assert_eq!(validate_field_name("f"), Ok(()));
    }
}
