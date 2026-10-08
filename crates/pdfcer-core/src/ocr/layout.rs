//! Page layout regions: the classes PP-DocLayoutV3 (the layout model
//! PaddleOCR-VL 1.5 runs before recognition) assigns, and the decoding of its
//! output rows. The model itself runs in `engine_layout` (`ocr-vl` feature).
//!
//! The model is a DETR detector: 300 fixed queries, each a class, a score, a
//! box in the original image's pixels and a predicted reading-order key.
//! [`decode_rows`] keeps the rows at or above a score threshold, drops
//! duplicates the way PaddleOCR-VL does (same class at IoU > 0.6, any class
//! at IoU > 0.98) and returns the rest in reading order.

use super::vl_pre::VlTask;

/// PaddleOCR-VL 1.5's score threshold; the model card's own is 0.5.
pub const DEFAULT_SCORE_THRESHOLD: f32 = 0.3;
/// Rows read from the model's output; it emits one per query.
pub const MAX_ROWS: usize = 300;
/// Same-class overlap above which the lower-scored region is dropped.
pub const SAME_CLASS_IOU: f32 = 0.6;
/// Any-class overlap above which the lower-scored region is dropped.
pub const ANY_CLASS_IOU: f32 = 0.98;

/// What a region holds: the model's 25 labels, in its index order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum LayoutClass {
    /// `abstract`.
    Abstract,
    /// `algorithm`: pseudo-code or code.
    Algorithm,
    /// `aside_text`: marginal text.
    AsideText,
    /// `chart`.
    Chart,
    /// `content`: a table of contents.
    Content,
    /// `display_formula`: a formula on its own line.
    DisplayFormula,
    /// `doc_title`.
    DocTitle,
    /// `figure_title`: a figure or table caption.
    FigureTitle,
    /// `footer`.
    Footer,
    /// `footer_image`.
    FooterImage,
    /// `footnote`.
    Footnote,
    /// `formula_number`: an equation number.
    FormulaNumber,
    /// `header`.
    Header,
    /// `header_image`.
    HeaderImage,
    /// `image`: a figure or photograph.
    Image,
    /// `inline_formula`.
    InlineFormula,
    /// `number`: a page number.
    Number,
    /// `paragraph_title`: a section heading.
    ParagraphTitle,
    /// `reference`: a bibliography block.
    Reference,
    /// `reference_content`: one bibliography entry.
    ReferenceContent,
    /// `seal`: a stamp.
    Seal,
    /// `table`.
    Table,
    /// `text`: body text.
    Text,
    /// `vertical_text`.
    VerticalText,
    /// `vision_footnote`: a note under a figure or table.
    VisionFootnote,
}

/// The coarse groups a shell layers regions by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum RegionGroup {
    /// Body text, footnotes, references, marginal text, code.
    Text,
    /// Document and section titles.
    Title,
    /// Figure and table captions.
    Caption,
    /// Tables.
    Table,
    /// Images and figures.
    Figure,
    /// Formulas and equation numbers.
    Formula,
    /// Charts.
    Chart,
    /// Seals and stamps.
    Seal,
    /// Running headers.
    Header,
    /// Running footers and page numbers.
    Footer,
}

const CLASSES: [(LayoutClass, &str); 25] = [
    (LayoutClass::Abstract, "abstract"),
    (LayoutClass::Algorithm, "algorithm"),
    (LayoutClass::AsideText, "aside_text"),
    (LayoutClass::Chart, "chart"),
    (LayoutClass::Content, "content"),
    (LayoutClass::DisplayFormula, "display_formula"),
    (LayoutClass::DocTitle, "doc_title"),
    (LayoutClass::FigureTitle, "figure_title"),
    (LayoutClass::Footer, "footer"),
    (LayoutClass::FooterImage, "footer_image"),
    (LayoutClass::Footnote, "footnote"),
    (LayoutClass::FormulaNumber, "formula_number"),
    (LayoutClass::Header, "header"),
    (LayoutClass::HeaderImage, "header_image"),
    (LayoutClass::Image, "image"),
    (LayoutClass::InlineFormula, "inline_formula"),
    (LayoutClass::Number, "number"),
    (LayoutClass::ParagraphTitle, "paragraph_title"),
    (LayoutClass::Reference, "reference"),
    (LayoutClass::ReferenceContent, "reference_content"),
    (LayoutClass::Seal, "seal"),
    (LayoutClass::Table, "table"),
    (LayoutClass::Text, "text"),
    (LayoutClass::VerticalText, "vertical_text"),
    (LayoutClass::VisionFootnote, "vision_footnote"),
];

