//! Planning an XFA form fill: the XDP text a field's new value replaces.
//!
//! A field's value lives in the Data DOM node it is bound to (XFA 3.3 Ch.9
//! p.343: a value entered in a bound field is copied to its data node), so a
//! fill rewrites that node's text in the `datasets` packet and nothing else.

use std::ops::Range;

use super::super::packets::{XdpDocument, XfaError, parse_xml};
use super::data::{ValueSpan, node_at, read_data};
use super::{MergeOptions, MergedForm};
use crate::object::ObjId;

/// Why a value cannot be written into an XFA form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum XfaFillError {
    /// The document has no `/AcroForm /XFA`, or its XDP has no template.
    #[error("the document has no XFA form")]
    NoXfa,
    /// The XDP could not be read.
    #[error(transparent)]
    Xfa(#[from] XfaError),
    /// No field or exclusion group at the template path took a data value.
    /// Writing a data node that does not exist yet is not supported.
    #[error("the container at that template path is not bound to a data value")]
    Unbound,
    /// The XDP is UTF-16, whose text offsets are not stream bytes; pdfcer
    /// rewrites UTF-8 XDP only.
    #[error("the XDP is UTF-16, which pdfcer does not rewrite")]
    Utf16,
    /// The data value's text straddles two `/XFA` packet streams.
    #[error("the bound data value spans two packet streams")]
    SplitAcrossStreams,
    /// The packet stream holding the value is not an indirect object.
    #[error("the packet stream holding the value is not an indirect object")]
    DirectStream,
}

/// What [`crate::edit::EditSession::fill_xfa_field`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct XfaFillOutcome {
    /// The packet stream rewritten.
    pub stream: ObjId,
    /// The data node's path, as in [`super::BoundValue::data`].
    pub data: Vec<usize>,
    /// Other bound containers that share the data node and so show the new
    /// value too (global binding, a shared `dataRef`).
    pub shared_with: Vec<Vec<usize>>,
}

/// One replacement in the XDP text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextEdit {
    pub(crate) range: Range<usize>,
    pub(crate) text: String,
}

/// The edit that makes the container at template `path` hold `value`, plus
/// the data node it writes.
pub(crate) fn plan_fill(
    xdp: &XdpDocument,
    merged: &MergedForm,
    path: &[usize],
    value: &str,
    opts: &MergeOptions,
) -> Result<(TextEdit, Vec<usize>), XfaFillError> {
    let at = merged
        .bound
        .iter()
        .find(|b| b.path == path)
        .map(|b| b.data.clone())
        .ok_or(XfaFillError::Unbound)?;
    let doc = parse_xml(xdp.text())?;
    let root = read_data(&doc, opts.data_attributes)?.ok_or(XfaFillError::Unbound)?;
    let node = node_at(&root, &at).ok_or(XfaFillError::Unbound)?;
    let escaped = escape(value);
    let edit = match &node.span {
        ValueSpan::None => return Err(XfaFillError::Unbound),
        ValueSpan::Attribute(r) | ValueSpan::Content(r) => TextEdit {
            range: r.clone(),
            text: escaped,
        },
        ValueSpan::Empty { element, qname } => {
            let text = xdp.text().get(element.clone()).unwrap_or("");
            let open = text.strip_suffix("/>").unwrap_or(text).trim_end();
            TextEdit {
                range: element.clone(),
                text: format!("{open}>{escaped}</{qname}>"),
            }
        }
    };
    Ok((edit, at))
}

/// XML-escape for both element content and a quoted attribute value.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\r' => out.push_str("&#13;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
    out
}
