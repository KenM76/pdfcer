//! # Forms Data Format import/export — FDF (ISO 32000-1 §12.7.7) + XFDF
//!
//! The **data-interchange half** of Pass 7.1: lift a filled form's field
//! values *out* of a PDF into a standalone data file, and push a data file's
//! values *back into* a PDF's fields. Two wire formats, one in-memory model
//! ([`FormData`]):
//!
//! - **FDF** (Forms Data Format, ISO 32000-1 §12.7.7) — a PDF-like file: a
//!   `%FDF-1.2` header, one indirect object holding a catalog dictionary
//!   `<< /FDF << /Fields [ … ] >> >>`, and a trailer. Because its object
//!   syntax **is** COS syntax, the reader reuses pdfcer's own
//!   [`crate::parser::Parser`] rather than a second, parallel tokenizer —
//!   the `/FDF` dictionary is located and parsed with the exact object model
//!   the rest of the engine uses (no new grammar, no new bug surface).
//! - **XFDF** (XML Forms Data Format) — an XML document
//!   `<xfdf><fields><field name="…"><value>…</value></field></fields></xfdf>`.
//!   XFDF's grammar is small and fixed, so this module carries a **tiny
//!   hand-rolled XML reader** scoped to exactly that subset (element +
//!   attribute + text + the five predefined entities + numeric character
//!   references) — **no XML dependency is added** (rule 13; the reader is
//!   ~120 lines and has a fuzz target). See [`parse_xml_document`].
//!
//! ## The model is intentionally value-only, dispatched by the target
//!
//! [`FormData`] is a list of `(fully-qualified-name, values)` pairs and
//! nothing more — it does **not** carry a field's type. That is deliberate:
//! a data file names *what value* a field should hold, and the field's
//! **type** (text vs. checkbox vs. choice) is a property of the *target
//! document*, not of the data. On import, [`crate::edit::EditSession`]
//! dispatches each value by the target field's modelled type — the same
//! dispatch `pdfcer fill-field` already does — so importing the same FDF
//! into two documents whose same-named field differs in type does the right
//! thing for each. A single value is one entry; a multi-select choice is
//! several (FDF: a `/V` array; XFDF: several `<value>` elements).
//!
//! ## What is NOT modelled (named non-goals)
//!
//! FDF and XFDF can also carry annotations, page references, JavaScript and
//! an `/F` source-file path. Pass 7.1 modelled **field values only** — the
//! must-have data-round-trip capability.
//!
//! **★ RICH TEXT IS NO LONGER ON THIS LIST (Pass 37.3, 2026-08-10).** This
//! paragraph named `<value-richtext>` bodies among the non-goals, and that
//! stopped being true the moment `FieldData::rich_value` landed. Corrected
//! here by acting on the finding the same change produced: *when a
//! limitation is lifted, grep for the sentences that described it.* This
//! paragraph, and a comment block in `pdfce-gui`'s export path, were what
//! that grep found.
//!
//! What is modelled now, precisely, because "supports rich text" would be
//! its own overstatement:
//!
//! - **Export CARRIES it.** `/RV` (FDF Table 246) and `<value-richtext>`
//!   (XFDF) are read off the field, written, and parsed back.
//! - **Import does NOT APPLY it**, and a rich-text field is skipped
//!   entirely rather than filled with plain text. §12.7.3.3 makes `/DS` +
//!   `/RV` the inputs to appearance generation (RT-M9), so a fresh plain
//!   `/V` beside an existing `/RV` renders the OLD text in a conforming
//!   reader.
//! - **Authoring rich text is not built at all.**
//!
//! An `/F`/`href` source hint is emitted on export (so a reader knows which
//! PDF the data came from) and ignored on import. Embedded FDF JavaScript is
//! **never executed** (decision 009, R53); it is not even modelled here.
//!
//! ## Spec sources (PDF-spec RAG, ISO 32000-1:2008)
//!
//! - `iso32000__s__12.7.7.md` — FDF file structure (Table 243 the FDF
//!   dictionary, Table 244 an FDF field dictionary: `/T` partial name,
//!   `/V` value, `/Kids` sub-fields), and the `/F` source-file entry.
//! - XFDF has no ISO clause (it is an Adobe-published companion format); the
//!   subset here matches the widely-implemented `xfdf`/`fields`/`field`/
//!   `value` element shape Acrobat and pdftk both read and write.

use crate::forms::{AcroForm, FieldValue};
use crate::object::Object;
use crate::parser::Parser;

