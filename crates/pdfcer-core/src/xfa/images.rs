//! The images an XFA form links by `href`, resolved only inside the file:
//! the catalog's `/Names /XFAImages` name tree (Adobe PDF Reference 1.7
//! Table 3.28, PDF 1.6; ISO 32000-1 §7.9.6 name trees) and nothing else.
//! An `href` is never followed to the filesystem or the network.

use std::collections::HashMap;

use crate::filters;
use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::pageops::references::name_tree_string_entries;
use crate::view::DocumentView;

/// Total decoded bytes [`XfaImages::from_view`] keeps (pdfcer policy,
/// `ARCHITECTURE.md` §10). Entries past it are left out, as if absent.
pub const MAX_XFA_IMAGE_BYTES: usize = 256 << 20;

/// Image bytes keyed by the exact `href` string a template names them by.
///
/// The bytes are the image file (JPEG, PNG, GIF, BMP or TIFF) with the
/// stream's own filters removed; the painter sniffs the format from them.
///
/// # Examples
///
/// ```
/// use pdfcer_core::xfa::XfaImages;
///
/// let mut images = XfaImages::new();
/// images.insert("logo.jpg", vec![0xFF, 0xD8, 0xFF]);
/// assert_eq!(images.get("logo.jpg"), Some(&[0xFF, 0xD8, 0xFF][..]));
/// assert!(images.get("./logo.jpg").is_none()); // exact match only
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XfaImages {
    entries: HashMap<Vec<u8>, Vec<u8>>,
    incomplete: bool,
}

impl XfaImages {
    /// No images: every `href` is unresolved.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The `/Names /XFAImages` tree of `view`'s catalog.
    ///
    /// Only string keys mapped to streams whose filters decode are kept; a
    /// name key, a non-stream value or an undecodable stream is skipped. The
    /// walk visits each `/Kids` node once and stops at the depth and node
    /// caps of [`crate::pageops::references`]; [`Self::is_complete`] says
    /// whether a cap (or [`MAX_XFA_IMAGE_BYTES`]) cut it short.
    #[must_use]
    pub fn from_view(view: &DocumentView<'_>) -> Self {
        let Some(tree) = view
            .catalog_dict()
            .and_then(|c| c.get(b"Names"))
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|n| n.get(b"XFAImages"))
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
        else {
            return Self::new();
        };
        let found = name_tree_string_entries(view, tree);
        let mut out = Self {
            entries: HashMap::new(),
            incomplete: found.is_none(),
        };
        let mut total = 0usize;
        let mut pairs: Vec<_> = found.unwrap_or_default().into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in pairs {
            let Object::Stream(stream) = view.resolve(&value) else {
                continue;
            };
            let Some(bytes) = view
                .slice(stream.data_span)
                .and_then(|raw| filters::decode_stream(&stream.dict, raw).ok())
            else {
                continue;
            };
            total = total.saturating_add(bytes.len());
            if total > MAX_XFA_IMAGE_BYTES {
                out.incomplete = true;
                break;
            }
            out.entries.insert(key, bytes);
        }
        out
    }

    /// Add or replace the image `href` names.
    pub fn insert(&mut self, href: impl Into<Vec<u8>>, bytes: Vec<u8>) {
        self.entries.insert(href.into(), bytes);
    }

    /// The bytes `href` names, matched exactly (no path normalisation).
    #[must_use]
    pub fn get(&self, href: &str) -> Option<&[u8]> {
        self.entries.get(href.as_bytes()).map(Vec::as_slice)
    }

    /// How many images are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `false` when a walk cap or [`MAX_XFA_IMAGE_BYTES`] left entries out.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        !self.incomplete
    }
}

/// RFC 4648 base64, skipping whitespace and stopping at `=`; `None` on any
/// other byte outside the alphabet or a dangling single sextet.
pub(crate) fn decode_base64(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut sextets = 0usize;
    for &c in text {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        sextets += 1;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).unwrap_or(0));
        }
    }
    (sextets % 4 != 1).then_some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::decode_base64;

    #[test]
    fn base64_decodes_padded_unpadded_and_wrapped_text() {
        assert_eq!(decode_base64(b"TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64(b"TWE=").unwrap(), b"Ma");
        assert_eq!(decode_base64(b"TQ==").unwrap(), b"M");
        assert_eq!(decode_base64(b"TW\n Fu\r\n").unwrap(), b"Man");
        assert_eq!(decode_base64(b"TWE").unwrap(), b"Ma");
        assert!(decode_base64(b"TWF*").is_none());
        assert!(decode_base64(b"TWFuT").is_none());
        assert_eq!(decode_base64(b"").unwrap(), b"");
    }
}
