//! Rewriting one property of an XMP packet in place, so `/Info` and the
//! catalog's `/Metadata` say the same thing (ISO 32000-1 §14.3.2; spec RAG
//! `xmp__ref__info_to_xmp_mapping.md`).
//!
//! Only the property's own bytes change. The rest of the packet, the
//! `xpacket` wrapper and its padding are kept byte for byte (`XP-A2`).
//! Every walk here is an iterator over the parsed tree, never recursion,
//! so a deeply nested hostile packet costs nodes, not stack (§10).

use std::ops::Range;

use roxmltree::{Attribute, Document, Node, ParsingOptions};

use super::info_entries::{AltLanguages, AuthorItems};

/// The RDF namespace (ISO 16684-1 §7.3).
const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// The XML namespace, which `xml:lang` lives in.
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// The PDF/A identification schema (`pdfaid:part`, ISO 19005-1 §6.7.11).
const PDFAID_NS: &str = "http://www.aiim.org/pdfa/ns/id/";
/// The largest decoded packet this module parses. A real packet is a few
/// KiB; anything bigger is reported unreadable rather than parsed.
pub(super) const MAX_PACKET_BYTES: usize = 8 << 20;
/// `roxmltree`'s node ceiling for a packet.
const MAX_NODES: u32 = 1_000_000;
/// The UTF-8 byte-order mark a packet may start with, kept on rewrite.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// How a property's value is structured (Part 3 Table 20 value types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// A simple text value.
    Simple,
    /// A Language Alternative (`rdf:Alt` of `xml:lang` items).
    LangAlt,
    /// An ordered array (`rdf:Seq`).
    Seq,
}

/// The XMP property an `/Info` key maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Property {
    pub(super) ns: &'static str,
    /// The prefix used when the namespace is not already bound.
    pub(super) prefix: &'static str,
    /// The local name, already escaped to an XML NCName.
    pub(super) local: String,
    pub(super) shape: Shape,
}

impl Property {
    /// The property as `prefix:local`, for disclosure.
    pub(super) fn qualified(&self) -> String {
        format!("{}:{}", self.prefix, self.local)
    }
}

/// How the rewrite treats the parts of the value it does not set.
#[derive(Debug, Clone, Copy)]
pub(super) struct Policy {
    pub(super) languages: AltLanguages,
    pub(super) authors: AuthorItems,
}

/// What [`rewrite`] concluded.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Change {
    /// The packet already says this; nothing to write.
    InStep,
    /// The packet's new text.
    Rewritten(String),
}

/// Why [`rewrite`] could not update the packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    /// Not UTF-8, not well-formed, over the size ceiling, or no `rdf:RDF`.
    Unreadable,
    /// The value holds a character XML 1.0 cannot carry.
    Unrepresentable,
}

/// A decoded packet split into its byte-order mark and its text.
pub(super) struct Packet<'a> {
    pub(super) bom: bool,
    pub(super) text: &'a str,
}

impl<'a> Packet<'a> {
    /// Read decoded stream bytes as a UTF-8 packet.
    ///
    /// # Errors
    ///
    /// [`Failure::Unreadable`] for a non-UTF-8 or oversized packet. UTF-16
    /// packets (allowed by ISO 16684-1 §7.1) are reported, not converted.
    pub(super) fn read(bytes: &'a [u8]) -> Result<Self, Failure> {
        if bytes.len() > MAX_PACKET_BYTES {
            return Err(Failure::Unreadable);
        }
        let (bom, body) = match bytes.strip_prefix(UTF8_BOM) {
            Some(rest) => (true, rest),
            None => (false, bytes),
        };
        let text = std::str::from_utf8(body).map_err(|_| Failure::Unreadable)?;
        Ok(Self { bom, text })
    }

    /// `text` as stream bytes, with the byte-order mark this packet had.
    pub(super) fn encode(&self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() + 3);
        if self.bom {
            out.extend_from_slice(UTF8_BOM);
        }
        out.extend_from_slice(text.as_bytes());
        out
    }
}

fn parse(text: &str) -> Result<Document<'_>, Failure> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_NODES,
    };
    Document::parse_with_options(text, options).map_err(|_| Failure::Unreadable)
}

fn is(node: Node<'_, '_>, ns: &str, local: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == local
}

fn rdf_root<'a, 'i>(doc: &'a Document<'i>) -> Result<Node<'a, 'i>, Failure> {
    doc.descendants()
        .find(|n| is(*n, RDF_NS, "RDF"))
        .ok_or(Failure::Unreadable)
}

fn descriptions<'a, 'i>(rdf: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    rdf.children().filter(|n| is(*n, RDF_NS, "Description"))
}

