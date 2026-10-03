//! Where a rule sits and how thick it is, per font.

use super::{DecorationMetrics, StrikeSource};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

/// AFM's standard-14 underline (`UnderlinePosition -100`,
/// `UnderlineThickness 50`, a centre line; 14 of 14 core AFMs).
const STANDARD_UNDERLINE: (f64, f64) = (-100.0, 50.0);

/// Metrics for the rules of one font, in thousandths of an em, measured to
/// the centre of each line.
#[derive(Debug, Clone, Copy)]
pub(super) struct LineMetrics {
    pub(super) underline_centre: f64,
    pub(super) underline_thickness: f64,
    pub(super) strike_centre: f64,
    pub(super) strike_thickness: f64,
    pub(super) strike_source: StrikeSource,
}

impl LineMetrics {
    /// `FontTables`: the embedded program's `post` underline and `OS/2`
    /// strikeout, each field falling back on its own. `Standard`, and every
    /// fallback: the AFM underline, a strikeout at half `/XHeight`, else a
    /// quarter em, as thick as the underline.
    pub(super) fn for_font(
        view: &DocumentView<'_>,
        font: Option<&Dict>,
        policy: DecorationMetrics,
    ) -> Self {
        let descriptor = font.and_then(|f| descriptor(view, f));
        let tables = match policy {
            DecorationMetrics::FontTables => descriptor
                .and_then(|d| program(view, d))
                .and_then(|p| crate::sfnt::line_metrics(&p)),
            DecorationMetrics::Standard => None,
        }
        .unwrap_or_default();
        let x_height = descriptor
            .and_then(|d| d.get(b"XHeight"))
            .map(|v| view.resolve(v))
            .and_then(Object::as_number)
            .filter(|h| *h > 0.0);
        let underline_thickness = tables.underline_thickness.unwrap_or(STANDARD_UNDERLINE.1);
        let (strike_centre, strike_thickness, strike_source) =
            match (tables.strike_centre, tables.strike_thickness, x_height) {
                (Some(c), Some(t), _) => (c, t, StrikeSource::FontTable),
                (_, _, Some(h)) => (h / 2.0, underline_thickness, StrikeSource::XHeight),
                _ => (250.0, underline_thickness, StrikeSource::QuarterEm),
            };
        Self {
            underline_centre: tables.underline_centre.unwrap_or(STANDARD_UNDERLINE.0),
            underline_thickness,
            strike_centre,
            strike_thickness,
            strike_source,
        }
    }
}

/// The font descriptor of a simple font, or of a `Type0`'s descendant.
fn descriptor<'v>(view: &'v DocumentView<'_>, font: &'v Dict) -> Option<&'v Dict> {
    let font = match font.get(b"DescendantFonts").map(|d| view.resolve(d)) {
        Some(Object::Array(kids)) => view.resolve(kids.first()?).as_dict()?,
        _ => font,
    };
    view.resolve(font.get(b"FontDescriptor")?).as_dict()
}

/// The decoded sfnt program: `/FontFile2`, or `/FontFile3` of subtype
/// `/OpenType` (§9.9 Table 124).
fn program(view: &DocumentView<'_>, descriptor: &Dict) -> Option<Vec<u8>> {
    let stream_at = |key: &[u8]| match descriptor.get(key).map(|o| view.resolve(o)) {
        Some(Object::Stream(s)) => Some(s),
        _ => None,
    };
    let stream = stream_at(b"FontFile2").or_else(|| {
        stream_at(b"FontFile3").filter(|s| {
            s.dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"OpenType")
        })
    })?;
    let raw = view.slice(stream.data_span)?;
    crate::filters::decode_stream(&stream.dict, raw).ok()
}
