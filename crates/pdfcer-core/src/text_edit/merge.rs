//! Merge adjacent text runs of one text object into a single show operator —
//! the inverse of [`split_text_object`](crate::edit::EditSession::split_text_object)
//! at the run level (`G035`).
//!
//! # Contract
//!
//! The first run's show operator is replaced by one operator carrying every
//! listed run's character codes in order, with an optional separator encoded in
//! the same font. The later runs' show operators are removed; **every other
//! operator between them is left in place**, so the text state and positioning
//! that later content relied on are unchanged. That is safe because `Tm`, `Td`,
//! `TD` and `T*` depend on the line matrix, which a show operator does not move
//! (ISO 32000-1 §9.4.2); the one exception, a following run with no positioning
//! of its own, is refused before planning
//! ([`VectorEditError::MergeWouldMoveNextRun`](crate::vector::VectorEditError::MergeWouldMoveNextRun)).
//!
//! The merged run is shown with the first run's state. Every run must share its
//! font resource and size, `Tc`, `Tw`, `Ts`, `Tr`, fill colour (and stroke
//! colour for a stroking render mode) and marked-content sequence; anything
//! else is refused by name, because showing one run's codes in another's state
//! would change how they draw.
//!
//! `Tz` may differ. Under [`MergeFit::Span`], the default, `Tz` is set so the
//! merged run spans from the first run's origin to the last run's end along the
//! first run's baseline: the §9.4.4 advance is linear in `Th`, so
//! `Tz = 100 × extent ÷ advance(Th = 1)`. This is what an OCR layer needs, where
//! every word carries its own fitted `Tz`. [`MergeFit::Natural`] keeps the first
//! run's `Tz`.

use crate::content::ContentStream;
use crate::span::ByteSpan;
use crate::text_edit::edit::{
    EditPlanTarget, OpRec, Rec, ShowData, ShowElem, ShowOp, Walk, emit_show, glyph_advance_with,
    resolve_font_dict, splice,
};
use crate::text_edit::encoding::InverseEncoding;
use crate::text_edit::format::FormatError;
use crate::text_extract::font::ExtractFont;
use crate::view::DocumentView;
use crate::writer::content::emit_number;
use std::collections::BTreeSet;

/// Tolerance for comparing text-state operands and matrix determinants.
const EPS: f64 = 1e-9;

/// What goes between two merged runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeSeparator {
    /// Nothing: `Inv` + `oice` becomes `Invoice`.
    #[default]
    None,
    /// One space character.
    Space,
    /// This text, encoded in the runs' font. A character the font cannot show
    /// is refused, as it is for [`edit_text`](crate::edit::EditSession::edit_text).
    Text(String),
}

impl MergeSeparator {
    fn as_str(&self) -> &str {
        match self {
            Self::None => "",
            Self::Space => " ",
            Self::Text(s) => s,
        }
    }
}

/// How the merged run's width is decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeFit {
    /// Set `Tz` so the merged run covers the same extent as the runs it
    /// replaces: from the first run's origin to the last run's end.
    #[default]
    Span,
    /// Keep the first run's `Tz`; the merged run takes its natural width.
    Natural,
}

/// Options for [`EditSession::merge_text_runs`](crate::edit::EditSession::merge_text_runs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MergeOptions {
    /// What goes between each pair of runs.
    pub separator: MergeSeparator,
    /// How the merged run's width is decided.
    pub fit: MergeFit,
}

impl MergeOptions {
    /// Set the separator.
    #[must_use]
    pub fn separator(mut self, separator: MergeSeparator) -> Self {
        self.separator = separator;
        self
    }

