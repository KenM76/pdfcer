//! Which PDF/X level a file claims (ISO 15930), read from where each part
//! puts the claim. A claim only: nothing here checks conformance.
//!
//! Carriers: the Info dictionary's `GTS_PDFXVersion` (and, for the 2001
//! edition only, `GTS_PDFXConformance`) through PDF/X-3:2003; the XMP
//! property `GTS_PDFXVersion` in namespace `http://www.npes.org/pdfx/ns/id/`
//! from PDF/X-4 on (ISO 15930-1 §5, 15930-4 §5, 15930-7 §6.10–6.11, 15930-9
//! §6.11.3). The header and Catalog `/Version` are never consulted (15930-3
//! and -4 §5). A `/GTS_PDFX` output intent alone is not a claim (15930-3
//! §6.2.2 requires it; no part identifies a file by it).

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

/// The PDF/X identification namespace (`pdfxid`).
const PDFXID_NS: &[u8] = b"http://www.npes.org/pdfx/ns/id/";
/// Longest claim value quoted back; registered values are at most 15.
const MAX_VALUE: usize = 64;
/// Namespace declarations examined in one packet.
const MAX_PREFIXES: usize = 32;

/// Every registered `GTS_PDFXVersion` value (CGATS GTS_ registry) plus the
/// PDF/X-6 levels of ISO 15930-9.
const KNOWN_LEVELS: &[&str] = &[
    "CGATS.12/1-1999",
    "PDF/X-1:2001",
    "PDF/X-1a:2001",
    "PDF/X-3:2002",
    "PDF/X-1a:2003",
    "PDF/X-2:2003",
    "PDF/X-3:2003",
    "PDF/X-4",
    "PDF/X-4p",
    "PDF/X-5g",
    "PDF/X-5n",
    "PDF/X-5pg",
    "PDF/X-6",
    "PDF/X-6p",
    "PDF/X-6n",
];

/// The PDF/X claim a file makes. From [`pdfx_claim`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PdfxClaim {
    /// The level claimed, verbatim: the XMP value when there is one, else
    /// the Info value, with `PDF/X-1:2001` read as `PDF/X-1a:2001` when
    /// `GTS_PDFXConformance` says so. An unregistered value is passed
    /// through, never mapped to the nearest known level.
    pub level: String,
    /// XMP `pdfxid:GTS_PDFXVersion`.
    pub xmp_version: Option<String>,
    /// Info `/GTS_PDFXVersion`.
    pub info_version: Option<String>,
    /// Info `/GTS_PDFXConformance` (the 2001 edition's X-1 / X-1a split).
    pub info_conformance: Option<String>,
    /// The `/OutputConditionIdentifier` of each `/OutputIntents` entry with
    /// `/S /GTS_PDFX`, empty text where an entry has none.
    pub output_intents: Vec<String>,
}

impl PdfxClaim {
    /// XMP and Info do not both state a version, or state the same one.
    /// PDF/X-4 on requires them to match when both are present.
    #[must_use]
    pub fn carriers_agree(&self) -> bool {
        match (&self.xmp_version, &self.info_version) {
            (Some(x), Some(i)) => x == i,
            _ => true,
        }
    }

    /// [`Self::level`] is a registered PDF/X level.
    #[must_use]
    pub fn is_recognised(&self) -> bool {
        KNOWN_LEVELS.contains(&self.level.as_str())
    }

    /// The file has the `/GTS_PDFX` output intent every PDF/X part requires.
    #[must_use]
    pub fn has_output_intent(&self) -> bool {
        !self.output_intents.is_empty()
    }
}

/// The PDF/X level `view` claims, or `None` when neither the XMP packet
/// nor the Info dictionary states one. An undecodable XMP packet, or an
/// XMP packet not in UTF-8, is read as stating nothing; a value that is
/// not short printable text is ignored.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::prepress::pdfx_claim;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf").to_vec(),
/// )?;
/// assert_eq!(pdfx_claim(&doc.view()), None);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn pdfx_claim(view: &DocumentView<'_>) -> Option<PdfxClaim> {
    let catalog = view
        .trailer_entry(b"Root")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let xmp_version = catalog.and_then(|c| xmp_version(view, c));
    let info = view
        .trailer_entry(b"Info")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let info_text = |key: &[u8]| info.and_then(|d| text_entry(view, d, key));
    let info_version = info_text(b"GTS_PDFXVersion");
    let info_conformance = info_text(b"GTS_PDFXConformance");
    let version = xmp_version.clone().or_else(|| info_version.clone())?;
    let level = match info_conformance.as_deref() {
        Some(c @ "PDF/X-1a:2001") if version == "PDF/X-1:2001" => c.to_owned(),
        _ => version,
    };
    Some(PdfxClaim {
        level,
        xmp_version,
        info_version,
        info_conformance,
        output_intents: catalog.map(|c| output_intents(view, c)).unwrap_or_default(),
    })
}