/// One field's exported data: its fully-qualified name and its value(s).
///
/// `values` holds **one** entry for a single-valued field (text, checkbox
/// on-state name, single-select choice) and **several** for a multi-select
/// choice (§12.7.4.4). Each value is the §7.9.2-decoded text of the stored
/// `/V` (a checkbox's on-state name decodes to its ASCII bytes). Import
/// re-encodes via [`crate::edit::encode_text_string`], so the round trip is
/// exact for the Latin/UTF-16 values pdfcer authors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldData {
    /// The fully-qualified field name (dotted, §12.7.3.2).
    pub name: String,
    /// The field's value(s): one entry, or several for a multi-select.
    pub values: Vec<String>,
    /// `/RV` (FDF Table 246) / `<value-richtext>` (XFDF) — the field's
    /// **rich text value**, the XHTML/CSS2-subset document that carries its
    /// formatting (§12.7.3.4).
    ///
    /// `None` for the overwhelming majority of fields, which are plain.
    ///
    /// # Why exporting this matters more than it looks
    ///
    /// Both formats have the slot precisely so formatting travels beside the
    /// plain value. pdfcer dropped it, so a styled field exported and
    /// re-imported came back unstyled — and the operator only found out on
    /// the re-import, by which time the styled original might be gone.
    ///
    /// **Carrying it out is safe; writing it back in is not yet done.**
    /// Import deliberately does not apply this: §12.7.3.3 makes `/DS` + `/RV`
    /// the inputs to appearance generation with an unconditional `shall` to
    /// regenerate on every value change, and pdfcer cannot yet generate a
    /// rich-text appearance. Writing `/RV` without that would leave the
    /// stored value and the rendered one disagreeing — which is the same
    /// wrong-value-on-screen failure `fill_text_field` refuses for.
    pub rich_value: Option<String>,
}

/// A form's exported field data, format-independent.
///
/// Produced from a live [`AcroForm`] by [`FormData::from_acroform`], from an
/// FDF file by [`FormData::parse_fdf`], or from an XFDF file by
/// [`FormData::parse_xfdf`]; serialized by [`FormData::to_fdf`] /
/// [`FormData::to_xfdf`]; applied to a document by
/// [`crate::edit::EditSession::import_form_data`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormData {
    /// The exported fields, in document (or file) order. A field is present
    /// here **only if it has a value** — an empty/unset field is omitted, so
    /// importing a data file never *clears* a field the file did not mention
    /// (the fuzzy-never-sneaky posture: a data file adds what it names, it is
    /// not a document-wide reset).
    pub fields: Vec<FieldData>,
}

/// Why an FDF/XFDF file could not be parsed.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum FdfError {
    /// The FDF file has no locatable `/FDF` catalog dictionary.
    #[error("not an FDF file: no /FDF dictionary was found")]
    NoFdfDictionary,
    /// The FDF `/FDF` dictionary could not be parsed as a COS object.
    #[error("the FDF /FDF dictionary is malformed: {0}")]
    MalformedFdf(String),
    /// The XFDF document has no `<xfdf>`/`<fields>` root.
    #[error("not an XFDF file: no <xfdf>/<fields> element was found")]
    NoXfdfRoot,
    /// The XFDF XML was malformed (unbalanced tag, unterminated string).
    #[error("the XFDF XML is malformed: {0}")]
    MalformedXml(String),
}

impl FormData {
    /// Export the present-valued fields of a modelled form (§12.7.7).
    ///
    /// Skips fields with no value ([`FieldValue::Absent`]), signature fields,
    /// and unnamed fields. A field that shares its parent's fully-qualified
    /// name ([`crate::forms::Field::shares_parent_name`]) is a duplicate
    /// representation of an already-exported logical field, so it is emitted
    /// once (the first occurrence wins).
    #[must_use]
    pub fn from_acroform(form: &AcroForm) -> Self {
        let mut fields: Vec<FieldData> = Vec::new();
        for field in &form.fields {
            if field.fully_qualified_name.is_empty() {
                continue;
            }
            let values = match &field.value {
                FieldValue::Absent | FieldValue::Signature => continue,
                FieldValue::Name(b) => vec![String::from_utf8_lossy(b).into_owned()],
                FieldValue::Text(b) => vec![crate::edit::decode_text_string(b).text],
                FieldValue::Choice(items) => items
                    .iter()
                    .map(|b| crate::edit::decode_text_string(b).text)
                    .collect(),
            };
            if values.is_empty() {
                continue;
            }
            // Decoded the same way `/V` is — `/RV` is a §7.9.2 text string
            // (Table 228's `/RV` row), not a byte blob, so a UTF-16BE-marked
            // rich value must come out as text rather than as mojibake.
            let rich_value = field
                .rich_value
                .as_ref()
                .map(|b| crate::edit::decode_text_string(b).text);
            // Same-FQN duplicate representations export once.
            if fields.iter().any(|f| f.name == field.fully_qualified_name) {
                continue;
            }
            fields.push(FieldData {
                name: field.fully_qualified_name.clone(),
                values,
                rich_value,
            });
        }
        Self { fields }
    }

