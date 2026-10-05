//! Recognising pdfcer's OCR layer during extraction, and the filter that
//! keeps or drops its text.
//!
//! A pdfcer OCR layer is a content stream wrapped in
//! `/pdfc_OCR << /Producer (pdfcer) … >> BDC … EMC` (the writer is
//! `pdfcer_core::ocr`). The tag alone is not enough: the `/Producer` check
//! is what tells pdfcer's layer from another tool's use of the same name.
//! Invisible text (render mode 3 or 7) is a different, wider question: every
//! OCR producer writes it, and so do some forms and accessibility tools.

use pdfcer_model::graph::ObjectGraph;
use pdfcer_model::object::{Dict, Object};
use pdfcer_model::view::DocumentView;

/// The marked-content tag around a pdfcer OCR layer.
pub const OCR_LAYER_TAG: &[u8] = b"pdfc_OCR";

/// The `/Producer` string in the tag's property list that makes the
/// sequence pdfcer's own.
pub const OCR_LAYER_PRODUCER: &[u8] = b"pdfcer";

/// Which text extraction keeps, by membership of a pdfcer OCR layer
/// ([`super::ExtractOptions::ocr_layer`]).
///
/// The filter runs during the content walk, so everything built on an
/// extraction (plain text, [`crate::block_layout`], table detection and the
/// exports that consume them) sees the same subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OcrLayerFilter {
    /// All text, OCR layer included. The default.
    #[default]
    All,
    /// Only text inside a pdfcer OCR layer.
    OnlyOcrLayer,
    /// Everything except text inside a pdfcer OCR layer.
    WithoutOcrLayer,
}

impl OcrLayerFilter {
    /// Whether text whose layer membership is `in_ocr_layer` is kept.
    #[must_use]
    pub const fn keeps(self, in_ocr_layer: bool) -> bool {
        match self {
            Self::All => true,
            Self::OnlyOcrLayer => in_ocr_layer,
            Self::WithoutOcrLayer => !in_ocr_layer,
        }
    }
}

/// Whether a `BDC` with `tag` and property list `props` opens a pdfcer OCR
/// layer.
pub(super) fn opens_ocr_layer(doc: &DocumentView<'_>, tag: &[u8], props: Option<&Dict>) -> bool {
    if tag != OCR_LAYER_TAG {
        return false;
    }
    let producer = props
        .and_then(|p| p.get(b"Producer"))
        .map(|o| doc.resolve(o));
    matches!(producer, Some(Object::String(s)) if s.as_slice() == OCR_LAYER_PRODUCER)
}

#[cfg(test)]
mod tests {
    use super::OcrLayerFilter;

    #[test]
    fn each_filter_keeps_its_side() {
        let keeps = |f: OcrLayerFilter| (f.keeps(true), f.keeps(false));
        assert_eq!(keeps(OcrLayerFilter::All), (true, true));
        assert_eq!(keeps(OcrLayerFilter::OnlyOcrLayer), (true, false));
        assert_eq!(keeps(OcrLayerFilter::WithoutOcrLayer), (false, true));
        assert_eq!(OcrLayerFilter::default(), OcrLayerFilter::All);
    }
}
