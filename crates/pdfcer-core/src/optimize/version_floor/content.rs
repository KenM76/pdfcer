//! Constructs only a stream's decoded data shows: inline images and
//! marked-content properties in content streams (§8.9.7, §14.6), and the
//! OpenType wrapper of a font program (§9.9).

use super::keys;
use super::scan::{Notes, lossy};
use crate::content::{ContentStream, ContentTokenKind};
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};

/// The decoded data of stream `id`, or `None` when it cannot be read.
pub(super) fn decoded(doc: &Document, id: ObjId) -> Option<Vec<u8>> {
    let Object::Stream(stream) = &doc.get(id)?.value else {
        return None;
    };
    let raw = doc.view().slice(stream.data_span)?;
    crate::filters::decode_stream(&stream.dict, raw).ok()
}

/// Scan content stream `data`; `false` when it does not tokenize.
pub(super) fn scan_content(data: Vec<u8>, notes: &mut Notes) -> bool {
    let Ok(stream) = ContentStream::parse(data) else {
        return false;
    };
    for op in stream.operations() {
        if let ContentTokenKind::InlineImage { params, .. } = &op.operator.kind {
            inline_image(params, notes);
            continue;
        }
        if !matches!(
            op.operator_name(&stream.buf),
            Some(b"BDC" | b"DP" | b"BMC" | b"MP")
        ) {
            continue;
        }
        let operand = |i: usize| match op.operands.get(i).map(|t| &t.kind) {
            Some(ContentTokenKind::Operand(o)) => Some(o),
            _ => None,
        };
        let tag = operand(0).and_then(Object::as_name).map(|n| n.as_bytes());
        marked_content(tag, operand(1).and_then(Object::as_dict), notes);
    }
    true
}

/// §8.9.7 Table 92: `/L` (and its long form) is PDF 2.0; 16-bit samples
/// are PDF 1.5.
fn inline_image(params: &Dict, notes: &mut Notes) {
    if params.get(b"L").is_some() || params.get(b"Length").is_some() {
        notes.insert((20, "inline image /L".to_owned()));
    }
    if params.get(b"BitsPerComponent").and_then(Object::as_int) == Some(16) {
        notes.insert((15, "16-bit image samples".to_owned()));
    }
    let names: Vec<&[u8]> = match params.get(b"Filter") {
        Some(Object::Name(n)) => vec![n.as_bytes()],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(Object::as_name)
            .map(|n| n.as_bytes())
            .collect(),
        _ => Vec::new(),
    };
    for n in names {
        if let Some(ver) = keys::filter(n) {
            notes.insert((ver, format!("/Filter /{}", lossy(n))));
        }
    }
}

/// §14.6 and §14.9: the optional-content tag is PDF 1.5, `ReversedChars`
/// 1.4, `TagSuspect` 1.6; inline properties `ActualText` (1.4 on `Span`,
/// else 1.5), `Alt`, `E`, `Lang` (1.5) and `AF`, `MCAF`, `Artifact` (2.0).
fn marked_content(tag: Option<&[u8]>, props: Option<&Dict>, notes: &mut Notes) {
    let tag_ver = match tag {
        Some(b"OC") => Some(15),
        Some(b"ReversedChars") => Some(14),
        Some(b"TagSuspect") => Some(16),
        _ => None,
    };
    if let (Some(ver), Some(t)) = (tag_ver, tag) {
        notes.insert((ver, format!("marked-content tag /{}", lossy(t))));
    }
    let Some(props) = props else { return };
    for (k, _) in props.iter() {
        let ver = match k.as_bytes() {
            b"ActualText" if tag == Some(b"Span") => 14,
            b"ActualText" | b"Alt" | b"E" | b"Lang" => 15,
            b"AF" | b"MCAF" | b"Artifact" => 20,
            _ => continue,
        };
        notes.insert((ver, format!("marked-content /{}", lossy(k.as_bytes()))));
    }
}

/// §9.9 Table 124: an OpenType font program is PDF 1.6.
pub(super) fn scan_font(data: &[u8], notes: &mut Notes) {
    if data.starts_with(b"OTTO") {
        notes.insert((16, "OpenType font program".to_owned()));
    }
}
