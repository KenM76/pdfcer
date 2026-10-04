//! The content fingerprint an export records of the state it was taken from,
//! so [`super::import`] can tell a stale export from a current one (G112).
//!
//! It hashes what the document *says*, not how its bytes are laid out: the
//! same objects hash the same whether they sit in an object stream or not,
//! compressed or not, in a session or saved and reopened. So a no-op save, a
//! re-compression or a full rewrite does not make an export stale; any change
//! to an object's value does.
//!
//! Recorded as a comment line in the export's header, not a dictionary key:
//! ISO 32000-1 Annex E.1 forbids private keys in the trailer, and a key in
//! the catalog or `/Info` would itself show up as an edit at import.

use super::{ENCODING_KEYS, EditableSource};
use crate::document::Document;
use crate::object::{Dict, Object, Stream};
use sha2::{Digest, Sha256};

/// A SHA-256 content fingerprint ([`fingerprint`]).
pub type Fingerprint = [u8; 32];

/// The header comment that carries the fingerprint, followed by
/// `sha256:` and 64 lowercase hex digits.
pub(super) const MARKER: &[u8] = b"%PdfcerExportBase sha256:";

/// How far into an export the marker is looked for: the header comments, well
/// before the first object.
const MARKER_WINDOW: usize = 4096;

/// Nesting beyond which a value is hashed as a single tag. The parser bounds
/// nesting far below this; the guard is for authored values (§10).
const MAX_DEPTH: usize = 512;

/// The content fingerprint of `src`.
///
/// SHA-256 over every live object in ascending id order: its number, its
/// generation and its value, with dictionary keys sorted (§7.3.7: entry order
/// is not significant), every stream's payload decoded and its `/Filter`,
/// `/DecodeParms` and `/Length` ignored. Cross-reference and object streams
/// (`/Type /XRef`, `/ObjStm`) are skipped: they are file layout, and a save
/// writes new ones. A stream that will not decode is hashed as stored. The
/// trailer is not hashed.
#[must_use]
pub fn fingerprint<S: EditableSource + ?Sized>(src: &S) -> Fingerprint {
    let mut h = Sha256::new();
    for id in src.object_ids() {
        let Some(value) = src.object(id) else {
            continue;
        };
        if matches!(value, Object::Stream(s) if is_layout(&s.dict)) {
            continue;
        }
        h.update(id.num.to_be_bytes());
        h.update(id.generation.to_be_bytes());
        feed(&mut h, src, value, 0);
    }
    h.finalize().into()
}

/// The fingerprint `edited` records of the state it was exported from, or
/// `None` when it carries none (an export older than the record, or one whose
/// marker line was deleted).
#[must_use]
pub fn recorded_base(edited: &Document) -> Option<Fingerprint> {
    let bytes = edited.bytes();
    let window = bytes.get(..bytes.len().min(MARKER_WINDOW))?;
    let at = window.windows(MARKER.len()).position(|w| w == MARKER)?;
    let hex = bytes.get(at + MARKER.len()..at + MARKER.len() + 64)?;
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(hex.chunks_exact(2)) {
        let text = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(text, 16).ok()?;
    }
    Some(out)
}

/// The marker line for `print`, newline-terminated.
pub(super) fn marker_line(print: &Fingerprint) -> Vec<u8> {
    let mut line = MARKER.to_vec();
    for b in print {
        line.extend_from_slice(format!("{b:02x}").as_bytes());
    }
    line.push(b'\n');
    line
}

fn is_layout(dict: &Dict) -> bool {
    matches!(
        dict.get(b"Type"),
        Some(Object::Name(n)) if n.0 == b"XRef" || n.0 == b"ObjStm"
    )
}

fn feed_len(h: &mut Sha256, n: usize) {
    h.update(u64::try_from(n).unwrap_or(u64::MAX).to_be_bytes());
}

fn feed_bytes(h: &mut Sha256, tag: u8, bytes: &[u8]) {
    h.update([tag]);
    feed_len(h, bytes.len());
    h.update(bytes);
}

fn feed<S: EditableSource + ?Sized>(h: &mut Sha256, src: &S, value: &Object, depth: usize) {
    if depth > MAX_DEPTH {
        h.update([0xff]);
        return;
    }
    match value {
        Object::Null => h.update([0]),
        Object::Boolean(b) => h.update([1, u8::from(*b)]),
        Object::Integer(i) => {
            h.update([2]);
            h.update(i.to_be_bytes());
        }
        Object::Real(r) => {
            h.update([3]);
            h.update(r.to_bits().to_be_bytes());
        }
        Object::String(s) => feed_bytes(h, 4, s),
        Object::Name(n) => feed_bytes(h, 5, &n.0),
        Object::Array(items) => {
            h.update([6]);
            feed_len(h, items.len());
            for item in items {
                feed(h, src, item, depth + 1);
            }
        }
        Object::Dict(d) => feed_dict(h, src, d, &[], depth),
        Object::Stream(s) => feed_stream(h, src, s, depth),
        Object::Reference(id) => {
            h.update([8]);
            h.update(id.num.to_be_bytes());
            h.update(id.generation.to_be_bytes());
        }
    }
}

fn feed_dict<S: EditableSource + ?Sized>(
    h: &mut Sha256,
    src: &S,
    dict: &Dict,
    skip: &[&[u8]],
    depth: usize,
) {
    let mut entries: Vec<_> = dict
        .iter()
        .filter(|(k, _)| !skip.contains(&k.0.as_slice()))
        .collect();
    entries.sort_unstable_by(|a, b| a.0.0.cmp(&b.0.0));
    h.update([7]);
    feed_len(h, entries.len());
    for (k, v) in entries {
        feed_bytes(h, 5, &k.0);
        feed(h, src, v, depth + 1);
    }
}

fn feed_stream<S: EditableSource + ?Sized>(h: &mut Sha256, src: &S, s: &Stream, depth: usize) {
    h.update([9]);
    feed_dict(h, src, &s.dict, &ENCODING_KEYS, depth);
    match src.stream_bytes(s) {
        Some(raw) => match crate::filters::decode_stream(&s.dict, raw) {
            Ok(data) => feed_bytes(h, 10, &data),
            Err(_) => feed_bytes(h, 11, raw),
        },
        None => h.update([12]),
    }
}
