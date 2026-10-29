//! The Internet Printing Protocol message codec (RFC 8010 §3, semantics
//! RFC 8011): encode a Print-Job or Get-Printer-Attributes request, decode
//! a printer's response. Pure bytes, no network and no platform code, so it
//! compiles and is tested on every target and fuzzed (`fuzz/ipp_response`).
//! The HTTP POST that carries a message is `pdfcer_fetch::post_ipp`.
//!
//! A message is `version(2) operation-or-status(2) request-id(4)`, then
//! attribute groups each opened by a delimiter tag, then the end tag
//! `0x03`, then the document data. An attribute is `value-tag(1)
//! name-length(2) name value-length(2) value`; a name length of 0 adds
//! another value to the previous attribute (RFC 8010 §3.1.5).

/// IPP/1.1, the version every IPP printer accepts (RFC 8011 §4.1.8).
pub const VERSION: (u8, u8) = (1, 1);

/// Print-Job (RFC 8011 §4.2.1): one request carrying the document.
pub const OP_PRINT_JOB: u16 = 0x0002;
/// Get-Printer-Attributes (RFC 8011 §4.2.5).
pub const OP_GET_PRINTER_ATTRIBUTES: u16 = 0x000B;

/// Delimiter tags (RFC 8010 §3.5.1).
pub mod group {
    /// operation-attributes-tag.
    pub const OPERATION: u8 = 0x01;
    /// job-attributes-tag.
    pub const JOB: u8 = 0x02;
    /// end-of-attributes-tag.
    pub const END: u8 = 0x03;
    /// printer-attributes-tag.
    pub const PRINTER: u8 = 0x04;
    /// unsupported-attributes-tag: what the printer ignored or substituted.
    pub const UNSUPPORTED: u8 = 0x05;
}

/// Value tags (RFC 8010 §3.5.2).
pub mod tag {
    /// integer.
    pub const INTEGER: u8 = 0x21;
    /// boolean.
    pub const BOOLEAN: u8 = 0x22;
    /// enum.
    pub const ENUM: u8 = 0x23;
    /// rangeOfInteger.
    pub const RANGE: u8 = 0x33;
    /// textWithoutLanguage.
    pub const TEXT: u8 = 0x41;
    /// nameWithoutLanguage.
    pub const NAME: u8 = 0x42;
    /// keyword.
    pub const KEYWORD: u8 = 0x44;
    /// uri.
    pub const URI: u8 = 0x45;
    /// charset.
    pub const CHARSET: u8 = 0x47;
    /// naturalLanguage.
    pub const LANGUAGE: u8 = 0x48;
    /// mimeMediaType.
    pub const MIME: u8 = 0x49;
}

/// The most attributes a response may carry before it is refused; a real
/// printer's full set is a few hundred.
pub const MAX_ATTRIBUTES: usize = 20_000;

/// One attribute value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IppValue {
    /// `integer`.
    Integer(i32),
    /// `boolean`.
    Boolean(bool),
    /// `enum`.
    Enum(i32),
    /// `rangeOfInteger`, inclusive.
    Range(i32, i32),
    /// Any character-string syntax (text, name, keyword, uri, charset,
    /// naturalLanguage, mimeMediaType, uriScheme), with its value tag.
    String {
        /// The value tag, one of the `0x41..=0x49` syntaxes.
        tag: u8,
        /// The value, decoded as UTF-8 (lossy).
        value: String,
    },
    /// An out-of-band value (`0x10..=0x1F`: unsupported, unknown, no-value).
    OutOfBand(u8),
    /// Any other syntax (dates, resolutions, collections), kept as bytes.
    Other {
        /// The value tag.
        tag: u8,
        /// The value bytes.
        bytes: Vec<u8>,
    },
}

impl IppValue {
    /// A `keyword` value.
    pub fn keyword(value: &str) -> Self {
        Self::string(tag::KEYWORD, value)
    }

    /// A string value of syntax `tag`.
    pub fn string(tag: u8, value: &str) -> Self {
        Self::String {
            tag,
            value: value.to_owned(),
        }
    }

    /// The value as text, for a character-string syntax.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The value as an integer, for `integer` and `enum`.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Integer(n) | Self::Enum(n) => Some(*n),
            _ => None,
        }
    }

    fn tag(&self) -> u8 {
        match self {
            Self::Integer(_) => tag::INTEGER,
            Self::Boolean(_) => tag::BOOLEAN,
            Self::Enum(_) => tag::ENUM,
            Self::Range(..) => tag::RANGE,
            Self::String { tag, .. } | Self::Other { tag, .. } => *tag,
            Self::OutOfBand(tag) => *tag,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Integer(n) | Self::Enum(n) => n.to_be_bytes().to_vec(),
            Self::Boolean(b) => vec![u8::from(*b)],
            Self::Range(lo, hi) => [lo.to_be_bytes(), hi.to_be_bytes()].concat(),
            Self::String { value, .. } => value.as_bytes().to_vec(),
            Self::OutOfBand(_) => Vec::new(),
            Self::Other { bytes, .. } => bytes.clone(),
        }
    }
}

