//! Keeping pdfcer OCR layers in their own `/Contents` streams when an edit
//! folds a multi-stream page into its first stream.
//!
//! An edit splices the page's concatenated content and writes the result to
//! `/Contents[0]`. The array's streams are one content stream divided at token
//! boundaries (ISO 32000-1 §7.8.2), so cutting the folded buffer back apart at
//! operator boundaries draws the same page. Cutting each whole layer section
//! back into the stream it came from keeps it a layer for [`super::marker`],
//! whose identity rule (a whole stream) is unchanged.

use super::marker::{is_layer_stream, layer_props};
use crate::content::ContentStream;
use crate::object::{ObjId, Object};
use crate::view::DocumentView;

/// How a folded buffer is written back across the page's `/Contents`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refold {
    /// The new payload of `/Contents[0]`.
    pub(crate) first: Vec<u8>,
    /// Every other entry whose payload changes, with its new payload (empty
    /// when emptied). An untouched layer stream is absent: it is not rewritten.
    pub(crate) others: Vec<(ObjId, Vec<u8>)>,
    /// Non-layer streams this refold empties.
    pub(crate) emptied: u64,
    /// Layer streams kept as their own entries.
    pub(crate) layers_kept: usize,
}

/// The refold of `folded` (the page's whole edited content) over `contents`,
/// or `None` when the page carries no pdfcer layer or the edited buffer does
/// not line up with the layers it had (one was deleted, or content now sits
/// between two layers with no stream of its own to hold it). `None` means the
/// caller folds everything into `/Contents[0]`.
pub(crate) fn refold(view: &DocumentView<'_>, contents: &[ObjId], folded: &[u8]) -> Option<Refold> {
    let layers: Vec<usize> = contents
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, id)| is_layer_stream(view, **id))
        .map(|(i, _)| i)
        .collect();
    if layers.is_empty() {
        return None;
    }
    let sections = layer_sections(folded)?;
    if sections.len() != layers.len() {
        return None;
    }
    let mut payload = vec![Vec::new(); contents.len()];
    let mut cursor = 0;
    let mut prev_layer = 0;
    for k in 0..=layers.len() {
        let section = sections.get(k).copied();
        let gap = folded.get(cursor..section.map_or(folded.len(), |s| s.0))?;
        // Content before the first layer stays in /Contents[0]; content after
        // a layer goes to the first non-layer stream that followed it.
        let owner = if k == 0 {
            Some(0)
        } else if trim(gap).is_empty() {
            None
        } else {
            let next = layers.get(k).copied().unwrap_or(contents.len());
            Some((prev_layer + 1..next).next()?)
        };
        if let Some(i) = owner {
            *payload.get_mut(i)? = if k == 0 { gap } else { trim(gap) }.to_vec();
        }
        if let (Some((s, e)), Some(&slot)) = (section, layers.get(k)) {
            *payload.get_mut(slot)? = folded.get(s..e)?.to_vec();
            cursor = e;
            prev_layer = slot;
        }
    }
    let mut payload = payload.into_iter();
    let mut out = Refold {
        first: payload.next()?,
        others: Vec::new(),
        emptied: 0,
        layers_kept: layers.len(),
    };
    for (&id, new) in contents.iter().skip(1).zip(payload) {
        if decoded(view, id).as_deref().map(trim) == Some(trim(&new)) {
            continue;
        }
        if new.is_empty() {
            out.emptied += 1;
        }
        out.others.push((id, new));
    }
    Some(out)
}

/// `[start, end)` of each top-level pdfcer layer section in `buf`, from its
/// `BDC`'s first operand to the end of the `EMC` that closes it. `None` when
/// the buffer does not parse or its marked content is unbalanced.
fn layer_sections(buf: &[u8]) -> Option<Vec<(usize, usize)>> {
    let cs = ContentStream::parse(buf.to_vec()).ok()?;
    let b = cs.buf.as_slice();
    let mut depth = 0_i64;
    let mut open: Option<usize> = None;
    let mut out = Vec::new();
    for op in cs.operations() {
        match op.operator_name(b) {
            Some(b"BDC" | b"BMC") => {
                if depth == 0 && layer_props(&op, b).is_some() {
                    open = Some(op.operands.first()?.span.start);
                }
                depth += 1;
            }
            Some(b"EMC") => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0
                    && let Some(start) = open.take()
                {
                    out.push((start, op.operator.span.start + op.operator.span.len));
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(out)
}

/// The current decoded payload of stream `id`.
pub(crate) fn decoded(view: &DocumentView<'_>, id: ObjId) -> Option<Vec<u8>> {
    let Object::Stream(stream) = view.graph().value(id)? else {
        return None;
    };
    let raw = view.slice(stream.data_span)?;
    crate::filters::decode_stream(&stream.dict, raw).ok()
}

fn trim(b: &[u8]) -> &[u8] {
    b.trim_ascii()
}

/// The disclosure for a refolded edit, replacing the plain fold's
/// "multi-stream page" line in `disclosures`.
pub(crate) fn restate(disclosures: &mut Vec<String>, r: &Refold) {
    disclosures.retain(|d| !d.starts_with("multi-stream page:"));
    let mut line = format!(
        "multi-stream page: {} pdfcer OCR layer stream(s) kept as their own /Contents entries",
        r.layers_kept
    );
    if r.emptied > 0 {
        line.push_str(&format!(
            "; {} other stream(s) collapsed into the first and emptied so the edit's byte \
             offsets stay coherent",
            r.emptied
        ));
    }
    line.push('.');
    disclosures.push(line);
}

#[cfg(test)]
// A test panicking on a bad index is the failure it reports.
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::layer_sections;

    #[test]
    fn sections_are_top_level_pdfcer_layers_only() {
        let buf = b"q 1 0 0 1 0 0 cm Q\n/pdfc_OCR <</Producer (pdfcer)>> BDC BT ET \
                    /Span <<>> BDC EMC EMC\n/pdfc_OCR <</Producer (other)>> BDC EMC";
        let s = layer_sections(buf).unwrap();
        assert_eq!(s.len(), 1);
        let (a, e) = s[0];
        assert!(buf[a..e].starts_with(b"/pdfc_OCR"));
        assert!(buf[a..e].ends_with(b"EMC EMC"));
    }

    #[test]
    fn unbalanced_marked_content_is_not_split() {
        assert!(layer_sections(b"EMC /pdfc_OCR <</Producer (pdfcer)>> BDC EMC").is_none());
        assert!(layer_sections(b"/pdfc_OCR <</Producer (pdfcer)>> BDC").is_none());
    }
}
