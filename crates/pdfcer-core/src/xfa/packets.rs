//! The XDP document behind `/AcroForm /XFA`, split into its packets
//! (ISO 32000-1 §12.7.8; XFA 3.3 Ch.23 pp.1036-1043).
//!
//! `/XFA` is one stream holding the whole XDP, or an array of alternating
//! `(name) stream` pairs whose streams, concatenated in array order, are the
//! XDP. A packet is a child element of `xdp:xdp`; its identity is that
//! element's local name, not the array's label string (no rule in ISO or XFA
//! covers a mismatch, so pdfcer trusts the content).

use std::ops::Range;

use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::view::DocumentView;

/// Largest XDP text accepted, summed over every packet stream.
pub const MAX_XDP_LEN: usize = 64 * 1024 * 1024;

/// Deepest element nesting accepted. roxmltree parses elements by recursive
/// descent, and a stack overflow is an abort no caller can catch, so depth
/// is measured on the raw text before parsing.
pub const MAX_XML_DEPTH: usize = 512;

/// Why an XFA layer's XDP could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum XfaError {
    /// `/XFA` is neither a stream nor an array of name/stream pairs, or an
    /// array entry that should be a stream is not one.
    #[error("/XFA is malformed: {0}")]
    Malformed(&'static str),
    /// A packet stream would not decode through its filters.
    #[error("XFA packet stream {index} would not decode")]
    Undecodable {
        /// Zero-based index of the stream among the packet streams.
        index: usize,
    },
    /// The XDP is larger than [`MAX_XDP_LEN`].
    #[error("XFA data exceeds {MAX_XDP_LEN} bytes")]
    TooLarge,
    /// The XDP bytes are not UTF-8 or BOM-marked UTF-16.
    #[error("XFA data is not UTF-8 or UTF-16 text")]
    NotText,
    /// Elements nest deeper than [`MAX_XML_DEPTH`].
    #[error("XFA data nests elements deeper than {MAX_XML_DEPTH}")]
    TooDeep,
    /// The XML is not well-formed, or carries a DTD (refused).
    #[error("XFA data is not well-formed XML: {0}")]
    Xml(String),
}

/// One packet: a child element of `xdp:xdp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdpPacket {
    /// The element's local name (`template`, `datasets`, `config`, ...).
    pub name: String,
    /// The element's namespace URI, when it has one. Version segments differ
    /// by edition (`.../xfa-template/3.3/`), so match on the prefix.
    pub namespace: Option<String>,
    /// Byte range of the whole element within [`XdpDocument::text`].
    pub range: Range<usize>,
}

/// A document's XDP text and the packets found in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdpDocument {
    text: String,
    packets: Vec<XdpPacket>,
}

impl XdpDocument {
    /// Split standalone XDP text (an `.xdp` file, or a packet stream's
    /// decoded text) into packets, as [`read_xdp`] does for a document.
    ///
    /// # Errors
    ///
    /// [`XfaError::TooLarge`] past [`MAX_XDP_LEN`], [`XfaError::TooDeep`],
    /// [`XfaError::Xml`].
    pub fn parse(text: String) -> Result<Self, XfaError> {
        if text.len() > MAX_XDP_LEN {
            return Err(XfaError::TooLarge);
        }
        parse_xdp(text)
    }

    /// The full XDP text, decoded to UTF-8.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The packets, in document order.
    #[must_use]
    pub fn packets(&self) -> &[XdpPacket] {
        &self.packets
    }

    /// The text of the first packet named `name` (the whole element).
    #[must_use]
    pub fn packet_text(&self, name: &str) -> Option<&str> {
        let p = self.packets.iter().find(|p| p.name == name)?;
        self.text.get(p.range.clone())
    }
}

/// Read the XDP behind `/AcroForm /XFA`.
///
/// `Ok(None)` when the document has no XFA layer. Every packet stream is
/// decoded through its filters; the total is capped at [`MAX_XDP_LEN`].
///
/// # Errors
///
/// [`XfaError`] when the layer exists but cannot be read.
pub fn read_xdp(view: &DocumentView<'_>) -> Result<Option<XdpDocument>, XfaError> {
    let Some(xfa) = xfa_entry(view) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    for (index, stream) in packet_streams(view, xfa)?.into_iter().enumerate() {
        let decoded = view
            .slice(stream.data_span)
            .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok())
            .ok_or(XfaError::Undecodable { index })?;
        if bytes.len() + decoded.len() > MAX_XDP_LEN {
            return Err(XfaError::TooLarge);
        }
        bytes.extend_from_slice(&decoded);
    }
    parse_xdp(text_of(&bytes)?).map(Some)
}

fn xfa_entry<'a>(view: &'a DocumentView<'_>) -> Option<&'a Object> {
    let acro = view
        .resolve(view.catalog_dict()?.get(b"AcroForm")?)
        .as_dict()?;
    Some(view.resolve(acro.get(b"XFA")?))
}