/// The packet's `pdfaid:part`, as an attribute or an element.
///
/// # Errors
///
/// [`Failure::Unreadable`] when the packet does not parse.
pub(super) fn pdfa_part(text: &str) -> Result<Option<u32>, Failure> {
    let doc = parse(text)?;
    let rdf = rdf_root(&doc)?;
    for desc in descriptions(rdf) {
        if let Some(v) = desc.attribute((PDFAID_NS, "part")) {
            return Ok(v.trim().parse().ok());
        }
        if let Some(el) = desc.children().find(|n| is(*n, PDFAID_NS, "part")) {
            return Ok(el.text().unwrap_or("").trim().parse().ok());
        }
    }
    Ok(None)
}

/// Where the property currently is.
enum Found<'a, 'i> {
    Attr(Attribute<'a, 'i>),
    Elem(Node<'a, 'i>),
}

fn find<'a, 'i>(rdf: Node<'a, 'i>, prop: &Property) -> Option<Found<'a, 'i>> {
    for desc in descriptions(rdf) {
        let attr = desc
            .attributes()
            .find(|a| a.namespace() == Some(prop.ns) && a.name() == prop.local);
        if let Some(a) = attr {
            return Some(Found::Attr(a));
        }
        if let Some(el) = desc.children().find(|n| is(*n, prop.ns, &prop.local)) {
            return Some(Found::Elem(el));
        }
    }
    None
}

/// Set (`Some`) or remove (`None`) `prop` in the packet `text`.
///
/// # Errors
///
/// [`Failure::Unreadable`] when the packet does not parse or has no
/// `rdf:RDF`; [`Failure::Unrepresentable`] when `value` holds a character
/// XML 1.0 forbids.
pub(super) fn rewrite(
    text: &str,
    prop: &Property,
    value: Option<&str>,
    policy: Policy,
) -> Result<Change, Failure> {
    if value.is_some_and(|v| v.chars().any(forbidden_in_xml)) {
        return Err(Failure::Unrepresentable);
    }
    let doc = parse(text)?;
    let rdf = rdf_root(&doc)?;
    let found = find(rdf, prop);
    let items = value.map(|v| items_for(prop, v, policy.authors));
    let splices = match (found, items) {
        (None, None) => return Ok(Change::InStep),
        (Some(f), None) => vec![(removal_range(text, &f), String::new())],
        (Some(f), Some(items)) => {
            if current_items(&f, prop.shape).as_deref() == Some(items.as_slice()) {
                return Ok(Change::InStep);
            }
            replace(text, rdf, &f, prop, &items, policy.languages)?
        }
        (None, Some(items)) => vec![insert(text, rdf, prop, &items)?],
    };
    Ok(Change::Rewritten(apply(text, splices)))
}

/// The item values `value` becomes: one for a simple or Alt property, and
/// for a `Seq` one item, or one per `;`-separated part (`XI-A2`).
fn items_for(prop: &Property, value: &str, authors: AuthorItems) -> Vec<String> {
    if prop.shape == Shape::Seq && authors == AuthorItems::SplitOnSemicolons {
        let parts: Vec<String> = value
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect();
        if !parts.is_empty() {
            return parts;
        }
    }
    vec![value.to_owned()]
}

/// What the property says now, in the same shape [`items_for`] returns.
/// `None` when it is in a shape this module would rebuild.
fn current_items(found: &Found<'_, '_>, shape: Shape) -> Option<Vec<String>> {
    let el = match found {
        Found::Attr(a) if shape == Shape::Simple => return Some(vec![a.value().to_owned()]),
        Found::Attr(..) => return None,
        Found::Elem(el) => *el,
    };
    match shape {
        Shape::Simple => {
            if el.children().any(|n| n.is_element()) {
                return None;
            }
            Some(vec![el.children().filter_map(|n| n.text()).collect()])
        }
        Shape::LangAlt => x_default(el).map(|li| vec![li.text().unwrap_or("").to_owned()]),
        Shape::Seq => {
            let seq = el.children().find(|n| is(*n, RDF_NS, "Seq"))?;
            Some(
                lis(seq)
                    .map(|li| li.text().unwrap_or("").to_owned())
                    .collect(),
            )
        }
    }
}

fn lis<'a, 'i>(container: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    container.children().filter(|n| is(*n, RDF_NS, "li"))
}

fn alt<'a, 'i>(el: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    el.children().find(|n| is(*n, RDF_NS, "Alt"))
}

fn x_default<'a, 'i>(el: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    lis(alt(el)?).find(|li| li.attribute((XML_NS, "lang")) == Some("x-default"))
}

