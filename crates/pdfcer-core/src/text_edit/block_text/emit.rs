//! The new text object for a planned block (§9.4.2 `Tm`, §9.4.3 `TJ`), the
//! glyph matrices its preview draws, and the report's disclosures.

use crate::content::ContentStream;
use crate::text_edit::edit::{EditLayout, TextEditPreview, emit_tm, mat_mul};
use crate::text_edit::model::{BREAK_TAG, TEXT_BLOCK_TAG};
use crate::text_edit::reflow_apply::{ReflowApplyError, origin_to_tm};
use crate::text_edit::reflow_style::{Current, Item, code_bytes, emit_items, restore_bytes};
use crate::text_state::TextStateParam;

use super::BlockEditReport;
use super::encode::Encoded;
use super::layout::{Located, Measure, PlacedLine};

/// What the emitter reads.
pub(super) struct EmitInput<'a> {
    pub(super) stream: &'a ContentStream,
    pub(super) located: &'a Located,
    pub(super) enc: &'a Encoded,
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
    /// One preview per look the new text uses, in look order.
    pub(super) runs: Vec<TextEditPreview>,
}

/// A positioned glyph: its character, code and glyph-space-to-page matrix.
type Placed = (Option<char>, u32, [f64; 6]);

/// Write one `BT … ET` holding every line: a `Tm` at each line origin, the
/// line's codes in one show run with the justify slack after each word gap,
/// and the text-state restore the replaced region owes the operators after
/// it (R88). The line-end marks go in too: a `pdfc_TextBlock` point after
/// `BT` and a `pdfc_Break` point after each line a typed break ends
/// (ISO 32000-1 §14.6; [`LineMarks`](crate::text_edit::LineMarks)).
pub(super) fn emit_lines(input: &EmitInput<'_>) -> Result<Emitted, ReflowApplyError> {
    let loc = input.located;
    let mut cur = Current::at_entry(&loc.region);
    let mut body = b"BT\n/".to_vec();
    body.extend_from_slice(TEXT_BLOCK_TAG);
    body.extend_from_slice(b" MP\n");
    let mut glyphs: Vec<Vec<Placed>> = vec![Vec::new(); input.enc.emit_styles.len()];
    for line in input.lines {
        let (e, f) = origin_to_tm(line.origin_x, line.baseline_y, &loc.prov)?;
        let p = &loc.prov;
        let tm = [p.tm_a, p.tm_b, p.tm_c, p.tm_d, e, f];
        body.extend_from_slice(&emit_tm(tm));
        body.push(b'\n');
        let items = line_items(input, line);
        emit_items(&loc.prov, &items, &mut cur, &mut body);
        if line.typed_break {
            body.extend_from_slice(b"\n/");
            body.extend_from_slice(BREAK_TAG);
            body.extend_from_slice(b" MP\n");
        }
        glyph_matrices(input, line, tm, &mut glyphs);
    }
    let restore = restore_bytes(&loc.region, &cur)?;
    let leak_closed = !restore.is_empty();
    body.extend_from_slice(&restore);
    body.extend_from_slice(b"ET");
    Ok(Emitted {
        body,
        marked_removed: marked_in(input.stream, loc.region.start, loc.region.end),
        leak_closed,
        runs: previews(input, glyphs),
    })
}

/// One preview per look with glyphs: its font, colours and render mode.
fn previews(input: &EmitInput<'_>, glyphs: Vec<Vec<Placed>>) -> Vec<TextEditPreview> {
    let enc = input.enc;
    let mut out = Vec::new();
    for (look, glyphs) in glyphs.into_iter().enumerate() {
        let font = enc.look_font.get(look).and_then(|&f| enc.fonts.get(f));
        let (Some(style), Some(font), Some(show)) =
            (enc.emit_styles.get(look), font, enc.shows.get(look))
        else {
            continue;
        };
        if glyphs.is_empty() {
            continue;
        }
        let layout = EditLayout {
            font_name: style.font.clone(),
            font_dict: font.font_dict.clone(),
            base_font: font.font.base_font.clone(),
            glyphs,
            bbox: [0.0; 4],
            fill: show.fill_color.clone(),
            stroke: show.stroke_color.clone(),
            render_mode: style.ambient.get(TextStateParam::RenderMode).value,
            fallback: None,
        };
        let program = font.program.clone();
        out.push(TextEditPreview::new(
            input.page_index,
            layout,
            Vec::new(),
            None,
            program,
        ));
    }
    out
}

/// The line's codes, word gaps (the encoded space) carrying the slack.
fn line_items<'s>(input: &EmitInput<'s>, line: &PlacedLine) -> Vec<Item<'s>> {
    let mut items = Vec::new();
    for (wi, r) in line.words.iter().enumerate() {
        let end = if wi + 1 < line.words.len() {
            r.end + 1
        } else {
            r.end
        };
        for i in r.start..end {
            let code = input.enc.codes.get(i).copied().unwrap_or(0);
            let (look, font) = input.enc.at(i);
            let (Some(style), Some(font)) = (input.enc.emit_styles.get(look), font) else {
                continue;
            };
            let bytes = font.font.bytes_per_code();
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
fn glyph_matrices(input: &EmitInput<'_>, line: &PlacedLine, tm: [f64; 6], out: &mut [Vec<Placed>]) {
    let p = &input.located.prov;
    let ctm = [p.ctm_a, 0.0, 0.0, p.ctm_d, p.ctm_e, p.ctm_f];
    let base = mat_mul(tm, ctm);
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
            let (look, _) = input.enc.at(i);
            if let (Some(style), Some(run)) = (input.enc.emit_styles.get(look), out.get_mut(look)) {
                let th = style.ambient.get(TextStateParam::HorizScale).value / 100.0;
                let rise = style.ambient.get(TextStateParam::Rise).value;
                let glyph = [style.size * th, 0.0, 0.0, style.size, tx, rise];
                run.push((input.flat.get(i).copied(), code, mat_mul(glyph, base)));
            }
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
    enc: &Encoded,
    loc: &Located,
    collapsed: bool,
    leak_closed: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in enc.fonts.iter().flat_map(|f| f.disclosures.iter()) {
        if !out.contains(d) {
            out.push(d.clone());
        }
    }
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
    out.extend(look_sentences(enc, loc));
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

/// How the block's looks carried over: kept per character, or flattened to
/// the first run's with the reason.
fn look_sentences(enc: &Encoded, loc: &Located) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(why) = &enc.fell_back {
        out.push(format!(
            "the block mixed {} looks, but they could not all set the new text ({why}); \
             the new text takes the first run's font, size and colour",
            loc.looks
        ));
    } else if enc.looks.styles.len() > 1 {
        out.push(format!(
            "the block's {} looks are kept: each surviving character keeps its own, and {} \
             new character(s) take the look of the character before them (after, at the start)",
            enc.looks.styles.len(),
            enc.looks.inherited
        ));
        for f in enc.fonts.iter().skip(1) {
            if let Some(from) = &f.substituted_from {
                out.push(format!(
                    "part of the text is set in {}, a same-face sibling of the block's own {from}",
                    f.font.base_font
                ));
            }
        }
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