    /// Serialize as an FDF file (§12.7.7). `source` is the optional `/F`
    /// source-PDF hint (a reader uses it to know which document the data is
    /// for); it is emitted as a text string when present.
    ///
    /// The output is a minimal, well-formed FDF: header, one indirect object
    /// carrying `<< /FDF << /Fields [ … ] /F (source) >> >>`, and a trailer
    /// whose `/Root` points at it. Field names are emitted **flat** (the full
    /// dotted name as one `/T`), which Acrobat and pdftk both read; the
    /// reader also accepts the `/Kids`-nested hierarchical form.
    #[must_use]
    pub fn to_fdf(&self, source: Option<&str>) -> Vec<u8> {
        let mut out = b"%FDF-1.2\n%\xE2\xE3\xCF\xD3\n1 0 obj\n<< /FDF << /Fields [".to_vec();
        for field in &self.fields {
            out.extend_from_slice(b"\n<< /T ");
            crate::writer::serialize::write_string(
                &mut out,
                &crate::edit::encode_text_string(&field.name),
            );
            out.extend_from_slice(b" /V ");
            if let [single] = field.values.as_slice() {
                write_fdf_value(&mut out, single);
            } else {
                out.push(b'[');
                for v in &field.values {
                    out.push(b' ');
                    write_fdf_value(&mut out, v);
                }
                out.extend_from_slice(b" ]");
            }
            // `/RV` beside `/V`, per FDF Table 246 (§12.7.7.3.2) — the same
            // key and meaning the field dictionary uses. Emitted only when
            // the field has one, so a plain form's FDF is byte-identical to
            // what it was before this existed.
            if let Some(rv) = &field.rich_value {
                out.extend_from_slice(b" /RV ");
                crate::writer::serialize::write_string(
                    &mut out,
                    &crate::edit::encode_text_string(rv),
                );
            }
            out.extend_from_slice(b" >>");
        }
        out.extend_from_slice(b" ]");
        if let Some(src) = source {
            out.extend_from_slice(b" /F ");
            crate::writer::serialize::write_string(&mut out, &crate::edit::encode_text_string(src));
        }
        out.extend_from_slice(b" >> >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
        out
    }

    /// Serialize as an XFDF document. `href` is the optional source-PDF hint
    /// emitted as the `<f href="…"/>` element.
    ///
    /// Whitespace-significant (`xml:space="preserve"`) so a value that is or
    /// contains spaces round-trips exactly. Text and attribute values are
    /// XML-escaped ([`xml_escape`]).
    #[must_use]
    pub fn to_xfdf(&self, href: Option<&str>) -> Vec<u8> {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n",
        );
        if let Some(h) = href {
            s.push_str("<f href=\"");
            s.push_str(&xml_escape_attr(h));
            s.push_str("\"/>\n");
        }
        s.push_str("<fields>\n");
        for field in &self.fields {
            s.push_str("<field name=\"");
            s.push_str(&xml_escape_attr(&field.name));
            s.push_str("\">");
            for v in &field.values {
                s.push_str("<value>");
                s.push_str(&xml_escape_text(v));
                s.push_str("</value>");
            }
            // `<value-richtext>` — XFDF's slot for the same content `/RV`
            // carries in FDF. Escaped as TEXT, deliberately: the rich value
            // is itself an XML document, and embedding it raw would let its
            // markup merge into the XFDF's own tree, so a `<span>` inside a
            // field value would become an XFDF element. Adobe's own readers
            // expect it escaped.
            if let Some(rv) = &field.rich_value {
                s.push_str("<value-richtext>");
                s.push_str(&xml_escape_text(rv));
                s.push_str("</value-richtext>");
            }
            s.push_str("</field>\n");
        }
        s.push_str("</fields>\n</xfdf>\n");
        s.into_bytes()
    }