    /// Set the width policy.
    #[must_use]
    pub const fn fit(mut self, fit: MergeFit) -> Self {
        self.fit = fit;
        self
    }
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MergeReport {
    /// The merged run's decoded text.
    pub text: String,
    /// How many runs became one.
    pub runs_merged: usize,
    /// `(before, after)` horizontal scaling percentages when `Tz` was set.
    pub h_scale_change: Option<(f64, f64)>,
    /// Operator-facing disclosures.
    pub disclosures: Vec<String>,
}

/// A planned merge: the new content buffer and its report.
pub(crate) struct MergePlan {
    pub(crate) new_content: Vec<u8>,
    pub(crate) report: MergeReport,
}

/// Plan a merge of the show operators whose byte spans are `spans` (in content
/// order, already checked contiguous and in range by
/// [`text_merge_refusal`](crate::vector::text_merge_refusal)).
pub(crate) fn plan_merge(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    stream: &ContentStream,
    spans: &[ByteSpan],
    opts: &MergeOptions,
) -> Result<MergePlan, FormatError> {
    let mut walk = Walk::new(doc, &target.resources);
    for op in stream.operations() {
        walk.operation(&op, &stream.buf);
    }
    let recs = walk.recs;

    // Each span names exactly one show operator.
    let mut idx: Vec<usize> = Vec::with_capacity(spans.len());
    for span in spans {
        let i = recs
            .iter()
            .position(|r| {
                matches!(r.rec, Rec::Show(_)) && span.end() == r.end && span.start >= r.start
            })
            .ok_or(FormatError::PinnedSpanNotFound {
                start: span.start,
                end: span.end(),
            })?;
        idx.push(i);
    }
    let shows: Vec<(&OpRec, &ShowData)> = idx
        .iter()
        .filter_map(|&i| match recs.get(i) {
            Some(
                r @ OpRec {
                    rec: Rec::Show(s), ..
                },
            ) => Some((r, s.as_ref())),
            _ => None,
        })
        .collect();
    let (Some(&(first_rec, first)), Some(&(last_rec, last))) = (shows.first(), shows.last()) else {
        return Err(FormatError::Unsupported(
            "a merge needs at least two runs".to_owned(),
        ));
    };

    // No other show operator, and no BT/ET, between consecutive runs.
    for pair in idx.windows(2) {
        let (a, b) = match pair {
            [a, b] => (*a, *b),
            _ => continue,
        };
        let between = recs.get(a + 1..b).unwrap_or(&[]);
        if b <= a
            || between
                .iter()
                .any(|r| matches!(r.rec, Rec::Show(_) | Rec::Boundary | Rec::EndText))
        {
            return Err(FormatError::Unsupported(
                "the runs to merge are not consecutive show operators of one text object"
                    .to_owned(),
            ));
        }
    }

    // Marked content: a BMC/BDC/EMC between the first and last run means the
    // runs belong to different sequences.
    for op in stream.operations() {
        let at = op.operator.span.start;
        if at > first_rec.end
            && at < last_rec.start
            && matches!(
                op.operator_name(&stream.buf),
                // ui-text-exempt: PDF operator keywords, §14.6 Table 320.
                Some(b"BMC" | b"BDC" | b"EMC")
            )
        {
            return Err(FormatError::MergeCrossesMarkedContent);
        }
    }

    for (n, (_, s)) in shows.iter().enumerate() {
        if matches!(s.op, ShowOp::Quote | ShowOp::DoubleQuote) {
            return Err(FormatError::MergeLineShowOperator { run: n });
        }
        if let Some(parameter) = state_difference(first, s) {
            return Err(FormatError::MergeStateDiffers { run: n, parameter });
        }
    }

    let font_dict =
        resolve_font_dict(doc, &target.resources, &first.font_name).ok_or_else(|| {
            FormatError::Unsupported("the runs' font resource is unresolvable".to_owned())
        })?;
    let font = ExtractFont::resolve(doc, font_dict);
    if !font.is_simple() {
        return Err(FormatError::MergeCompositeFont {
            base_font: font.base_font.clone(),
        });
    }

    // Encode the separator in the runs' font.
    let sep_text = opts.separator.as_str();
    let sep_codes: Vec<u8> = if sep_text.is_empty() {
        Vec::new()
    } else {
        let names = font.glyph_names().ok_or_else(|| {
            FormatError::Unsupported("the runs' font has no invertible encoding".to_owned())
        })?;
        let inverse = InverseEncoding::build(&font.base_font, names);
        let prefer: BTreeSet<u8> = shows
            .iter()
            .flat_map(|(_, s)| s.slots.iter())
            .filter_map(|slot| u8::try_from(slot.code).ok())
            .collect();
        inverse
            .encode_str(sep_text, &prefer)
            .map_err(FormatError::Refused)?
            .codes
    };

    // The merged element list, adjacent strings coalesced.
    let mut elems: Vec<ShowElem> = Vec::new();
    let mut text = String::new();
    for (n, (_, s)) in shows.iter().enumerate() {
        if n > 0 && !sep_codes.is_empty() {
            push_str(&mut elems, &sep_codes);
            text.push_str(sep_text);
        }
        for e in &s.elems {
            match e {
                ShowElem::Str(b) => push_str(&mut elems, b),
                ShowElem::Num(v) => elems.push(ShowElem::Num(*v)),
            }
        }
        text.push_str(&s.text);
    }

    let ambient_pct = first.text_state.h_scale.value;
    let new_pct = match opts.fit {
        MergeFit::Natural => None,
        MergeFit::Span => {
            if !shows.iter().all(|(_, s)| s.matrix_known) {
                return Err(FormatError::MergePositionUnknown);
            }
            let extent = span_extent(&font, first, last).ok_or(FormatError::MergeRunsOutOfOrder)?;
            let natural = advance(&font, first, &elems, 1.0);
            if !(natural.is_finite() && natural > EPS) {
                return Err(FormatError::NoAdvanceWidth {
                    base_font: font.base_font.clone(),
                });
            }
            let pct = round6(100.0 * extent / natural);
            ((pct - ambient_pct).abs() > 1e-6).then_some(pct)
        }
    };

    // Build the replacement for the first run; remove the others.
    let mut first_bytes = Vec::new();
    if let Some(pct) = new_pct {
        emit_number(&mut first_bytes, pct);
        first_bytes.extend_from_slice(b" Tz ");
    }
    first_bytes.extend_from_slice(&emit_show(&elems));
    if new_pct.is_some() {
        first_bytes.push(b' ');
        emit_number(&mut first_bytes, ambient_pct);
        first_bytes.extend_from_slice(b" Tz");
    }
    let mut edits: Vec<(usize, usize, Vec<u8>)> = shows
        .iter()
        .enumerate()
        .map(|(n, (r, _))| {
            let bytes = if n == 0 {
                first_bytes.clone()
            } else {
                Vec::new()
            };
            (r.start, r.end, bytes)
        })
        .collect();
    let new_content = splice(&stream.buf, &mut edits);

    let mut disclosures = vec![format!(
        "{} text runs were merged into one: \"{text}\".",
        shows.len()
    )];
    if let Some(pct) = new_pct {
        disclosures.push(format!(
            "Its horizontal scaling was set to {pct:.2}% so it spans the same width as the runs \
             it replaces."
        ));
    }
    Ok(MergePlan {
        new_content,
        report: MergeReport {
            text,
            runs_merged: shows.len(),
            h_scale_change: new_pct.map(|p| (ambient_pct, p)),
            disclosures,
        },
    })
}

/// Append `bytes` to the last string element, or start a new one.
fn push_str(elems: &mut Vec<ShowElem>, bytes: &[u8]) {
    if let Some(ShowElem::Str(s)) = elems.last_mut() {
        s.extend_from_slice(bytes);
    } else {
        elems.push(ShowElem::Str(bytes.to_vec()));
    }
}

/// The first text-state parameter in which `s` differs from `first`, if any.
fn state_difference(first: &ShowData, s: &ShowData) -> Option<&'static str> {
    let a = &first.text_state;
    let b = &s.text_state;
    let differs = |x: f64, y: f64| (x - y).abs() > EPS;
    if first.font_name != s.font_name {
        return Some("font");
    }
    if differs(first.tf_size, s.tf_size) {
        return Some("font size");
    }
    if differs(a.char_spacing.value, b.char_spacing.value) {
        return Some("character spacing (Tc)");
    }
    if differs(a.word_spacing.value, b.word_spacing.value) {
        return Some("word spacing (Tw)");
    }
    if differs(a.rise.value, b.rise.value) {
        return Some("rise (Ts)");
    }
    if differs(a.render_mode.value, b.render_mode.value) {
        return Some("render mode (Tr)");
    }
    if first.fill_color != s.fill_color {
        return Some("fill colour");
    }
    // Stroke colour only shows in modes 1, 2, 5 and 6 (§9.3.6 Table 106).
    let strokes = matches!(a.render_mode.value.round() as i64, 1 | 2 | 5 | 6);
    if strokes && first.stroke_color != s.stroke_color {
        return Some("stroke colour");
    }
    if first.mcid != s.mcid {
        return Some("marked-content sequence");
    }
    None
}

