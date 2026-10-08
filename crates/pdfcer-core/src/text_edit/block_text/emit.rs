//! The new text object for a planned block (§9.4.2 `Tm`, §9.4.3 `TJ`), the
//! glyph matrices its preview draws, and the report's disclosures.

use crate::content::ContentStream;
use crate::text_edit::edit::block_encode::BlockEncoding;
use crate::text_edit::edit::{EditLayout, TextEditPreview, emit_tm, mat_mul};
use crate::text_edit::model::{BREAK_TAG, TEXT_BLOCK_TAG};
use crate::text_edit::reflow_apply::{ReflowApplyError, origin_to_tm};
use crate::text_edit::reflow_style::{Current, Item, code_bytes, emit_items, restore_bytes};
use crate::text_edit::reflow_walk::SpanStyle;
use crate::text_state::TextStateParam;

use super::BlockEditReport;
use super::layout::{Located, Measure, PlacedLine};

/// What the emitter reads.
pub(super) struct EmitInput<'a> {
    pub(super) stream: &'a ContentStream,
    pub(super) located: &'a Located,
    pub(super) enc: &'a BlockEncoding,
    pub(super) flat: &'a [char],
    pub(super) lines: &'a [PlacedLine],
    pub(super) measure: &'a Measure,
    pub(super) page_index: usize,
}

/// The replacement region body and what writing it found.
pub(super) struct Emitted {
    pub(super) body: Vec<u8>,
    /// `BDC`/`BMC` sequences inside the replaced region (balanced: the
    /// region walk refuses an unbalanced one).
    pub(super) marked_removed: usize,
    /// An R88 restore was appended before `ET`.
    pub(super) leak_closed: bool,
    pub(super) glyphs: TextEditPreview,
}

/// Write one `BT … ET` holding every line: a `Tm` at each line origin, the
/// line's codes in one show run with the justify slack after each word gap,
/// and the text-state restore the replaced region owes the operators after
/// it (R88). The line-end marks go in too: a `pdfc_TextBlock` point after
/// `BT` and a `pdfc_Break` point after each line a typed break ends
/// (ISO 32000-1 §14.6; [`LineMarks`](crate::text_edit::LineMarks)).
pub(super) fn emit_lines(input: &EmitInput<'_>) -> Result<Emitted, ReflowApplyError> {
    let loc = input.located;
    let mut style = loc.style.clone();
    style.font.clone_from(&input.enc.font_resource);
    let bytes = input.enc.font.bytes_per_code();
    let mut cur = Current::at_entry(&loc.region);
    let mut body = b"BT\n/".to_vec();
    body.extend_from_slice(TEXT_BLOCK_TAG);
    body.extend_from_slice(b" MP\n");
    let mut glyphs = Vec::new();
    for line in input.lines {
        let (e, f) = origin_to_tm(line.origin_x, line.baseline_y, &loc.prov)?;
        let p = &loc.prov;
        let tm = [p.tm_a, p.tm_b, p.tm_c, p.tm_d, e, f];
        body.extend_from_slice(&emit_tm(tm));
        body.push(b'\n');
        let items = line_items(input, line, &style, bytes);
        emit_items(&loc.prov, &items, &mut cur, &mut body);
        if line.typed_break {
            body.extend_from_slice(b"\n/");
            body.extend_from_slice(BREAK_TAG);
            body.extend_from_slice(b" MP\n");
        }
        glyph_matrices(input, line, &style, tm, &mut glyphs);
    }
    let restore = restore_bytes(&loc.region, &cur)?;
    let leak_closed = !restore.is_empty();
    body.extend_from_slice(&restore);
    body.extend_from_slice(b"ET");
    let layout = EditLayout {
        font_name: style.font.clone(),
        font_dict: input.enc.font_dict.clone(),
        base_font: input.enc.font.base_font.clone(),
        glyphs,
        bbox: [0.0; 4],
        fill: input.enc.anchor.fill_color.clone(),
        stroke: input.enc.anchor.stroke_color.clone(),
        render_mode: style.ambient.get(TextStateParam::RenderMode).value,
        fallback: None,
    };
    Ok(Emitted {
        body,
        marked_removed: marked_in(input.stream, loc.region.start, loc.region.end),
        leak_closed,
        glyphs: TextEditPreview::new(
            input.page_index,
            layout,
            Vec::new(),
            None,
            input.enc.program.clone(),
        ),
    })
}

