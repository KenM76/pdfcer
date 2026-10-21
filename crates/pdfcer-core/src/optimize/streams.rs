//! The stream actions: what [`StreamAction`] does to one stream's bytes.

use super::{SkipReason, StreamAction};
use crate::filters::{self, flate};
use crate::object::{Dict, Name, ObjId, Object};

/// Filters [`StreamAction::Recompress`] and [`StreamAction::Brotli`] decode
/// and drop (ISO 32000-2 §7.4, Table 6, and EXTN-BROTLI-1). Each is generic
/// and lossless.
pub(crate) const GENERIC: [&[u8]; 6] = [
    b"FlateDecode",
    b"BrotliDecode",
    b"LZWDecode",
    b"ASCIIHexDecode",
    b"ASCII85Decode",
    b"RunLengthDecode",
];

/// One planned stream replacement, applied by
/// [`EditSession::apply_optimize`](crate::edit::EditSession::apply_optimize).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rewrite {
    pub(crate) id: ObjId,
    /// The session value when planned, for the stale-plan check.
    pub(crate) before: Object,
    /// What produced `dict` and `data`, redone on the stream as the run
    /// leaves it when an earlier step of the run rewrote it.
    pub(crate) action: StreamAction,
    /// The resample to redo instead of `action`, for a downsampled image.
    pub(crate) image: Option<super::image::pipeline::Job>,
    /// A `/Matte` pair's other half: neither is written without the other.
    pub(crate) partner: Option<ObjId>,
    pub(crate) dict: Dict,
    pub(crate) data: Vec<u8>,
}

/// The filter chain as names; an unreadable `/Filter` reads as one
/// `(invalid)` entry so no action treats it as unfiltered.
pub(crate) fn chain(dict: &Dict) -> Vec<String> {
    filters::filter_names(dict).map_or_else(
        |_| vec!["(invalid)".to_owned()],
        |names| {
            names
                .iter()
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .collect()
        },
    )
}

/// The role that keeps a stream out of every action, if any.
pub(crate) fn protected(dict: &Dict) -> Option<&'static str> {
    let type_name = match dict.get(b"Type") {
        Some(Object::Name(n)) => n.as_bytes(),
        _ => b"",
    };
    match type_name {
        b"Metadata" => Some("XMP metadata"),
        b"ObjStm" => Some("an object stream"),
        b"XRef" => Some("a cross-reference stream"),
        _ if dict.contains_key(b"F") => Some("an external-file stream"),
        _ => None,
    }
}

/// Apply `action` to a stream stored as `raw` under `dict`.
///
/// `Ok(None)` is [`StreamAction::Leave`], and [`StreamAction::Image`]: the
/// image path resolves those first (`image::pipeline`). A result that is not strictly
/// smaller than `raw` is refused as [`SkipReason::NotSmaller`].
pub(crate) fn apply(
    action: StreamAction,
    dict: &Dict,
    raw: &[u8],
) -> Result<Option<(Dict, Vec<u8>)>, SkipReason> {
    match action {
        StreamAction::Leave | StreamAction::Image(_) => Ok(None),
        StreamAction::Recompress { level } => reencode(dict, raw, b"FlateDecode", |d| {
            Ok(flate::encode_with_level(d, u32::from(level)))
        })
        .map(Some),
        StreamAction::Brotli { quality } => reencode(dict, raw, b"BrotliDecode", |d| {
            filters::brotli::encode(d, u32::from(quality))
                .map_err(|e| SkipReason::Undecodable(e.to_string()))
        })
        .map(Some),
    }
}

/// Decode the generic chain and re-encode with `encode` as one `filter`.
fn reencode(
    dict: &Dict,
    raw: &[u8],
    filter: &[u8],
    encode: impl FnOnce(&[u8]) -> Result<Vec<u8>, SkipReason>,
) -> Result<(Dict, Vec<u8>), SkipReason> {
    for name in chain(dict) {
        if !GENERIC.contains(&name.as_bytes()) {
            return Err(SkipReason::UnsupportedFilter(name));
        }
    }
    let decoded =
        filters::decode_stream(dict, raw).map_err(|e| SkipReason::Undecodable(e.to_string()))?;
    let data = encode(&decoded)?;
    if data.len() >= raw.len() {
        return Err(SkipReason::NotSmaller {
            before: raw.len() as u64,
            after: data.len() as u64,
        });
    }
    let mut out = dict.clone();
    out.remove(b"DecodeParms");
    out.remove(b"DP");
    out.insert(Name::from(b"Filter"), Object::Name(Name::from(filter)));
    out.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(data.len()).unwrap_or(i64::MAX)),
    );
    Ok((out, data))
}