fn text_entry(view: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<String> {
    match view.resolve(dict.get(key)?) {
        Object::String(s) => claim_value(&crate::textstring::decode_text_string(s).text),
        _ => None,
    }
}

/// `value` trimmed, when it is short printable ASCII.
fn claim_value(value: &str) -> Option<String> {
    let v = value.trim();
    let printable = v.bytes().all(|b| b.is_ascii_graphic() || b == b' ');
    (!v.is_empty() && v.len() <= MAX_VALUE && printable).then(|| v.to_owned())
}

/// The `/OutputConditionIdentifier` of each `/OutputIntents` entry with
/// `/S /GTS_PDFX` (empty text where an entry has none), whether or not the
/// file claims a PDF/X level. Every PDF/X part requires such an intent;
/// a shell that counts an intent alone as "PDF/X" asks this beside
/// [`pdfx_claim`].
#[must_use]
pub fn pdfx_output_intents(view: &DocumentView<'_>) -> Vec<String> {
    view.trailer_entry(b"Root")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .map(|c| output_intents(view, c))
        .unwrap_or_default()
}

fn output_intents(view: &DocumentView<'_>, catalog: &Dict) -> Vec<String> {
    let items = catalog
        .get(b"OutputIntents")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|i| view.resolve(i).as_dict())
        .filter(|d| {
            d.get(b"S")
                .map(|s| view.resolve(s))
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"GTS_PDFX")
        })
        .map(|d| text_entry(view, d, b"OutputConditionIdentifier").unwrap_or_default())
        .collect()
}

fn xmp_version(view: &DocumentView<'_>, catalog: &Dict) -> Option<String> {
    let Object::Stream(stream) = view.resolve(catalog.get(b"Metadata")?) else {
        return None;
    };
    let bytes = view
        .slice(stream.data_span)
        .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok())?;
    let window = bytes
        .get(
            ..bytes
                .len()
                .min(crate::font_unembed::MAX_METADATA_SCAN_BYTES),
        )
        .unwrap_or(&bytes);
    namespace_prefixes(window, PDFXID_NS)
        .into_iter()
        .find_map(|p| xml_property(window, p, b"GTS_PDFXVersion"))
}

/// The prefixes `packet` binds to `uri` (`xmlns:p="uri"`); an empty prefix
/// for a default-namespace declaration (`xmlns="uri"`).
fn namespace_prefixes<'p>(packet: &'p [u8], uri: &[u8]) -> Vec<&'p [u8]> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = find(packet.get(at..).unwrap_or_default(), b"xmlns") {
        let start = at + found + b"xmlns".len();
        at = start;
        let rest = packet.get(start..).unwrap_or_default();
        let (prefix, after) = match rest.split_first() {
            Some((b':', tail)) => {
                let len = tail.iter().take_while(|&&b| is_name_byte(b)).count();
                (tail.get(..len).unwrap_or_default(), tail.get(len..))
            }
            _ => (&[][..], Some(rest)),
        };
        if after.and_then(attribute_value) == Some(uri) {
            out.push(prefix);
            if out.len() >= MAX_PREFIXES {
                break;
            }
        }
    }
    out
}

/// The first value of property `prefix:local` in attribute form
/// (`prefix:local="v"`) or element form (`<prefix:local>v<`). An empty
/// prefix matches the element form only: a default namespace does not
/// apply to attributes.
fn xml_property(packet: &[u8], prefix: &[u8], local: &[u8]) -> Option<String> {
    let qname = if prefix.is_empty() {
        local.to_vec()
    } else {
        [prefix, b":", local].concat()
    };
    let mut at = 0;
    while let Some(found) = find(packet.get(at..)?, &qname) {
        let start = at + found;
        at = start + qname.len();
        let before = start.checked_sub(1).and_then(|i| packet.get(i)).copied();
        let rest = packet.get(at..)?;
        let value = match before {
            Some(b'<') => element_text(rest),
            Some(b' ' | b'\t' | b'\r' | b'\n') if !prefix.is_empty() => attribute_value(rest),
            _ => None,
        };
        if let Some(v) = value.and_then(|v| claim_value(std::str::from_utf8(v).ok()?)) {
            return Some(v);
        }
    }
    None
}

/// The text content after a start tag's name: past any attributes to `>`,
/// up to the next `<`. `None` for an empty-element tag or a name that only
/// begins `rest`'s real name.
fn element_text(rest: &[u8]) -> Option<&[u8]> {
    let close = rest.iter().position(|&b| b == b'>')?;
    let tag = rest.get(..close)?;
    let is_name_end = tag.first().is_none_or(u8::is_ascii_whitespace);
    if !is_name_end || tag.last() == Some(&b'/') {
        return None;
    }
    let text = rest.get(close + 1..)?;
    text.get(..text.iter().position(|&b| b == b'<')?)
}

/// The quoted value after `=` at the start of `rest`, either quote.
fn attribute_value(rest: &[u8]) -> Option<&[u8]> {
    let rest = rest
        .trim_ascii_start()
        .strip_prefix(b"=")?
        .trim_ascii_start();
    let (&quote, tail) = rest.split_first()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    tail.get(..tail.iter().position(|&b| b == quote)?)
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.') || b >= 0x80
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
#[path = "pdfx_tests.rs"]
mod tests;