/// The Alt items besides `x-default` that survive under `languages`
/// (`XI-A3`), as their original source text.
fn kept_languages<'i>(text: &'i str, el: Node<'_, 'i>, languages: AltLanguages) -> Vec<&'i str> {
    let Some(alt) = alt(el) else {
        return Vec::new();
    };
    let old_default = x_default(el).map(|li| li.text().unwrap_or(""));
    lis(alt)
        .filter(|li| li.attribute((XML_NS, "lang")) != Some("x-default"))
        .filter(|li| match languages {
            AltLanguages::Keep => true,
            AltLanguages::Drop => false,
            AltLanguages::ReplaceDuplicates => {
                old_default.is_none_or(|d| li.text().unwrap_or("") != d)
            }
        })
        .filter_map(|li| text.get(li.range()))
        .collect()
}

/// The splices that set an existing property to `items`.
fn replace(
    text: &str,
    rdf: Node<'_, '_>,
    found: &Found<'_, '_>,
    prop: &Property,
    items: &[String],
    languages: AltLanguages,
) -> Result<Vec<(Range<usize>, String)>, Failure> {
    match found {
        Found::Attr(a) if prop.shape == Shape::Simple => {
            Ok(vec![(a.range_value(), escape(first(items), true))])
        }
        // A Seq or Alt written as an attribute is not valid RDF; move it
        // into an element of the right shape.
        Found::Attr(..) => Ok(vec![
            (removal_range(text, found), String::new()),
            insert(text, rdf, prop, items)?,
        ]),
        Found::Elem(el) => {
            let kept = if prop.shape == Shape::LangAlt {
                kept_languages(text, *el, languages)
            } else {
                Vec::new()
            };
            let rdf_prefix = el.lookup_prefix(RDF_NS).ok_or(Failure::Unreadable)?;
            let (open, qname) = start_tag(text, *el).ok_or(Failure::Unreadable)?;
            let body = content(prop.shape, rdf_prefix, items, &kept);
            Ok(vec![(el.range(), format!("{open}{body}</{qname}>"))])
        }
    }
}

/// An element's start tag as `<qname ...>` (never self-closing), and its
/// qualified name, both taken from the source.
fn start_tag<'i>(text: &'i str, el: Node<'_, 'i>) -> Option<(String, &'i str)> {
    let src = text.get(el.range())?;
    let after_lt = src.get(1..)?;
    let name_len = after_lt.find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')?;
    let tag = src.get(..start_tag_end(src)?)?;
    let open = match tag.strip_suffix("/>") {
        Some(head) => format!("{}>", head.trim_end()),
        None => tag.to_owned(),
    };
    Some((open, after_lt.get(..name_len)?))
}

