//! Justification spacing in a reflowed block: a `Tw` or `Tc` that differs
//! between the block's source lines stretched those lines to the margin, and
//! is a property of the old line breaks rather than of the words.
//!
//! A re-wrap sets it to the value the last source line used (the paragraph's
//! natural spacing), measures every advance as if shown at that value, and
//! re-justifies the new lines by gap displacement, so the new last line is
//! ragged at natural spacing. A `Tw`/`Tc` the same on every line is a style
//! and is kept.

use crate::text_state::TextStateParam;

use super::model::{Block, EditableTextModel, GlyphRef};
use super::reflow_walk::BlockRegion;

/// Below this, two spacing values are the same.
const SPACING_EPS: f64 = 1e-6;

/// Per parameter, the base value the block's justification spacing is set
/// to, or `None` when the parameter is the same on every source line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct JustifySpacing {
    /// `Tw` (§9.3.3).
    pub(crate) tw: Option<f64>,
    /// `Tc` (§9.3.2).
    pub(crate) tc: Option<f64>,
    /// The spread of the stripped values, `(min, max)`, per parameter.
    tw_range: (f64, f64),
    tc_range: (f64, f64),
}

impl JustifySpacing {
    /// Detect the block's justification spacing from its glyphs' provenance.
    pub(crate) fn detect(model: &EditableTextModel<'_>, block: &Block) -> Self {
        let mut per_line: Vec<(f64, f64)> = Vec::new();
        for &li in &block.line_indices {
            let Some(line) = model.lines().get(li) else {
                continue;
            };
            for &gref in &line.glyphs {
                if let Some(p) = model.provenance(gref) {
                    per_line.push((
                        p.text_state.get(TextStateParam::WordSpacing).value,
                        p.text_state.get(TextStateParam::CharSpacing).value,
                    ));
                }
            }
        }
        let last_line_first = block
            .line_indices
            .last()
            .and_then(|&li| model.lines().get(li))
            .and_then(|l| l.glyphs.first())
            .and_then(|&g| model.provenance(g))
            .map(|p| {
                (
                    p.text_state.get(TextStateParam::WordSpacing).value,
                    p.text_state.get(TextStateParam::CharSpacing).value,
                )
            });
        let Some((tw_base, tc_base)) = last_line_first else {
            return Self::default();
        };
        let tw = spread(per_line.iter().map(|v| v.0));
        let tc = spread(per_line.iter().map(|v| v.1));
        Self {
            tw: (tw.1 - tw.0 > SPACING_EPS).then_some(tw_base),
            tc: (tc.1 - tc.0 > SPACING_EPS).then_some(tc_base),
            tw_range: tw,
            tc_range: tc,
        }
    }

    /// `gref`'s advance, points, as shown at the base spacing:
    /// `tx = ((w0·Tfs) + Tc + Tw)·Th` (§9.4.4) less the stripped `Tc` and,
    /// for a single-byte code 32 only (§9.3.3), the stripped `Tw`, scaled by
    /// the text matrix and CTM into user space.
    pub(crate) fn advance(&self, model: &EditableTextModel<'_>, gref: GlyphRef) -> f64 {
        let Some(g) = model.glyph(gref) else {
            return 0.0;
        };
        let advance = f64::from(g.advance);
        let Some(p) = g.provenance.as_ref() else {
            return advance;
        };
        let ts = &p.text_state;
        let mut delta = 0.0;
        if let Some(base) = self.tc {
            delta += ts.get(TextStateParam::CharSpacing).value - base;
        }
        if let Some(base) = self.tw
            && g.code == 32
            && !p.composite
        {
            delta += ts.get(TextStateParam::WordSpacing).value - base;
        }
        if delta == 0.0 {
            return advance;
        }
        let th = ts.get(TextStateParam::HorizScale).value / 100.0;
        let [a, b, ..] = p.text_matrix.map(f64::from);
        let [ca, cb, cc, cd, ..] = p.ctm.map(f64::from);
        let unit = (a * ca + b * cc).hypot(a * cb + b * cd);
        advance - delta * th * unit
    }

    /// Set the stripped parameters to their base in every block style, so
    /// the emission writes the base value.
    pub(super) fn apply_to(&self, region: &mut BlockRegion) {
        for style in region.styles.values_mut() {
            for (param, base) in [
                (TextStateParam::WordSpacing, self.tw),
                (TextStateParam::CharSpacing, self.tc),
            ] {
                if let Some(v) = base {
                    let mut raw = Vec::new();
                    crate::writer::content::emit_number(&mut raw, v);
                    style.ambient.set(param, v, &raw);
                }
            }
        }
    }

    /// The disclosure naming what was stripped, if anything was.
    pub(crate) fn disclosure(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(base) = self.tw {
            parts.push(format!(
                "word spacing (Tw {:.3} to {:.3}) set to {base:.3}",
                self.tw_range.0, self.tw_range.1
            ));
        }
        if let Some(base) = self.tc {
            parts.push(format!(
                "character spacing (Tc {:.3} to {:.3}) set to {base:.3}",
                self.tc_range.0, self.tc_range.1
            ));
        }
        (!parts.is_empty()).then(|| {
            format!(
                "reflow: the source lines were stretched to the margin by per-line {}, the last \
                 source line's value; the new lines are re-justified by gap displacement \
                 (§9.4.3) and the new last line keeps that natural spacing",
                parts.join(" and "),
            )
        })
    }
}

/// `(min, max)` of `values`, or `(0, 0)` when empty.
fn spread(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let (lo, hi) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) }
}