impl LayoutClass {
    /// The class for the model's label index.
    #[must_use]
    pub fn from_index(index: usize) -> Option<Self> {
        CLASSES.get(index).map(|(c, _)| *c)
    }

    /// The model's label, e.g. `"display_formula"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        CLASSES
            .iter()
            .find(|(c, _)| *c == self)
            .map_or("", |(_, s)| s)
    }

    /// The class for a label as [`LayoutClass::as_str`] spells it.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        CLASSES.iter().find(|(_, s)| *s == label).map(|(c, _)| *c)
    }

    /// The coarse group this class is layered under.
    #[must_use]
    pub fn group(self) -> RegionGroup {
        use LayoutClass as C;
        match self {
            C::DocTitle | C::ParagraphTitle => RegionGroup::Title,
            C::FigureTitle => RegionGroup::Caption,
            C::Table => RegionGroup::Table,
            C::Image => RegionGroup::Figure,
            C::DisplayFormula | C::InlineFormula | C::FormulaNumber => RegionGroup::Formula,
            C::Chart => RegionGroup::Chart,
            C::Seal => RegionGroup::Seal,
            C::Header | C::HeaderImage => RegionGroup::Header,
            C::Footer | C::FooterImage | C::Number => RegionGroup::Footer,
            C::Abstract
            | C::Algorithm
            | C::AsideText
            | C::Content
            | C::Footnote
            | C::Reference
            | C::ReferenceContent
            | C::Text
            | C::VerticalText
            | C::VisionFootnote => RegionGroup::Text,
        }
    }
}

impl LayoutClass {
    /// What the vision model is asked to read this class as; `None` for
    /// pictures, which are not read.
    #[must_use]
    pub fn task(self) -> Option<VlTask> {
        use LayoutClass as C;
        match self {
            C::Image | C::HeaderImage | C::FooterImage => None,
            C::Table => Some(VlTask::Table),
            C::DisplayFormula | C::InlineFormula => Some(VlTask::Formula),
            C::Chart => Some(VlTask::Chart),
            C::Seal => Some(VlTask::Seal),
            _ => Some(VlTask::Ocr),
        }
    }
}

impl std::fmt::Display for LayoutClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One detected region.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LayoutRegion {
    /// What the region holds.
    pub class: LayoutClass,
    /// The model's score, 0..=1.
    pub score: f32,
    /// `[x0, y0, x1, y1]` in image pixels, y down, clamped to the image.
    pub bbox: [f32; 4],
    /// The model's reading-order key: smaller reads first. Not a dense rank.
    pub order: f32,
}

impl LayoutRegion {
    fn area(&self) -> f32 {
        let [x0, y0, x1, y1] = self.bbox;
        (x1 - x0).max(0.0) * (y1 - y0).max(0.0)
    }

    fn iou(&self, other: &Self) -> f32 {
        let [a0, b0, a1, b1] = self.bbox;
        let [c0, d0, c1, d1] = other.bbox;
        let w = (a1.min(c1) - a0.max(c0)).max(0.0);
        let h = (b1.min(d1) - b0.max(d0)).max(0.0);
        let inter = w * h;
        let union = self.area() + other.area() - inter;
        if union > 0.0 { inter / union } else { 0.0 }
    }
}

/// Turn the model's `[n, 7]` rows into regions: score filter, clamp, drop
/// duplicates, sort by reading order. Rows past [`MAX_ROWS`], with an
/// unknown class or a non-finite value, or with no area, are skipped.
#[must_use]
pub fn decode_rows(rows: &[f32], width: f32, height: f32, threshold: f32) -> Vec<LayoutRegion> {
    let mut found: Vec<LayoutRegion> = rows
        .chunks_exact(7)
        .take(MAX_ROWS)
        .filter_map(|r| decode_row(r, width, height, threshold))
        .collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<LayoutRegion> = Vec::with_capacity(found.len());
    for r in found {
        let duplicate = kept.iter().any(|k| {
            let iou = k.iou(&r);
            iou > ANY_CLASS_IOU || (k.class == r.class && iou > SAME_CLASS_IOU)
        });
        if !duplicate {
            kept.push(r);
        }
    }
    kept.sort_by(|a, b| a.order.total_cmp(&b.order));
    kept
}

