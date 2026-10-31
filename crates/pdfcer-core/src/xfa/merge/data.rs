//! The Data DOM a merge reads: the `xfa:data` element of the `datasets`
//! packet as groups and values (XFA 3.3 Ch.4 pp.97-100, p.202;
//! `adobe_ext__xfa_som_data_binding.md` §7).

use std::ops::Range;

use super::super::packets::{MAX_XML_DEPTH, XfaError};
use super::DataAttributes;

/// The XFA data namespace (`xfa:data`, XFA 3.3 p.130).
pub(super) const DATA_NS: &str = "http://www.xfa.org/schema/xfa-data/1.0/";

/// Ceiling on data nodes read (a pdfcer guard; the spec sets none).
pub(super) const MAX_DATA_NODES: usize = 1_000_000;

/// One Data DOM node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DataNode {
    pub(super) name: String,
    /// The text of a value; empty for a group.
    pub(super) value: String,
    /// Loaded attributes first, then elements (p.100 Ex 3.20).
    pub(super) children: Vec<DataNode>,
    pub(super) group: bool,
    /// Where the value's text sits in the XDP text, for a fill to replace.
    pub(super) span: ValueSpan,
    /// The element's content, group or not, for a fill to append to.
    pub(super) body: ValueSpan,
}

/// The bytes of the XDP text a new value replaces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ValueSpan {
    /// A group, or a node not read from the text.
    #[default]
    None,
    /// An attribute's value, between its quotes.
    Attribute(Range<usize>),
    /// An element's content, between its start and end tags (empty when
    /// it has none: `<a></a>`).
    Content(Range<usize>),
    /// A self-closing element (`<a/>`), the whole element: a fill rewrites
    /// it as `<qname>value</qname>`.
    Empty {
        element: Range<usize>,
        qname: String,
    },
}

/// The `xfa:data` element of `doc` as the Data DOM root, or `None` when the
/// XDP has no `datasets` packet or it holds no `xfa:data`.
pub(super) fn read_data(
    doc: &roxmltree::Document<'_>,
    attributes: DataAttributes,
) -> Result<Option<DataNode>, XfaError> {
    let root = doc.root_element();
    let datasets = if root.tag_name().name() == "datasets" {
        Some(root)
    } else {
        root.children()
            .find(|n| n.is_element() && n.tag_name().name() == "datasets")
    };
    let Some(data) = datasets.and_then(|d| {
        d.children().find(|n| {
            n.is_element()
                && n.tag_name().name() == "data"
                && n.tag_name().namespace() == Some(DATA_NS)
        })
    }) else {
        return Ok(None);
    };
    let mut budget = MAX_DATA_NODES;
    let mut out = convert(data, 0, attributes, &mut budget)?;
    out.group = true;
    Ok(Some(out))
}

fn convert(
    node: roxmltree::Node<'_, '_>,
    depth: usize,
    attributes: DataAttributes,
    budget: &mut usize,
) -> Result<DataNode, XfaError> {
    if depth > MAX_XML_DEPTH {
        return Err(XfaError::TooDeep);
    }
    let mut out = DataNode {
        name: node.tag_name().name().to_owned(),
        value: String::new(),
        children: Vec::new(),
        group: false,
        span: content_span(node),
        body: content_span(node),
    };
    if attributes == DataAttributes::Load {
        // Namespaced attributes (`xfa:dataNode`, `xmlns`) direct the loader;
        // they are not data.
        for a in node.attributes().filter(|a| a.namespace().is_none()) {
            take(budget)?;
            out.children.push(DataNode {
                name: a.name().to_owned(),
                value: a.value().to_owned(),
                children: Vec::new(),
                group: false,
                span: ValueSpan::Attribute(a.range_value()),
                body: ValueSpan::None,
            });
        }
    }
    let mut elements = false;
    for child in node.children() {
        if child.is_element() {
            take(budget)?;
            elements = true;
            out.children
                .push(convert(child, depth + 1, attributes, budget)?);
        } else if let Some(t) = child.text().filter(|_| child.is_text()) {
            out.value.push_str(t);
        }
    }
    out.group = match node.attribute((DATA_NS, "dataNode")) {
        Some("dataGroup") => true,
        Some("dataValue") => false,
        _ => elements,
    };
    if out.group {
        out.value.clear();
        out.span = ValueSpan::None;
    }
    Ok(out)
}

/// Where `node`'s content lies, so a fill can replace or append to it: its
/// first to last child, an empty range before an empty element's end tag,
/// or the whole tag of a self-closing one.
pub(super) fn content_span(node: roxmltree::Node<'_, '_>) -> ValueSpan {
    let range = node.range();
    if let (Some(first), Some(last)) = (node.first_child(), node.last_child()) {
        return ValueSpan::Content(first.range().start..last.range().end);
    }
    let text = node
        .document()
        .input_text()
        .get(range.clone())
        .unwrap_or("");
    if text.ends_with("/>") {
        let qname = text
            .get(1..)
            .unwrap_or("")
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap_or("")
            .to_owned();
        return ValueSpan::Empty {
            element: range,
            qname,
        };
    }
    match text.rfind("</") {
        Some(at) => ValueSpan::Content(range.start + at..range.start + at),
        None => ValueSpan::None,
    }
}

fn take(budget: &mut usize) -> Result<(), XfaError> {
    *budget = budget.checked_sub(1).ok_or(XfaError::TooLarge)?;
    Ok(())
}

/// The node at `path` (child indices) below `root`.
pub(super) fn node_at<'a>(root: &'a DataNode, path: &[usize]) -> Option<&'a DataNode> {
    path.iter().try_fold(root, |node, &i| node.children.get(i))
}