/// A named attribute and its values (one or more; `1setOf` is several).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppAttribute {
    /// The attribute name, e.g. `document-format-supported`.
    pub name: String,
    /// Its values, in wire order.
    pub values: Vec<IppValue>,
}

impl IppAttribute {
    /// An attribute with one value.
    pub fn new(name: &str, value: IppValue) -> Self {
        Self::many(name, vec![value])
    }

    /// An attribute with several values (`1setOf`).
    pub fn many(name: &str, values: Vec<IppValue>) -> Self {
        Self {
            name: name.to_owned(),
            values,
        }
    }
}

/// An attribute group: its delimiter tag and attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppGroup {
    /// The delimiter tag, one of [`group`]'s constants.
    pub tag: u8,
    /// The group's attributes, in wire order.
    pub attributes: Vec<IppAttribute>,
}

impl IppGroup {
    /// A group of `attributes` opened by delimiter `tag`.
    pub fn new(tag: u8, attributes: Vec<IppAttribute>) -> Self {
        Self { tag, attributes }
    }
}

/// Encode a request: `operation`, its groups in order, the end tag, then
/// `document`. A name or value longer than 65 535 bytes is truncated (the
/// field's length is a `u16`).
pub fn encode_request(
    operation: u16,
    request_id: u32,
    groups: &[IppGroup],
    document: &[u8],
) -> Vec<u8> {
    let mut out = vec![VERSION.0, VERSION.1];
    out.extend_from_slice(&operation.to_be_bytes());
    out.extend_from_slice(&request_id.to_be_bytes());
    for g in groups {
        out.push(g.tag);
        for attribute in &g.attributes {
            for (i, value) in attribute.values.iter().enumerate() {
                let name = if i == 0 { attribute.name.as_str() } else { "" };
                push_field(&mut out, value.tag(), name.as_bytes(), &value.bytes());
            }
        }
    }
    out.push(group::END);
    out.extend_from_slice(document);
    out
}

fn push_field(out: &mut Vec<u8>, tag: u8, name: &[u8], value: &[u8]) {
    let len = |b: &[u8]| u16::try_from(b.len()).unwrap_or(u16::MAX);
    let name = name.get(..usize::from(len(name))).unwrap_or_default();
    let value = value.get(..usize::from(len(value))).unwrap_or_default();
    out.push(tag);
    out.extend_from_slice(&len(name).to_be_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&len(value).to_be_bytes());
    out.extend_from_slice(value);
}

/// The operation attributes every request opens with (RFC 8011 §4.1.4):
/// charset, natural language, the target printer and the requesting user.
pub fn operation_group(printer_uri: &str, user: &str) -> IppGroup {
    IppGroup::new(
        group::OPERATION,
        vec![
            IppAttribute::new(
                "attributes-charset",
                IppValue::string(tag::CHARSET, "utf-8"),
            ),
            IppAttribute::new(
                "attributes-natural-language",
                IppValue::string(tag::LANGUAGE, "en"),
            ),
            IppAttribute::new("printer-uri", IppValue::string(tag::URI, printer_uri)),
            IppAttribute::new("requesting-user-name", IppValue::string(tag::NAME, user)),
        ],
    )
}

/// The printer attributes pdfcer asks for before printing.
pub const QUERIED_ATTRIBUTES: [&str; 11] = [
    "printer-name",
    "printer-make-and-model",
    "printer-state",
    "printer-state-reasons",
    "document-format-supported",
    "copies-supported",
    "sides-supported",
    "print-color-mode-supported",
    "page-ranges-supported",
    "finishings-supported",
    "output-bin-supported",
];

/// A Get-Printer-Attributes request for [`QUERIED_ATTRIBUTES`].
pub fn get_printer_attributes(printer_uri: &str, user: &str, request_id: u32) -> Vec<u8> {
    let mut op = operation_group(printer_uri, user);
    op.attributes.push(IppAttribute::many(
        "requested-attributes",
        QUERIED_ATTRIBUTES
            .iter()
            .map(|a| IppValue::keyword(a))
            .collect(),
    ));
    encode_request(OP_GET_PRINTER_ATTRIBUTES, request_id, &[op], &[])
}

mod http;
mod job;
mod response;

pub use http::{MAX_HTTP_RESPONSE_BYTES, TransportError, post_ipp_http};
pub use job::{IppJob, Sides};
pub use response::{IppError, IppResponse, http_url, parse_response, status_name};

#[cfg(test)]
mod tests;