fn decode_row(r: &[f32], width: f32, height: f32, threshold: f32) -> Option<LayoutRegion> {
    let &[class, score, x0, y0, x1, y1, order] = r else {
        return None;
    };
    if !r.iter().all(|v| v.is_finite()) || score < threshold || class < 0.0 {
        return None;
    }
    // The class column holds a small whole number as a float.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let class = LayoutClass::from_index(class.round() as usize)?;
    let bbox = [
        x0.clamp(0.0, width),
        y0.clamp(0.0, height),
        x1.clamp(0.0, width),
        y1.clamp(0.0, height),
    ];
    let region = LayoutRegion {
        class,
        score,
        bbox,
        order,
    };
    (region.area() > 0.0).then_some(region)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn row(class: f32, score: f32, b: [f32; 4], order: f32) -> [f32; 7] {
        [class, score, b[0], b[1], b[2], b[3], order]
    }

    #[test]
    fn labels_round_trip_in_model_order() {
        assert_eq!(LayoutClass::from_index(0), Some(LayoutClass::Abstract));
        assert_eq!(LayoutClass::from_index(21), Some(LayoutClass::Table));
        assert_eq!(
            LayoutClass::from_index(24),
            Some(LayoutClass::VisionFootnote)
        );
        assert_eq!(LayoutClass::from_index(25), None);
        for (i, (c, s)) in CLASSES.iter().enumerate() {
            assert_eq!(LayoutClass::from_index(i), Some(*c));
            assert_eq!(c.as_str(), *s);
            assert_eq!(LayoutClass::from_label(s), Some(*c));
        }
        assert_eq!(LayoutClass::Number.group(), RegionGroup::Footer);
        assert_eq!(LayoutClass::InlineFormula.group(), RegionGroup::Formula);
    }

    #[test]
    fn rows_are_filtered_clamped_deduplicated_and_ordered() {
        let rows: Vec<f32> = [
            row(22.0, 0.95, [10.0, 100.0, 500.0, 200.0], 50.0),
            // Same class, overlapping the first: dropped.
            row(22.0, 0.80, [12.0, 102.0, 500.0, 205.0], 51.0),
            // Below the threshold.
            row(21.0, 0.20, [10.0, 300.0, 500.0, 400.0], 60.0),
            // A table straddling the right edge: clamped.
            row(21.0, 0.90, [10.0, 300.0, 900.0, 400.0], 60.0),
            // A different class over the text at IoU < 0.98: kept.
            row(17.0, 0.70, [10.0, 100.0, 500.0, 150.0], 10.0),
            // Unknown class, no area, NaN.
            row(30.0, 0.99, [0.0, 0.0, 50.0, 50.0], 1.0),
            row(3.0, 0.99, [40.0, 40.0, 40.0, 90.0], 1.0),
            row(3.0, f32::NAN, [0.0, 0.0, 50.0, 50.0], 1.0),
        ]
        .concat();
        let got = decode_rows(&rows, 600.0, 800.0, DEFAULT_SCORE_THRESHOLD);
        let classes: Vec<LayoutClass> = got.iter().map(|r| r.class).collect();
        assert_eq!(
            classes,
            [
                LayoutClass::ParagraphTitle,
                LayoutClass::Text,
                LayoutClass::Table
            ]
        );
        assert_eq!(got[2].bbox, [10.0, 300.0, 600.0, 400.0]);
        assert!((got[1].score - 0.95).abs() < 1e-6);
    }

    #[test]
    fn a_near_identical_box_of_another_class_is_dropped() {
        let rows: Vec<f32> = [
            row(14.0, 0.9, [0.0, 0.0, 100.0, 100.0], 1.0),
            row(3.0, 0.8, [0.0, 0.0, 100.0, 99.5], 2.0),
        ]
        .concat();
        let got = decode_rows(&rows, 200.0, 200.0, 0.3);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].class, LayoutClass::Image);
    }
}