/// The byte length of the start tag at the head of `src`, through its
/// `>`, skipping quoted attribute values.
fn start_tag_end(src: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in src.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn qualify(prefix: &str, local: &str) -> String {
    if prefix.is_empty() {
        local.to_owned()
    } else {
        format!("{prefix}:{local}")
    }
}

/// The element content for `items` in `shape`, with `kept` Alt items
/// appended after the `x-default` one.
fn content(shape: Shape, rdf: &str, items: &[String], kept: &[&str]) -> String {
    let li = qualify(rdf, "li");
    match shape {
        Shape::Simple => escape(first(items), false),
        Shape::LangAlt => {
            let alt = qualify(rdf, "Alt");
            format!(
                "<{alt}><{li} xml:lang=\"x-default\">{}</{li}>{}</{alt}>",
                escape(first(items), false),
                kept.concat()
            )
        }
        Shape::Seq => {
            let seq = qualify(rdf, "Seq");
            let body: String = items
                .iter()
                .map(|v| format!("<{li}>{}</{li}>", escape(v, false)))
                .collect();
            format!("<{seq}>{body}</{seq}>")
        }
    }
}

/// The splice that adds `prop` to the first `rdf:Description`, or a new
/// `rdf:Description` holding it when the packet has none.
fn insert(
    text: &str,
    rdf: Node<'_, '_>,
    prop: &Property,
    items: &[String],
) -> Result<(Range<usize>, String), Failure> {
    let rdf_prefix = rdf.lookup_prefix(RDF_NS).ok_or(Failure::Unreadable)?;
    let host = descriptions(rdf).next();
    let scope = host.unwrap_or(rdf);
    let (qname, decl) = match scope.lookup_prefix(prop.ns) {
        Some(p) => (qualify(p, &prop.local), String::new()),
        None => (
            qualify(prop.prefix, &prop.local),
            format!(" xmlns:{}=\"{}\"", prop.prefix, prop.ns),
        ),
    };
    let body = content(prop.shape, rdf_prefix, items, &[]);
    let element = format!("<{qname}{decl}>{body}</{qname}>");
    match host {
        Some(desc) => append_child(text, desc, &element),
        None => {
            let d = qualify(rdf_prefix, "Description");
            let about = qualify(rdf_prefix, "about");
            append_child(text, rdf, &format!("<{d} {about}=\"\">{element}</{d}>"))
        }
    }
}

/// The splice that makes `child` the last child of `parent`, opening a
/// self-closing `parent` when needed.
fn append_child(
    text: &str,
    parent: Node<'_, '_>,
    child: &str,
) -> Result<(Range<usize>, String), Failure> {
    let range = parent.range();
    let src = text.get(range.clone()).ok_or(Failure::Unreadable)?;
    if src.ends_with("/>") && start_tag_end(src) == Some(src.len()) {
        let (_, qname) = start_tag(text, parent).ok_or(Failure::Unreadable)?;
        let at = range.end - 2;
        return Ok((at..range.end, format!(">{child}</{qname}>")));
    }
    let close = range.start + src.rfind("</").ok_or(Failure::Unreadable)?;
    let at = trimmed_end(text, close);
    Ok((at..at, format!("{}{child}", child_indent(text, parent))))
}

/// A newline and the indentation of `parent`'s last element child, or of
/// `parent` plus one step; empty when the packet is written on one line.
fn child_indent(text: &str, parent: Node<'_, '_>) -> String {
    let line_indent = |pos: usize| -> Option<&str> {
        let line_start = text.get(..pos)?.rfind('\n')? + 1;
        let ws = text.get(line_start..pos)?;
        ws.chars().all(is_xml_space).then_some(ws)
    };
    if let Some(last) = parent.children().rev().find(Node::is_element) {
        return line_indent(last.range().start).map_or_else(String::new, |ws| format!("\n{ws}"));
    }
    line_indent(parent.range().start).map_or_else(String::new, |ws| format!("\n{ws}  "))
}

/// The bytes removing a found property deletes: the attribute or element
/// and the whitespace before it.
fn removal_range(text: &str, found: &Found<'_, '_>) -> Range<usize> {
    let range = match found {
        Found::Attr(a) => a.range(),
        Found::Elem(el) => el.range(),
    };
    trimmed_end(text, range.start)..range.end
}

/// `end` moved back over the XML whitespace before it.
fn trimmed_end(text: &str, end: usize) -> usize {
    text.get(..end)
        .map_or(end, |head| head.trim_end_matches(is_xml_space).len())
}

/// The first item, which every caller has at least one of.
fn first(items: &[String]) -> &str {
    items.first().map_or("", String::as_str)
}

const fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// Apply non-overlapping splices to `text`.
fn apply(text: &str, mut splices: Vec<(Range<usize>, String)>) -> String {
    splices.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = text.to_owned();
    for (range, with) in splices {
        if out.is_char_boundary(range.start) && out.is_char_boundary(range.end) {
            out.replace_range(range, &with);
        }
    }
    out
}

/// Whether XML 1.0 §2.2 `Char` excludes `c`.
const fn forbidden_in_xml(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// `value` escaped for element content, or for a `"`-quoted attribute
/// value. Carriage returns and (in attributes) tabs and newlines become
/// character references so a parser's normalisation cannot change them.
fn escape(value: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\r' => out.push_str("&#xD;"),
            '\n' if attribute => out.push_str("&#xA;"),
            '\t' if attribute => out.push_str("&#x9;"),
            _ => out.push(c),
        }
    }
    out
}

/// An `/Info` key as an XMP local name (XMP Part 3 §2.2.1): a BMP
/// character that is not an XML name character becomes U+2182 and four
/// hex digits; a non-BMP character becomes its two surrogates, each so
/// escaped. A first character that may continue but not start a name is
/// escaped too (spec RAG gap `XI-N1`, DERIVED). `:` is escaped because
/// a local name is an NCName.
pub(super) fn escape_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for (i, c) in key.chars().enumerate() {
        let ok = if i == 0 { name_start(c) } else { name_char(c) };
        if ok && u32::from(c) <= 0xFFFF {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\u{2182}{unit:04X}"));
            }
        }
    }
    out
}

/// XML 1.0 (fifth edition) `NameStartChar`, without `:`.
const fn name_start(c: char) -> bool {
    matches!(c,
        'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}'
        | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}'
        | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}')
}

/// XML 1.0 (fifth edition) `NameChar`, without `:`.
const fn name_char(c: char) -> bool {
    name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

#[cfg(test)]
mod tests;