fn packet_streams<'a>(
    view: &'a DocumentView<'_>,
    xfa: &'a Object,
) -> Result<Vec<&'a crate::object::Stream>, XfaError> {
    match xfa {
        Object::Stream(s) => Ok(vec![s]),
        Object::Array(items) => items
            .iter()
            .skip(1)
            .step_by(2)
            .map(|o| match view.resolve(o) {
                Object::Stream(s) => Ok(s),
                _ => Err(XfaError::Malformed("a packet entry is not a stream")),
            })
            .collect(),
        _ => Err(XfaError::Malformed("neither a stream nor an array")),
    }
}

/// Decode XDP bytes: a UTF-16 BOM selects UTF-16, otherwise UTF-8 (a UTF-8
/// BOM is dropped).
fn text_of(bytes: &[u8]) -> Result<String, XfaError> {
    let utf16 = |be: bool, body: &[u8]| {
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| {
                let pair = [
                    c.first().copied().unwrap_or(0),
                    c.get(1).copied().unwrap_or(0),
                ];
                if be {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| XfaError::NotText)
    };
    match bytes {
        [0xFE, 0xFF, rest @ ..] => utf16(true, rest),
        [0xFF, 0xFE, rest @ ..] => utf16(false, rest),
        [0xEF, 0xBB, 0xBF, rest @ ..] => {
            String::from_utf8(rest.to_vec()).map_err(|_| XfaError::NotText)
        }
        _ => String::from_utf8(bytes.to_vec()).map_err(|_| XfaError::NotText),
    }
}

/// Split XDP text into packets. A document whose root is not `xdp` is taken
/// as a single packet (the root element itself).
pub(crate) fn parse_xdp(text: String) -> Result<XdpDocument, XfaError> {
    if nesting_depth(&text) > MAX_XML_DEPTH {
        return Err(XfaError::TooDeep);
    }
    let packets = {
        let doc = parse_xml(&text)?;
        let root = doc.root_element();
        let nodes: Vec<roxmltree::Node<'_, '_>> = if root.tag_name().name() == "xdp" {
            root.children()
                .filter(roxmltree::Node::is_element)
                .collect()
        } else {
            vec![root]
        };
        nodes
            .into_iter()
            .map(|n| XdpPacket {
                name: n.tag_name().name().to_owned(),
                namespace: n.tag_name().namespace().map(str::to_owned),
                range: n.range(),
            })
            .collect()
    };
    Ok(XdpDocument { text, packets })
}

/// Parse with DTDs refused. Callers check [`nesting_depth`] first.
pub(crate) fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, XfaError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: 8_000_000,
    };
    roxmltree::Document::parse_with_options(text, options).map_err(|e| XfaError::Xml(e.to_string()))
}

/// The deepest element nesting in `text`, without parsing it.
///
/// Skips comments, CDATA, processing instructions, declarations and quoted
/// attribute values; a self-closing tag does not stay open. Malformed input
/// can only make it over-count, which errs towards refusing.
pub(crate) fn nesting_depth(text: &str) -> usize {
    let b = text.as_bytes();
    let skip_past = |from: usize, end: &[u8]| -> usize {
        b.get(from..)
            .and_then(|rest| rest.windows(end.len()).position(|w| w == end))
            .map_or(b.len(), |p| from + p + end.len())
    };
    let (mut i, mut depth, mut max) = (0usize, 0usize, 0usize);
    while let Some(rest) = b.get(i..).filter(|r| !r.is_empty()) {
        if rest.first() != Some(&b'<') {
            i += 1;
        } else if rest.starts_with(b"<!--") {
            i = skip_past(i + 4, b"-->");
        } else if rest.starts_with(b"<![CDATA[") {
            i = skip_past(i + 9, b"]]>");
        } else if rest.starts_with(b"<?") {
            i = skip_past(i + 2, b"?>");
        } else if rest.starts_with(b"<!") {
            i = skip_past(i + 2, b">");
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            i = skip_past(i + 2, b">");
        } else {
            let (end, self_closing) = tag_end(b, i + 1);
            if !self_closing {
                depth += 1;
                max = max.max(depth);
            }
            i = end;
        }
    }
    max
}

/// Index just past the `>` closing the start tag at `from`, and whether it
/// was `/>`. Quoted attribute values may contain `>`.
fn tag_end(b: &[u8], from: usize) -> (usize, bool) {
    let mut quote: Option<u8> = None;
    let mut i = from;
    while let Some(&c) = b.get(i) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => return (i + 1, i > from && b.get(i - 1) == Some(&b'/')),
            None => {}
        }
        i += 1;
    }
    (b.len(), false)
}

#[cfg(test)]
mod tests;
