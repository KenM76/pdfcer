//! Planning an XFA form fill: the XDP text a field's new value replaces.
//!
//! A field's value lives in the Data DOM node it is bound to (XFA 3.3 Ch.9
//! p.343: a value entered in a bound field is copied to its data node), so a
//! fill rewrites that node's text in the `datasets` packet and nothing else.
//! An unbound field gets a node created as re-normalization would
//! (pp.209-210) unless [`DataCreation::Refuse`].

use std::ops::Range;

use super::super::packets::{XdpDocument, XfaError, parse_xml};
use super::super::template::Template;
use super::data::{DATA_NS, ValueSpan, content_span, node_at, read_data};
use super::{DataCreation, MergeOptions, Unbound, merge_planned};
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
    /// No field or exclusion group at the template path took a data value,
    /// and none can be created for it: [`DataCreation::Refuse`], a
    /// container that does not bind by name, or a name that is not an XML
    /// element name.
    #[error("the container at that template path is not bound to a data value")]
    Unbound,
    /// Creating the data node needs a `datasets` packet, and the `/XFA`
    /// array has none to write into.
    #[error("the XFA packet array has no datasets packet to create data in")]
    NoDatasetsPacket,
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
    /// The data nodes created, outermost first (data groups, then the
    /// value); empty when the field was bound already. An inference to
    /// disclose: the file gains data the operator did not type.
    pub created: Vec<String>,
}

/// One replacement in the XDP text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextEdit {
    pub(crate) range: Range<usize>,
    pub(crate) text: String,
}

/// The edit that makes the container at template `path` hold `value`.
pub(crate) struct FillPlan {
    pub(crate) edit: TextEdit,
    /// See [`XfaFillOutcome::created`].
    pub(crate) created: Vec<String>,
    /// The edit adds a `datasets` packet.
    pub(crate) new_packet: bool,
}

/// Plan writing `value` for the container at template `path`.
pub(crate) fn plan_fill(
    xdp: &XdpDocument,
    template: &Template,
    path: &[usize],
    value: &str,
    opts: &MergeOptions,
) -> Result<FillPlan, XfaFillError> {
    let (merged, unbound) = merge_planned(template, xdp, opts)?;
    let doc = parse_xml(xdp.text())?;
    let root = read_data(&doc, opts.data_attributes)?;
    let escaped = escape(value);
    if let Some(b) = merged.bound.iter().find(|b| b.path == path) {
        let node = root
            .as_ref()
            .and_then(|r| node_at(r, &b.data))
            .ok_or(XfaFillError::Unbound)?;
        let edit = match &node.span {
            ValueSpan::Attribute(r) | ValueSpan::Content(r) => TextEdit {
                range: r.clone(),
                text: escaped,
            },
            span => insert(xdp.text(), span, &escaped).ok_or(XfaFillError::Unbound)?,
        };
        let plan = FillPlan {
            edit,
            created: Vec::new(),
            new_packet: false,
        };
        return Ok(plan);
    }
    if opts.data_creation == DataCreation::Refuse {
        return Err(XfaFillError::Unbound);
    }
    let u = unbound
        .iter()
        .find(|u| u.path == path)
        .ok_or(XfaFillError::Unbound)?;
    create(xdp.text(), &doc, root.as_ref(), u, &escaped)
}

/// The edit creating `u`'s data node, holding the escaped value.
fn create(
    text: &str,
    doc: &roxmltree::Document<'_>,
    root: Option<&super::data::DataNode>,
    u: &Unbound,
    escaped: &str,
) -> Result<FillPlan, XfaFillError> {
    let names: Vec<&str> = u
        .groups
        .iter()
        .map(String::as_str)
        .chain([u.name.as_str()])
        .collect();
    if !names.iter().all(|n| is_xml_name(n)) {
        return Err(XfaFillError::Unbound);
    }
    let mut markup = escaped.to_owned();
    for n in names.iter().rev() {
        markup = format!("<{n}>{markup}</{n}>");
    }
    let created = names.iter().map(|n| (*n).to_owned()).collect();
    let mut new_packet = false;
    let body = if let Some(root) = root {
        node_at(root, &u.scope).map(|n| n.body.clone())
    } else {
        let top = doc.root_element();
        let datasets = if top.tag_name().name() == "datasets" {
            Some(top)
        } else {
            top.children()
                .find(|n| n.is_element() && n.tag_name().name() == "datasets")
        };
        match datasets {
            Some(d) => {
                markup = format!("<xfa:data xmlns:xfa=\"{DATA_NS}\">{markup}</xfa:data>");
                Some(content_span(d))
            }
            None if top.tag_name().name() == "xdp" => {
                new_packet = true;
                markup = format!(
                    "<xfa:datasets xmlns:xfa=\"{DATA_NS}\"><xfa:data>{markup}</xfa:data></xfa:datasets>"
                );
                Some(content_span(top))
            }
            None => None,
        }
    };
    let edit = body
        .and_then(|b| insert(text, &b, &markup))
        .ok_or(XfaFillError::Unbound)?;
    Ok(FillPlan {
        edit,
        created,
        new_packet,
    })
}

/// The edit appending `markup` to an element's content: after its last
/// child, or by opening a self-closing element.
fn insert(text: &str, body: &ValueSpan, markup: &str) -> Option<TextEdit> {
    match body {
        ValueSpan::Content(r) => Some(TextEdit {
            range: r.end..r.end,
            text: markup.to_owned(),
        }),
        ValueSpan::Empty { element, qname } => {
            let open = text.get(element.clone())?;
            let open = open.strip_suffix("/>").unwrap_or(open).trim_end();
            Some(TextEdit {
                range: element.clone(),
                text: format!("{open}>{markup}</{qname}>"),
            })
        }
        ValueSpan::Attribute(_) | ValueSpan::None => None,
    }
}

/// Whether `name` can be written as an unprefixed XML element name.
fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
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
