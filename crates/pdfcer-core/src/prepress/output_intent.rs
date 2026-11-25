//! The document's own output intent: the one entry of `/OutputIntents` the
//! renderer, soft proofing and colour conversion all use.

use std::sync::Arc;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

use super::PdfxOutputIntent;

/// The output intent pdfcer uses, from [`output_intent`]: its decoded
/// destination profile and its identifying entries (ISO 32000-2 §14.11.5,
/// Table 365).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocumentOutputIntent {
    /// The decoded `/DestOutputProfile` ICC bytes.
    pub profile: Arc<[u8]>,
    /// The profile stream's `/N`, its component count (Table 66): four or
    /// more is a subtractive (print) condition.
    pub components: i64,
    /// `/S`, the intent subtype (`GTS_PDFX`, `GTS_PDFA1`, `ISO_PDFE1`).
    pub subtype: Option<String>,
    /// `/OutputConditionIdentifier`, such as `FOGRA39` or `Custom`.
    pub condition_identifier: Option<String>,
    /// `/OutputCondition`, the human-readable condition.
    pub condition: Option<String>,
    /// `/RegistryName`, conventionally `http://www.color.org`.
    pub registry_name: Option<String>,
    /// `/Info`, further human-readable identification.
    pub info: Option<String>,
}

/// The output intent pdfcer uses for this document, or `None` when it has
/// no usable one.
///
/// # Which intent, when there are several
///
/// ISO 32000 gives no selector (the choice is "a matter for agreement" out
/// of band). pdfcer takes the first `/OutputIntents` entry whose
/// `/DestOutputProfile` is a stream with an integer `/N` (Table 66) that
/// decodes, so a broken intent listed ahead of a good one is passed over.
/// The `/S` subtype is not consulted. An intent naming a registered
/// condition with no profile is skipped: resolving the name needs a
/// registry pdfcer does not ship and the engine must not fetch
/// (`ARCHITECTURE.md` §1.1). The renderer's blending space, soft proofing
/// and `ColourDestination::OutputIntent` all read this one choice.
///
/// Read-only; the profile is decoded through the stream's filters with
/// their output ceilings.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::prepress::output_intent;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// assert!(output_intent(&doc.view()).is_none());
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn output_intent(view: &DocumentView<'_>) -> Option<DocumentOutputIntent> {
    let catalog = view
        .trailer_entry(b"Root")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)?;
    let items = catalog
        .get(b"OutputIntents")
        .and_then(|o| view.resolve(o).as_array())?;
    items.iter().find_map(|item| {
        let intent = view.resolve(item).as_dict()?;
        let Object::Stream(st) = view.resolve(intent.get(b"DestOutputProfile")?) else {
            return None;
        };
        let components = st.dict.get(b"N").map(|o| view.resolve(o))?.as_int()?;
        let raw = view.slice(st.data_span)?;
        let profile = crate::filters::decode_stream(&st.dict, raw).ok()?;
        let text = |key: &[u8]| text_entry(view, intent, key);
        Some(DocumentOutputIntent {
            profile: Arc::from(profile),
            components,
            subtype: intent
                .get(b"S")
                .and_then(|o| view.resolve(o).as_name())
                .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
            condition_identifier: text(b"OutputConditionIdentifier"),
            condition: text(b"OutputCondition"),
            registry_name: text(b"RegistryName"),
            info: text(b"Info"),
        })
    })
}

fn text_entry(view: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<String> {
    match view.resolve(dict.get(key)?) {
        Object::String(s) => Some(crate::textstring::decode_text_string(s).text),
        _ => None,
    }
}

impl PdfxOutputIntent {
    /// The document's own output intent ([`output_intent`]) as a PDF/X
    /// destination: its profile, `/OutputConditionIdentifier`,
    /// `/RegistryName` and `/Info`. `None` when the document has no usable
    /// intent.
    #[must_use]
    pub fn from_document(view: &DocumentView<'_>) -> Option<Self> {
        output_intent(view).map(|i| Self {
            profile: i.profile.to_vec(),
            condition: i.condition_identifier,
            registry_name: i.registry_name,
            info: i.info,
        })
    }
}