/// The line's codes, word gaps (the encoded space) carrying the slack.
fn line_items<'s>(
    input: &EmitInput<'_>,
    line: &PlacedLine,
    style: &'s SpanStyle,
    bytes: usize,
) -> Vec<Item<'s>> {
    let mut items = Vec::new();
    for (wi, r) in line.words.iter().enumerate() {
        let end = if wi + 1 < line.words.len() {
            r.end + 1
        } else {
            r.end
        };
        for i in r.start..end {
            let code = input.enc.codes.get(i).copied().unwrap_or(0);
            items.push(Item {
                style,
                code: code_bytes(code, bytes),
                after: if i == r.end { line.per_gap } else { 0.0 },
            });
        }
    }
    items
}

/// One matrix per code: `[Tfs·Th 0 0 Tfs tx Trise] × Tm × CTM` (§9.4.4),
/// `tx` the text-space advance so far plus the slack shifts.
fn glyph_matrices(
    input: &EmitInput<'_>,
    line: &PlacedLine,
    style: &SpanStyle,
    tm: [f64; 6],
    out: &mut Vec<(Option<char>, u32, [f64; 6])>,
) {
    let p = &input.located.prov;
    let ctm = [p.ctm_a, 0.0, 0.0, p.ctm_d, p.ctm_e, p.ctm_f];
    let base = mat_mul(tm, ctm);
    let th = style.ambient.get(TextStateParam::HorizScale).value / 100.0;
    let rise = style.ambient.get(TextStateParam::Rise).value;
    let scale = input.measure.scale;
    let mut tx = 0.0;
    for (wi, r) in line.words.iter().enumerate() {
        let end = if wi + 1 < line.words.len() {
            r.end + 1
        } else {
            r.end
        };
        for i in r.start..end {
            let code = input.enc.codes.get(i).copied().unwrap_or(0);
            let glyph = [style.size * th, 0.0, 0.0, style.size, tx, rise];
            out.push((input.flat.get(i).copied(), code, mat_mul(glyph, base)));
            tx += input.measure.advances.get(i).copied().unwrap_or(0.0);
            if i == r.end && scale.abs() > f64::EPSILON {
                tx += line.per_gap / scale;
            }
        }
    }
}

/// `BDC`/`BMC` operators starting inside `[start, end)`.
fn marked_in(stream: &ContentStream, start: usize, end: usize) -> usize {
    stream
        .operations()
        .filter(|op| (start..end).contains(&op.operator.span.start))
        .filter(|op| matches!(op.operator_name(&stream.buf), Some(b"BDC" | b"BMC")))
        .count()
}

/// Every sentence the report discloses (rule 4: the layout, any font
/// change, glyphs added, lost tagging and the overflow are all stated).
pub(super) fn disclosures(
    report: &BlockEditReport,
    enc: &BlockEncoding,
    loc: &Located,
    collapsed: bool,
    leak_closed: bool,
) -> Vec<String> {
    let mut out = enc.disclosures.clone();
    out.push(format!(
        "block {} replaced and wrapped from {} to {} line(s) at {:.1} pt, \
         {:?}-aligned, in {}: the line breaks are derived",
        report.block_index,
        report.lines_before,
        report.lines_after,
        report.wrap_width,
        report.alignment,
        report.base_font,
    ));
    if let Some(from) = &report.font_substituted_from {
        out.push(format!(
            "the text is set in {}, a same-face sibling of the block's own {from}",
            report.base_font
        ));
    }
    if loc.looks > 1 {
        out.push(format!(
            "the block mixed {} looks; the new text takes the first run's font, size and colour",
            loc.looks
        ));
    }
    if collapsed {
        out.push("runs of spaces and tabs were collapsed to single word gaps".to_owned());
    }
    out.extend(overflow_sentences(report));
    if let Some(mcid) = report.tagged_mcid {
        out.push(format!(
            "the block is tagged (MCID {mcid}); any /ActualText or /Alt on its structure element now describes the old text"
        ));
    }
    if report.marked_content_removed > 0 {
        out.push(format!(
            "{} marked-content sequence(s) between the block's lines were removed with them",
            report.marked_content_removed
        ));
    }
    out.extend(loc.spacing_note.clone());
    if leak_closed {
        out.push("text state the new lines set is restored before the text object ends".to_owned());
    }
    if report.extra_objects_emptied > 0 {
        out.push(format!(
            "the page's {} further content stream(s) were folded into the first and emptied",
            report.extra_objects_emptied
        ));
    }
    out
}

fn overflow_sentences(report: &BlockEditReport) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(pt) = report.overflow_pt {
        out.push(format!(
            "the new text runs {pt:.1} pt below the block's original bottom and may overlap what is beneath it"
        ));
    }
    if let Some(po) = report.page_overflow {
        out.push(format!(
            "the block now extends past the page: {:.1} pt below, {:.1} pt right, {} line(s) outside",
            po.past_bottom_pt, po.past_right_pt, po.lines_outside
        ));
    }
    out
}