/// The §9.4.4 advance of `elems` in `first`'s state at horizontal scaling `th`.
fn advance(font: &ExtractFont, first: &ShowData, elems: &[ShowElem], th: f64) -> f64 {
    elems
        .iter()
        .map(|e| match e {
            ShowElem::Str(b) => b
                .iter()
                .map(|&c| {
                    glyph_advance_with(
                        font,
                        u32::from(c),
                        first.tf_size,
                        first.tc(),
                        first.tw(),
                        th,
                        true,
                    )
                })
                .sum::<f64>(),
            ShowElem::Num(n) => -n / 1000.0 * first.tf_size * th,
        })
        .sum()
}

/// The distance from `first`'s origin to the end of `last`, measured along
/// `first`'s baseline in its text space. `None` when it is not positive or
/// `first`'s text matrix is singular.
fn span_extent(font: &ExtractFont, first: &ShowData, last: &ShowData) -> Option<f64> {
    let [a, b, c, d, e0, f0] = first.text_matrix;
    let [la, lb, _, _, le, lf] = last.text_matrix;
    let last_adv = advance(font, last, &last.elems, last.th());
    // End of the last run in user space: its Tm applied to (advance, 0).
    let (ex, ey) = (la.mul_add(last_adv, le), lb.mul_add(last_adv, lf));
    let (dx, dy) = (ex - e0, ey - f0);
    let det = a.mul_add(d, -(b * c));
    if det.abs() < EPS {
        return None;
    }
    // Row-vector convention (§8.3.4): (t, s)·[a b; c d] = (dx, dy).
    let t = (dx * d - dy * c) / det;
    (t.is_finite() && t > EPS).then_some(t)
}

/// Round a derived operand to six decimal places.
fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}