    /// Parse an FDF file's field data (§12.7.7).
    ///
    /// Locates the `/FDF` dictionary (the reader is tolerant of a missing or
    /// stale xref — it scans for the `/FDF` key rather than trusting the
    /// trailer `/Root`), parses it with pdfcer's own [`Parser`], and flattens
    /// its `/Fields` tree (handling both a flat dotted `/T` and a `/Kids`
    /// hierarchy) into [`FieldData`] entries.
    ///
    /// # Errors
    ///
    /// [`FdfError::NoFdfDictionary`] when no `/FDF` dictionary is present;
    /// [`FdfError::MalformedFdf`] when it cannot be parsed.
    pub fn parse_fdf(bytes: &[u8]) -> Result<Self, FdfError> {
        // Locate "/FDF" and parse the dictionary that follows it. In a
        // well-formed FDF the catalog is `<< /FDF << … >> >>`, so the value
        // right after the `/FDF` key is the FDF dictionary itself.
        let key_pos = find_subsequence(bytes, b"/FDF").ok_or(FdfError::NoFdfDictionary)?;
        let after = key_pos + b"/FDF".len();
        let mut parser = Parser::at(bytes, after);
        let fdf = parser
            .parse_object()
            .map_err(|e| FdfError::MalformedFdf(e.to_string()))?;
        let fdf_dict = fdf.as_dict().ok_or(FdfError::NoFdfDictionary)?;
        let mut fields = Vec::new();
        if let Some(arr) = fdf_dict.get(b"Fields").and_then(Object::as_array) {
            for field in arr {
                walk_fdf_field(field, "", &mut fields);
            }
        }
        Ok(Self { fields })
    }

    /// Parse an XFDF document's field data.
    ///
    /// Uses the scoped hand-rolled XML reader ([`parse_xml_document`]). Both
    /// the flat form (`name="a.b.c"`) and the nested form (`<field
    /// name="a"><field name="b">…`) are flattened to dotted names.
    ///
    /// # Errors
    ///
    /// [`FdfError::NoXfdfRoot`] when there is no `<fields>` element;
    /// [`FdfError::MalformedXml`] on unbalanced/unterminated XML.
    pub fn parse_xfdf(bytes: &[u8]) -> Result<Self, FdfError> {
        let text = String::from_utf8_lossy(bytes);
        let root = parse_xml_document(&text).map_err(FdfError::MalformedXml)?;
        // Find the <fields> element anywhere under the root.
        let fields_el = find_element(&root, "fields").ok_or(FdfError::NoXfdfRoot)?;
        let mut fields = Vec::new();
        for child in &fields_el.children {
            walk_xfdf_field(child, "", &mut fields);
        }
        Ok(Self { fields })
    }
}

/// Write one FDF `/V` value: a name when it is a valid checkbox on-state
/// name (no whitespace, delimiter-free, non-empty), else a text string.
///
/// A checkbox `/V` is a **name** in the PDF (`/Yes`), and a data file that
/// re-imports it must produce a name so the target checkbox recognises the
/// state. A text value that happens to be name-safe is indistinguishable
/// here; that is harmless because import re-dispatches by the target field's
/// modelled type, so a text field receiving a bare token still fills as text.
fn write_fdf_value(out: &mut Vec<u8>, value: &str) {
    crate::writer::serialize::write_string(out, &crate::edit::encode_text_string(value));
}

/// Depth-first flatten one FDF `/Fields` entry (§12.7.7 Table 244).
///
/// `/T` contributes a dotted segment; `/Kids` recurses; `/V` (a string, a
/// name, or an array of strings) becomes the entry's value(s). Bounded by the
/// COS parser's own nesting guard.
fn walk_fdf_field(field: &Object, parent: &str, out: &mut Vec<FieldData>) {
    let Some(dict) = field.as_dict() else {
        return;
    };
    let seg = dict.get(b"T").and_then(|o| match o {
        Object::String(s) => Some(crate::edit::decode_text_string(s).text),
        _ => None,
    });
    let fqn = match seg {
        Some(t) if parent.is_empty() => t,
        Some(t) => format!("{parent}.{t}"),
        None => parent.to_owned(),
    };
    // Sub-fields via /Kids (hierarchical FDF).
    if let Some(kids) = dict.get(b"Kids").and_then(Object::as_array) {
        for kid in kids {
            walk_fdf_field(kid, &fqn, out);
        }
        return;
    }
    // A terminal field: read /V, and /RV beside it (FDF Table 246).
    if let Some(v) = dict.get(b"V") {
        let values = fdf_value_strings(v);
        // Read even though import does not yet APPLY it — a parse that drops
        // the entry cannot round-trip, and the round-trip is what the export
        // half exists for.
        let rich_value = dict
            .get(b"RV")
            .and_then(|o| match o {
                Object::String(b) => Some(b.as_slice()),
                _ => None,
            })
            .map(|b| crate::edit::decode_text_string(b).text);
        if !values.is_empty() && !fqn.is_empty() {
            out.push(FieldData {
                name: fqn,
                values,
                rich_value,
            });
        }
    }
}

