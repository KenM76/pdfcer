//! XFA forms: which kind a document carries (ISO 32000-1 §12.7.8, Table 28).
//!
//! [`crate::forms::XfaPresence`] says an `/AcroForm /XFA` layer exists, which
//! is equally true of a static form (real PDF pages beside the XML) and a
//! dynamic one (one placeholder page; the real pages exist only once an XFA
//! engine lays the template out). [`form_kind`] tells the two apart.

mod packets;
mod template;

pub use packets::{MAX_XDP_LEN, MAX_XML_DEPTH, XdpDocument, XdpPacket, XfaError, read_xdp};
pub use template::{
    AnchorType, ContainerKind, LayoutStrategy, Measurement, Occur, Presence, Template, XfaNode,
    read_template,
};

use crate::graph::ObjectGraph;
use crate::object::Object;

/// The kind of XFA form a document carries.
///
/// Read by [`form_kind`]. The test is `/NeedsRendering` (ISO 32000-1
/// Table 28, PDF 1.7; deprecated with XFA in ISO 32000-2). ISO places it in
/// the catalog; XFA 3.3's Adobe-implementation chapter places it in the
/// AcroForm dictionary, so either location counts:
/// `true` says the document "shall be regenerated when first opened",
/// which is the spec's own statement that the pages in the file are not the
/// document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XfaFormKind {
    /// No `/AcroForm /XFA` layer. A stray `/NeedsRendering` without one is
    /// meaningless and reads as this.
    NotXfa,
    /// An XFA layer with `/NeedsRendering` absent or `false`: the file's own
    /// pages are the document, and the XML travels beside them.
    Static,
    /// An XFA layer with `/NeedsRendering true`: the file's pages are a
    /// placeholder written for viewers without an XFA engine, and the
    /// document lives in the template and datasets packets.
    Dynamic,
}

impl XfaFormKind {
    /// Whether the pages in the file are a placeholder, not the document.
    #[must_use]
    pub const fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic)
    }
}

/// Read which kind of XFA form `graph` carries.
///
/// Generic over [`ObjectGraph`], so it reads a [`crate::document::Document`]
/// and a live edit overlay alike. Never fails: a missing or malformed catalog
/// reads as [`XfaFormKind::NotXfa`], and a `/NeedsRendering` that is not a
/// boolean reads as its default, `false`.
///
/// # Example
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::xfa::{form_kind, XfaFormKind};
///
/// let doc = Document::from_bytes(std::fs::read("lease.pdf")?)?;
/// if form_kind(&doc) == XfaFormKind::Dynamic {
///     println!("pages are an XFA placeholder");
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn form_kind<G: ObjectGraph + ?Sized>(graph: &G) -> XfaFormKind {
    let Some(catalog) = graph.catalog_dict() else {
        return XfaFormKind::NotXfa;
    };
    let Some(acro) = catalog
        .get(b"AcroForm")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
        .filter(|acro| acro.get(b"XFA").is_some())
    else {
        return XfaFormKind::NotXfa;
    };
    let flag = |d: &crate::object::Dict| {
        matches!(
            d.get(b"NeedsRendering").map(|o| graph.resolve(o)),
            Some(Object::Boolean(true))
        )
    };
    if flag(catalog) || flag(acro) {
        XfaFormKind::Dynamic
    } else {
        XfaFormKind::Static
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pageops::tests_support::build_pdf;

    fn kind_of(catalog_extra: &str, acroform: &str) -> XfaFormKind {
        let catalog = format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} {acroform} >>");
        let doc = build_pdf(&[
            (1, catalog.as_str()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
            (4, "<< /Length 0 >>\nstream\n\nendstream"),
        ]);
        form_kind(&doc)
    }

    const XFA: &str = "/AcroForm << /Fields [] /XFA 4 0 R >>";

    #[test]
    fn needs_rendering_true_with_xfa_is_dynamic() {
        assert_eq!(kind_of("/NeedsRendering true", XFA), XfaFormKind::Dynamic);
        assert!(kind_of("/NeedsRendering true", XFA).is_dynamic());
    }

    #[test]
    fn xfa_without_the_flag_or_with_false_is_static() {
        assert_eq!(kind_of("", XFA), XfaFormKind::Static);
        assert_eq!(kind_of("/NeedsRendering false", XFA), XfaFormKind::Static);
    }

    #[test]
    fn the_flag_in_the_acroform_dictionary_also_counts() {
        let acro = "/AcroForm << /Fields [] /XFA 4 0 R /NeedsRendering true >>";
        assert_eq!(kind_of("", acro), XfaFormKind::Dynamic);
    }

    #[test]
    fn a_non_boolean_flag_reads_as_its_default() {
        assert_eq!(kind_of("/NeedsRendering 1", XFA), XfaFormKind::Static);
    }

    #[test]
    fn the_flag_without_an_xfa_layer_is_not_xfa() {
        let plain = "/AcroForm << /Fields [] >>";
        assert_eq!(kind_of("/NeedsRendering true", plain), XfaFormKind::NotXfa);
        assert_eq!(kind_of("/NeedsRendering true", ""), XfaFormKind::NotXfa);
    }
}