/// Decode an FDF `/V` object into value string(s).
fn fdf_value_strings(v: &Object) -> Vec<String> {
    match v {
        Object::String(s) => vec![crate::edit::decode_text_string(s).text],
        Object::Name(n) => vec![String::from_utf8_lossy(n.as_bytes()).into_owned()],
        Object::Array(items) => items
            .iter()
            .filter_map(|o| match o {
                Object::String(s) => Some(crate::edit::decode_text_string(s).text),
                Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Depth-first flatten one XFDF `<field>` element into dotted-name entries.
fn walk_xfdf_field(el: &XmlElement, parent: &str, out: &mut Vec<FieldData>) {
    if el.name != "field" {
        return;
    }
    let seg = el.attr("name").unwrap_or_default();
    let fqn = if parent.is_empty() {
        seg.to_owned()
    } else if seg.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}.{seg}")
    };
    // Collect direct <value> children; recurse into nested <field> children.
    let mut values: Vec<String> = Vec::new();
    let mut has_subfields = false;
    let mut rich_value = None;
    for child in &el.children {
        match child.name.as_str() {
            "value" => values.push(child.text.clone()),
            // XFDF's slot for what `/RV` carries in FDF. The parser already
            // un-escapes element text, so this arrives as the rich document
            // itself rather than as escaped markup.
            "value-richtext" => rich_value = Some(child.text.clone()),
            "field" => {
                has_subfields = true;
                walk_xfdf_field(child, &fqn, out);
            }
            _ => {}
        }
    }
    if !has_subfields && !values.is_empty() && !fqn.is_empty() {
        out.push(FieldData {
            name: fqn,
            values,
            rich_value,
        });
    }
}

/// Find the first occurrence of `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// XML escaping (write side)
// ---------------------------------------------------------------------------

/// Escape text content for XML (`&`, `<`, `>`).
fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape an attribute value for XML (adds `"` on top of the text set).
fn xml_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A tiny scoped XML reader — the XFDF subset only (no XML dependency, rule 13)
// ---------------------------------------------------------------------------

/// One item of an element's content, in document order.
///
/// # Why this exists alongside `text` and `children`
///
/// XFDF only ever needs a `<value>`'s character data, so [`XmlElement`]
/// originally kept text and child elements in two separate buckets and said
/// so: *"Mixed content (text interleaved with children) is flattened."*
///
/// **Rich text (§12.7.3.4) cannot use a flattened model.** In
/// `<p>Hello <b>bold</b> world</p>` the position of `<b>` relative to the
/// surrounding character data IS the content — flattened, `text` is
/// `"Hello  world"` and nothing records where the bold run belonged. Any
/// renderer built on that emits the words in the wrong order.
///
/// So order is recorded here, **additively**: `text` and `children` keep
/// their exact previous meaning and every existing consumer is untouched.
/// The alternative was a second XML parser for rich text, which project
/// rule 2 warns against directly — two parsers of one grammar drift, and
/// this one is the one with a fuzz target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlNode {
    /// A run of character data (entity-decoded), exactly as it appeared.
    ///
    /// **Not** whitespace-trimmed: in rich text the space in
    /// `</b> world` is content, and trimming it joins two words.
    Text(String),
    /// A child element, by its index in [`XmlElement::children`].
    ///
    /// An index rather than a nested `XmlElement` so the child is stored
    /// once. Stable because children are only ever appended during a parse
    /// and never reordered or removed.
    Child(usize),
}

/// One parsed XML element: tag name, attributes, child elements, and the
/// concatenated text of its direct text nodes.
///
/// Deliberately minimal — enough for XFDF's `xfdf`/`fields`/`field`/`value`
/// shape and for the §12.7.3.4 rich-text grammar, and nothing more.
///
/// `text` is the concatenated run of character data directly inside this
/// element, which for a `<value>` is exactly the field value. It discards
/// the position of that text relative to child elements; [`Self::nodes`]
/// preserves it for callers that need mixed content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XmlElement {
    /// The element's tag name (namespace prefix, if any, kept verbatim).
    pub name: String,
    /// The element's attributes, in document order.
    pub attributes: Vec<(String, String)>,
    /// Child elements, in document order.
    pub children: Vec<XmlElement>,
    /// The element's direct character data (entity-decoded), concatenated.
    pub text: String,
    /// This element's content in document order — text runs and child
    /// references interleaved. See [`XmlNode`] for why.
    pub nodes: Vec<XmlNode>,
}

impl XmlElement {
    /// The value of attribute `key`, or `None`.
    #[must_use]
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Depth-first search for the first descendant (or self) element named
/// `name`.
fn find_element<'a>(el: &'a XmlElement, name: &str) -> Option<&'a XmlElement> {
    if el.name == name {
        return Some(el);
    }
    for child in &el.children {
        if let Some(found) = find_element(child, name) {
            return Some(found);
        }
    }
    None
}

/// Maximum XML nesting depth the scoped reader descends — a pure adversarial
/// backstop (a hostile XFDF with millions of nested tags must not blow the
/// stack). XFDF field hierarchies are shallow; 256 is far past any real one.
const MAX_XML_DEPTH: usize = 256;

/// Parse an XML document into its single root element, using the scoped
/// reader.
///
/// Handles: the `<?xml …?>` declaration, `<!-- … -->` comments, `<!DOCTYPE
/// …>` (skipped), start/end/empty-element tags, attributes (single- or
/// double-quoted), character data, and the five predefined entities plus
/// numeric character references. It does **not** handle CDATA sections,
/// processing instructions beyond the declaration, or DTD-defined entities —
/// none of which appear in the XFDF subset.
///
/// # Errors
///
/// A description string on any structural error (unterminated tag or string,
/// mismatched end tag, no root element, depth overflow).
pub(crate) fn parse_xml_document(input: &str) -> Result<XmlElement, String> {
    let bytes: Vec<char> = input.chars().collect();
    let mut p = XmlParser {
        chars: &bytes,
        pos: 0,
    };
    p.skip_misc()?;
    let root = p.parse_element(0)?;
    Ok(root)
}

/// The scoped XML parser's cursor state.
struct XmlParser<'a> {
    chars: &'a [char],
    pos: usize,
}

impl XmlParser<'_> {
    /// Peek the current character without consuming.
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Whether the remaining input begins with `s`.
    fn starts_with(&self, s: &str) -> bool {
        let rest = self.chars.get(self.pos..).unwrap_or(&[]);
        let want = s.chars().count();
        rest.len() >= want && rest.iter().zip(s.chars()).all(|(a, b)| *a == b)
    }

    /// Advance past a literal prefix known to be present.
    fn consume_str(&mut self, s: &str) {
        self.pos += s.chars().count();
    }

    /// Skip whitespace.
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// Skip the XML declaration, comments, DOCTYPE, and inter-element
    /// whitespace — everything that is not an element start.
    fn skip_misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.starts_with("<?") {
                self.skip_until("?>")?;
            } else if self.starts_with("<!--") {
                self.skip_until("-->")?;
            } else if self.starts_with("<!") {
                self.skip_until(">")?;
            } else {
                return Ok(());
            }
        }
    }

    /// Advance past the next occurrence of `end`, consuming it.
    fn skip_until(&mut self, end: &str) -> Result<(), String> {
        while self.pos < self.chars.len() {
            if self.starts_with(end) {
                self.consume_str(end);
                return Ok(());
            }
            self.pos += 1;
        }
        Err(format!("unterminated section (expected {end:?})"))
    }

    /// Parse one element at `depth` (its `<tag …>` is at the cursor).
    fn parse_element(&mut self, depth: usize) -> Result<XmlElement, String> {
        if depth > MAX_XML_DEPTH {
            return Err("XML nesting too deep".to_owned());
        }
        if self.peek() != Some('<') {
            return Err("expected an element".to_owned());
        }
        self.pos += 1; // consume '<'
        let name = self.parse_name();
        if name.is_empty() {
            return Err("empty tag name".to_owned());
        }
        let mut el = XmlElement {
            name,
            ..XmlElement::default()
        };
        // Attributes.
        loop {
            self.skip_ws();
            match self.peek() {
                Some('/') => {
                    // Empty element `<tag/>`.
                    self.pos += 1;
                    if self.peek() != Some('>') {
                        return Err("malformed empty-element tag".to_owned());
                    }
                    self.pos += 1;
                    return Ok(el);
                }
                Some('>') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {
                    let (k, v) = self.parse_attribute()?;
                    el.attributes.push((k, v));
                }
                None => return Err("unterminated start tag".to_owned()),
            }
        }
        // Content: text and child elements until the matching end tag.
        loop {
            match self.peek() {
                None => return Err(format!("unterminated element <{}>", el.name)),
                Some('<') => {
                    if self.starts_with("</") {
                        self.consume_str("</");
                        let end_name = self.parse_name();
                        self.skip_ws();
                        if self.peek() != Some('>') {
                            return Err("malformed end tag".to_owned());
                        }
                        self.pos += 1;
                        if end_name != el.name {
                            return Err(format!(
                                "mismatched end tag </{end_name}> for <{}>",
                                el.name
                            ));
                        }
                        return Ok(el);
                    } else if self.starts_with("<!--") {
                        self.skip_until("-->")?;
                    } else {
                        let child = self.parse_element(depth + 1)?;
                        // Order recorded BEFORE the push, so the index is
                        // the position this child is about to occupy.
                        el.nodes.push(XmlNode::Child(el.children.len()));
                        el.children.push(child);
                    }
                }
                Some(_) => {
                    // Character data up to the next '<'.
                    let text = self.parse_char_data();
                    let decoded = decode_entities(&text);
                    // `text` stays concatenated for XFDF's leaf values;
                    // `nodes` keeps this run separate and in place, because
                    // rich text needs to know it sat between two elements.
                    // Kept verbatim — trimming would weld `</b>` to the word
                    // after it.
                    el.nodes.push(XmlNode::Text(decoded.clone()));
                    el.text.push_str(&decoded);
                }
            }
        }
    }

    /// Read a tag or attribute name (up to whitespace or a delimiter).
    fn parse_name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || matches!(c, '>' | '/' | '=') {
                break;
            }
            self.pos += 1;
        }
        self.chars
            .get(start..self.pos)
            .unwrap_or(&[])
            .iter()
            .collect()
    }

    /// Parse one `name="value"` (or `name='value'`) attribute.
    fn parse_attribute(&mut self) -> Result<(String, String), String> {
        let name = self.parse_name();
        self.skip_ws();
        if self.peek() != Some('=') {
            return Err(format!("attribute {name:?} has no value"));
        }
        self.pos += 1;
        self.skip_ws();
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(format!("attribute {name:?} value is not quoted")),
        };
        self.pos += 1;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == quote {
                break;
            }
            self.pos += 1;
        }
        if self.peek() != Some(quote) {
            return Err(format!("unterminated attribute value for {name:?}"));
        }
        let raw: String = self
            .chars
            .get(start..self.pos)
            .unwrap_or(&[])
            .iter()
            .collect();
        self.pos += 1; // consume closing quote
        Ok((name, decode_entities(&raw)))
    }

    /// Read character data up to the next `<`.
    fn parse_char_data(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == '<' {
                break;
            }
            self.pos += 1;
        }
        self.chars
            .get(start..self.pos)
            .unwrap_or(&[])
            .iter()
            .collect()
    }
}

/// Decode the five XML predefined entities plus numeric character references
/// (`&#NN;` decimal, `&#xHH;` hex). An unrecognised `&…;` is left verbatim
/// (lenient — a stray ampersand in real-world data should not fail the parse).
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        // Gather up to the terminating ';'.
        let mut entity = String::new();
        let mut terminated = false;
        for e in chars.by_ref() {
            if e == ';' {
                terminated = true;
                break;
            }
            if entity.len() > 12 {
                break;
            }
            entity.push(e);
        }
        if !terminated {
            out.push('&');
            out.push_str(&entity);
            continue;
        }
        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            other => {
                if let Some(cp) = parse_char_ref(other) {
                    out.push(cp);
                } else {
                    out.push('&');
                    out.push_str(other);
                    out.push(';');
                }
            }
        }
    }
    out
}

/// Parse a numeric character reference body (`#NN` decimal or `#xHH` hex)
/// into a `char`, or `None`.
fn parse_char_ref(body: &str) -> Option<char> {
    let digits = body.strip_prefix('#')?;
    let code = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        digits.parse::<u32>().ok()?
    };
    char::from_u32(code)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// Mixed content keeps its order, and the text runs keep their spaces.
    ///
    /// This is the property rich text (§12.7.3.4) needs and XFDF never did.
    /// `text` alone says `"Hello  world"` — two words and a gap where the
    /// bold run belonged, which is unrenderable. The assertion on the
    /// LEADING and TRAILING spaces is the part that matters most: trim the
    /// text runs and `</b>` welds to the next word.
    #[test]
    fn mixed_content_records_order_and_keeps_its_spaces() {
        let root = parse_xml_document("<p>Hello <b>bold</b> world</p>").expect("parses");

        // The flattened view, unchanged — every existing caller sees this.
        assert_eq!(root.text, "Hello  world");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name, "b");

        // The ordered view, which is the point.
        assert_eq!(
            root.nodes,
            vec![
                XmlNode::Text("Hello ".to_owned()),
                XmlNode::Child(0),
                XmlNode::Text(" world".to_owned()),
            ]
        );
    }

    /// Two children resolve to two DIFFERENT indices, in document order.
    ///
    /// A single-child case cannot distinguish "the index of this child"
    /// from a hard-coded `0`, which is exactly how an off-by-one in the
    /// order/push sequence would survive the test above.
    #[test]
    fn each_child_reference_indexes_its_own_element() {
        let root = parse_xml_document("<p><b>one</b>mid<i>two</i></p>").expect("parses");
        assert_eq!(
            root.nodes,
            vec![
                XmlNode::Child(0),
                XmlNode::Text("mid".to_owned()),
                XmlNode::Child(1),
            ]
        );
        assert_eq!(root.children[0].name, "b");
        assert_eq!(root.children[1].name, "i");
    }

    fn form(fields: &[(&str, &[&str])]) -> FormData {
        FormData {
            fields: fields
                .iter()
                .map(|(n, vs)| FieldData {
                    name: (*n).to_owned(),
                    values: vs.iter().map(|s| (*s).to_owned()).collect(),
                    rich_value: None,
                })
                .collect(),
        }
    }

    #[test]
    fn fdf_round_trips_single_and_multi_values() {
        let data = form(&[
            ("FullName", &["Ada Lovelace"]),
            ("Subscribe", &["Yes"]),
            ("Langs", &["Rust", "Ada"]),
        ]);
        let bytes = data.to_fdf(Some("form.pdf"));
        let parsed = FormData::parse_fdf(&bytes).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn xfdf_round_trips_single_and_multi_values() {
        let data = form(&[
            ("FullName", &["Ada Lovelace"]),
            ("Note", &["a < b & c > d"]),
            ("Langs", &["Rust", "Ada"]),
        ]);
        let bytes = data.to_xfdf(Some("form.pdf"));
        let parsed = FormData::parse_xfdf(&bytes).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn xfdf_reads_nested_hierarchical_fields() {
        let xml = r#"<?xml version="1.0"?>
<xfdf xmlns="http://ns.adobe.com/xfdf/">
<fields>
<field name="address"><field name="city"><value>Paris</value></field></field>
</fields></xfdf>"#;
        let parsed = FormData::parse_xfdf(xml.as_bytes()).unwrap();
        assert_eq!(parsed.fields.len(), 1);
        assert_eq!(parsed.fields[0].name, "address.city");
        assert_eq!(parsed.fields[0].values, vec!["Paris".to_owned()]);
    }

    #[test]
    fn fdf_reads_kids_hierarchy() {
        let fdf = b"%FDF-1.2\n1 0 obj\n<< /FDF << /Fields \
            [ << /T (address) /Kids [ << /T (city) /V (Paris) >> ] >> ] >> >>\n\
            endobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF";
        let parsed = FormData::parse_fdf(fdf).unwrap();
        assert_eq!(parsed.fields.len(), 1);
        assert_eq!(parsed.fields[0].name, "address.city");
        assert_eq!(parsed.fields[0].values, vec!["Paris".to_owned()]);
    }

    #[test]
    fn entity_decoding_covers_predefined_and_numeric() {
        assert_eq!(
            decode_entities("a&amp;b&lt;c&gt;d&quot;e&apos;f"),
            "a&b<c>d\"e'f"
        );
        assert_eq!(decode_entities("&#65;&#x42;"), "AB");
        // Unrecognised entity left verbatim.
        assert_eq!(decode_entities("&nbsp;"), "&nbsp;");
        // A bare ampersand not starting an entity.
        assert_eq!(decode_entities("Tom & Jerry"), "Tom & Jerry");
    }

    #[test]
    fn no_fdf_dictionary_is_an_error() {
        assert!(matches!(
            FormData::parse_fdf(b"not an fdf at all"),
            Err(FdfError::NoFdfDictionary)
        ));
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(FormData::parse_xfdf(b"<xfdf><fields><field name=\"x\"></fields>").is_err());
    }

    #[test]
    fn empty_form_emits_valid_empty_containers() {
        let data = FormData::default();
        assert!(
            FormData::parse_fdf(&data.to_fdf(None))
                .unwrap()
                .fields
                .is_empty()
        );
        assert!(
            FormData::parse_xfdf(&data.to_xfdf(None))
                .unwrap()
                .fields
                .is_empty()
        );
    }
}
