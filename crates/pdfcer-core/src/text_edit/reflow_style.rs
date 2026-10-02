//! Re-emission of a reflowed block glyph by glyph: each glyph's own code
//! under its own show operator's font, size, text state and colours, with
//! the source's intra-word `TJ` positioning kept and justify slack added at
//! the word gaps.
//!
//! State is written only where it changes between consecutive glyphs, and
//! whatever the new body leaves in force differently from what followed the
//! old region is restored before `ET` (decision 019 §3.4, R88).

use std::collections::HashMap;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::text_edit::cause::UnsupportedCause;
use crate::text_extract::font::ExtractFont;
use crate::text_state::TextStateParam;
use crate::view::DocumentView;
use crate::writer::content::{emit_literal_string, emit_number};

use super::edit::{emit_tm, resolve_font_dict, writes_vertically};
use super::model::{EditableTextModel, GlyphRef};
use super::reflow::{BlockAlignment, ReflowLine, ReflowPreview, WordTok};
use super::reflow_apply::{BlockProvenance, ReflowApplyError, origin_to_tm, restore_ops};
use super::reflow_spacing::JustifySpacing;
use super::reflow_walk::{BlockRegion, Paint, SpanStyle};

/// Below this, in points, a displacement is not written as a `TJ` number.
const KERN_EPS: f64 = 0.005;
/// Below this a scale is degenerate (zero-size or zero-scaled text).
const SCALE_EPS: f64 = 1e-6;

/// What the emitter needs to know about one font resource the block uses.
pub(super) struct FontInfo {
    pub(super) base_font: String,
    pub(super) embedded: bool,
    bytes_per_code: usize,
    simple: bool,
}

/// The fonts the block shows, resolved, in first-use order.
pub(super) struct BlockFonts {
    pub(super) order: Vec<Vec<u8>>,
    pub(super) info: HashMap<Vec<u8>, FontInfo>,
}

/// The state a block glyph was shown under.
///
/// # Errors
///
/// [`UnsupportedCause::ShowOperatorsNotFound`] when the walk recorded no
/// state for the glyph's show operator.
pub(super) fn style_of<'r>(
    model: &EditableTextModel<'_>,
    region: &'r BlockRegion,
    gref: GlyphRef,
) -> Result<&'r SpanStyle, ReflowApplyError> {
    let p = model
        .provenance(gref)
        .ok_or(ReflowApplyError::NoProvenance)?;
    region
        .styles
        .get(&p.operator_span.start)
        .ok_or(ReflowApplyError::Unsupported(
            UnsupportedCause::ShowOperatorsNotFound,
        ))
}

/// Resolve every font the block's words show, refusing a vertical one
/// (`/WMode 1`, ISO 32000-2 §9.7.4.3) by name.
pub(super) fn resolve_fonts(
    doc: &DocumentView<'_>,
    resources: &Dict,
    model: &EditableTextModel<'_>,
    region: &BlockRegion,
    words: &[WordTok],
) -> Result<BlockFonts, ReflowApplyError> {
    let mut fonts = BlockFonts {
        order: Vec::new(),
        info: HashMap::new(),
    };
    let glyphs = words
        .iter()
        .flat_map(|w| w.glyphs.iter().chain(w.space_after.iter()));
    for &gref in glyphs {
        let name = &style_of(model, region, gref)?.font;
        if fonts.info.contains_key(name) {
            continue;
        }
        let dict = resolve_font_dict(doc, resources, name).ok_or(ReflowApplyError::Unsupported(
            UnsupportedCause::FontUnresolvable,
        ))?;
        let type0 = dict
            .get(b"Subtype")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"Type0");
        if type0 && writes_vertically(doc, dict) {
            return Err(ReflowApplyError::Unsupported(
                UnsupportedCause::VerticalWriting,
            ));
        }
        let font = ExtractFont::resolve(doc, dict);
        fonts.order.push(name.clone());
        fonts.info.insert(
            name.clone(),
            FontInfo {
                base_font: font.base_font.clone(),
                embedded: font_is_embedded(dict, doc),
                bytes_per_code: font.bytes_per_code(),
                simple: font.is_simple(),
            },
        );
    }
    Ok(fonts)
}

/// Whether the font carries an embedded program (`/FontFile`/`2`/`3`).
fn font_is_embedded(font_dict: &Dict, doc: &DocumentView<'_>) -> bool {
    font_dict
        .get(b"FontDescriptor")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
        .is_some_and(|d| {
            d.contains_key(b"FontFile")
                || d.contains_key(b"FontFile2")
                || d.contains_key(b"FontFile3")
        })
}

/// Everything the emission reads.
pub(super) struct EmitCtx<'r, 'm> {
    pub(super) model: &'r EditableTextModel<'m>,
    pub(super) region: &'r BlockRegion,
    pub(super) fonts: &'r BlockFonts,
    pub(super) prov: &'r BlockProvenance,
    /// The justification spacing the block's styles were set to the base of.
    pub(super) spacing: JustifySpacing,
    /// The block's space glyphs, in content order.
    pub(super) spaces: Vec<GlyphRef>,
    /// The advance assumed for a code-32 space the block never showed.
    pub(super) synthetic_space: f64,
}

/// The new `BT … ET` and what it did.
pub(super) struct Emitted {
    pub(super) body: Vec<u8>,
    pub(super) justified_lines: usize,
    /// A restore was appended before `ET`.
    pub(super) leak_closed: bool,
    /// Word gaps written with a code-32 space the source never showed.
    pub(super) synthetic_spaces: usize,
}

/// One glyph to show, and the displacement (points, along the line) to add
/// after it.
pub(super) struct Item<'r> {
    pub(super) style: &'r SpanStyle,
    pub(super) code: Vec<u8>,
    pub(super) after: f64,
}

/// The state the new body has put in force so far.
pub(super) struct Current {
    font: Option<(Vec<u8>, f64)>,
    vals: [f64; 6],
    set: [bool; 6],
    fill: Paint,
    stroke: Paint,
}

impl Current {
    /// The state in force where the region's first `BT` stood.
    pub(super) fn at_entry(region: &BlockRegion) -> Self {
        let entry = &region.entry;
        Self {
            font: entry.font.clone(),
            vals: TextStateParam::ALL.map(|p| entry.ambient.get(p).value),
            set: [false; 6],
            fill: entry.fill.clone(),
            stroke: entry.stroke.clone(),
        }
    }
}

/// Emit the block's lines per the preview.
///
/// # Errors
///
/// [`UnsupportedCause::NoSpaceGlyph`], a degenerate CTM, or an unrestorable
/// exit state.
pub(super) fn emit_block(
    ctx: &EmitCtx<'_, '_>,
    words: &[WordTok],
    preview: &ReflowPreview,
) -> Result<Emitted, ReflowApplyError> {
    let mut cur = Current::at_entry(ctx.region);
    let mut out = Emitted {
        body: b"BT\n".to_vec(),
        justified_lines: 0,
        leak_closed: false,
        synthetic_spaces: 0,
    };
    let justified = preview.alignment.alignment.is_justified();
    for line in &preview.lines {
        let line_words: Vec<&WordTok> = words_of(words, line).collect();
        let (mut items, gaps, actual) = line_items(ctx, &line_words, &mut out)?;
        let slack = justified_line_slack(line, justified)
            .map(|s| s + line.natural_width - actual)
            .filter(|&s| s > KERN_EPS && !gaps.is_empty());
        if let Some(s) = slack {
            out.justified_lines += 1;
            let per_gap = s / gaps.len() as f64;
            for &g in &gaps {
                if let Some(it) = items.get_mut(g) {
                    it.after += per_gap;
                }
            }
        }
        let shift = match preview.alignment.alignment {
            BlockAlignment::Right => line.natural_width - actual,
            BlockAlignment::Center => (line.natural_width - actual) / 2.0,
            _ => 0.0,
        };
        let p = ctx.prov;
        let (e, f) = origin_to_tm(line.origin_x + shift, line.baseline_y, p)?;
        out.body
            .extend_from_slice(&emit_tm([p.tm_a, p.tm_b, p.tm_c, p.tm_d, e, f]));
        out.body.push(b'\n');
        emit_items(ctx.prov, &items, &mut cur, &mut out.body);
    }
    let restore = restore_bytes(ctx.region, &cur)?;
    out.leak_closed = !restore.is_empty();
    out.body.extend_from_slice(&restore);
    out.body.extend_from_slice(b"ET");
    Ok(out)
}

/// A line's glyph items, the indices of its word-gap items, and its width
/// as emitted (word widths plus gap advances), points.
fn line_items<'r>(
    ctx: &EmitCtx<'r, '_>,
    line_words: &[&WordTok],
    out: &mut Emitted,
) -> Result<(Vec<Item<'r>>, Vec<usize>, f64), ReflowApplyError> {
    let mut items = Vec::new();
    let mut gaps = Vec::new();
    let mut width = 0.0;
    for (wi, w) in line_words.iter().enumerate() {
        width += w.width;
        let mut last_style = None;
        for (j, &gref) in w.glyphs.iter().enumerate() {
            let style = style_of(ctx.model, ctx.region, gref)?;
            let code = ctx.model.glyph(gref).map_or(0, |g| g.code);
            items.push(Item {
                style,
                code: code_bytes(code, bytes_per_code(ctx, style)),
                after: w.kerns.get(j + 1).copied().unwrap_or(0.0),
            });
            last_style = Some(style);
        }
        if wi + 1 == line_words.len() {
            continue;
        }
        let Some(last_style) = last_style else {
            continue;
        };
        let (gap, advance) = gap_item(ctx, w, last_style, out)?;
        width += advance;
        gaps.push(items.len());
        items.push(gap);
    }
    Ok((items, gaps, width))
}

fn bytes_per_code(ctx: &EmitCtx<'_, '_>, style: &SpanStyle) -> usize {
    ctx.fonts
        .info
        .get(&style.font)
        .map_or(1, |f| f.bytes_per_code)
}

/// A character code as show-string bytes, big-endian (§9.4.3, §9.7.6.2).
pub(super) fn code_bytes(code: u32, bytes: usize) -> Vec<u8> {
    let be = code.to_be_bytes();
    be.get(4 - bytes.clamp(1, 4)..).unwrap_or(&be).to_vec()
}

/// The space glyph for the gap after `w`, and its advance: the space that
/// followed `w` in the source; else a block space in the same font; else any
/// block space; else a code-32 space in a single-byte font.
fn gap_item<'r>(
    ctx: &EmitCtx<'r, '_>,
    w: &WordTok,
    last_style: &'r SpanStyle,
    out: &mut Emitted,
) -> Result<(Item<'r>, f64), ReflowApplyError> {
    // A borrowed space prefers the word's own look, so the gap adds no
    // state change; failing that its font, so the code means a space.
    let source = w.space_after.or_else(|| {
        let style = |s: &GlyphRef| style_of(ctx.model, ctx.region, *s).ok();
        ctx.spaces
            .iter()
            .find(|s| style(s).is_some_and(|st| st.same_look(last_style)))
            .or_else(|| {
                ctx.spaces
                    .iter()
                    .find(|s| style(s).is_some_and(|st| st.font == last_style.font))
            })
            .or_else(|| ctx.spaces.first())
            .copied()
    });
    if let Some(gref) = source {
        let style = style_of(ctx.model, ctx.region, gref)?;
        let g = ctx.model.glyph(gref);
        let item = Item {
            style,
            code: code_bytes(g.map_or(32, |g| g.code), bytes_per_code(ctx, style)),
            after: 0.0,
        };
        return Ok((item, ctx.spacing.advance(ctx.model, gref)));
    }
    let info = ctx.fonts.info.get(&last_style.font);
    if info.is_some_and(|f| f.simple) {
        out.synthetic_spaces += 1;
        let item = Item {
            style: last_style,
            code: vec![b' '],
            after: 0.0,
        };
        return Ok((item, ctx.synthetic_space));
    }
    Err(ReflowApplyError::Unsupported(
        UnsupportedCause::NoSpaceGlyph {
            font: info.map(|f| f.base_font.clone()).unwrap_or_default(),
        },
    ))
}

/// Write a line's items as show operators, one per run of identically
/// styled glyphs, with the state changes each run needs before it.
pub(super) fn emit_items(
    prov: &BlockProvenance,
    items: &[Item<'_>],
    cur: &mut Current,
    body: &mut Vec<u8>,
) {
    let mut start = 0;
    while start < items.len() {
        let Some(first) = items.get(start) else { break };
        let len = items
            .get(start..)
            .unwrap_or(&[])
            .iter()
            .take_while(|it| std::ptr::eq(it.style, first.style) || it.style.same_look(first.style))
            .count()
            .max(1);
        set_style(first.style, cur, body);
        show(prov, items.get(start..start + len).unwrap_or(&[]), body);
        start += len;
    }
}

/// Emit the operators that change `cur` to `style`: `Tf`, the five §9.3
/// parameters a show reads (leading is not: no line here uses `T*`), and
/// the fill and stroke colours (§8.6.8).
fn set_style(style: &SpanStyle, cur: &mut Current, body: &mut Vec<u8>) {
    let want = Some((style.font.clone(), style.size));
    if cur.font != want {
        body.push(b'/');
        body.extend_from_slice(&style.font);
        body.push(b' ');
        emit_number(body, style.size);
        body.extend_from_slice(b" Tf\n");
        cur.font = want;
    }
    for (i, p) in TextStateParam::ALL.into_iter().enumerate() {
        if p == TextStateParam::Leading {
            continue;
        }
        let v = style.ambient.get(p).value;
        if cur.vals.get(i).is_some_and(|&c| c != v) {
            emit_number(body, v);
            body.push(b' ');
            body.extend_from_slice(p.operator());
            body.push(b'\n');
            if let (Some(c), Some(s)) = (cur.vals.get_mut(i), cur.set.get_mut(i)) {
                *c = v;
                *s = true;
            }
        }
    }
    if cur.fill != style.fill {
        body.extend_from_slice(&style.fill.set_bytes(false));
        cur.fill = style.fill.clone();
    }
    if cur.stroke != style.stroke {
        body.extend_from_slice(&style.stroke.set_bytes(true));
        cur.stroke = style.stroke.clone();
    }
}

/// Show one run as `(…) Tj`, or as `[(…) N (…)] TJ` when it carries
/// displacements. `N = −d·1000 / (Tfs·Th·a·ca)` converts a user-space
/// displacement `d` to a `TJ` number (§9.4.3, §9.4.4); negative opens.
fn show(prov: &BlockProvenance, run: &[Item<'_>], body: &mut Vec<u8>) {
    let Some(first) = run.first() else { return };
    let s = first.style;
    let th = s.ambient.get(TextStateParam::HorizScale).value / 100.0;
    let scale = s.size * th * prov.tm_a * prov.ctm_a;
    let mut arr = vec![b'['];
    let mut buf = Vec::new();
    let mut numbered = false;
    for it in run {
        buf.extend_from_slice(&it.code);
        if it.after.abs() > KERN_EPS && scale.abs() > SCALE_EPS {
            emit_literal_string(&mut arr, &buf);
            buf.clear();
            arr.push(b' ');
            // A thousandth of a TJ unit is far below a device pixel; the
            // rounding drops the f32 noise of the extracted positions.
            let n = -it.after * 1000.0 / scale;
            emit_number(&mut arr, (n * 1000.0).round() / 1000.0);
            arr.push(b' ');
            numbered = true;
        }
    }
    if !numbered {
        emit_literal_string(body, &buf);
        body.extend_from_slice(b" Tj\n");
        return;
    }
    if !buf.is_empty() {
        emit_literal_string(&mut arr, &buf);
    }
    body.extend_from_slice(&arr);
    body.extend_from_slice(b"] TJ\n");
}

/// The bytes that put the state after the old region back: text state by
/// the R88 ladder, then the colours and the font the new body changed.
pub(super) fn restore_bytes(
    region: &BlockRegion,
    cur: &Current,
) -> Result<Vec<u8>, ReflowApplyError> {
    let emitted: Vec<(TextStateParam, f64)> = TextStateParam::ALL
        .into_iter()
        .zip(cur.vals)
        .zip(cur.set)
        .filter(|&(_, set)| set)
        .map(|(pv, _)| pv)
        .collect();
    let exit = &region.exit;
    let mut out = restore_ops(&emitted, &region.entry.ambient, &exit.ambient).map_err(|e| {
        ReflowApplyError::Unsupported(UnsupportedCause::StateNotRestorable {
            detail: e.to_string(),
        })
    })?;
    if cur.fill != exit.fill {
        out.extend_from_slice(&exit.fill.set_bytes(false));
    }
    if cur.stroke != exit.stroke {
        out.extend_from_slice(&exit.stroke.set_bytes(true));
    }
    if let Some((name, size)) = &exit.font
        && cur.font != exit.font
    {
        out.push(b'/');
        out.extend_from_slice(name);
        out.push(b' ');
        emit_number(&mut out, *size);
        out.extend_from_slice(b" Tf\n");
    }
    Ok(out)
}

/// The justified slack for a full (non-last, multi-word) justified line, as
/// the preview decided it.
fn justified_line_slack(line: &ReflowLine, justified: bool) -> Option<f64> {
    if !justified {
        return None;
    }
    line.justified_slack
        .filter(|&s| s > 0.0 && line.gap_count >= 1)
}

/// The words a preview line spans, panic-free against a stale range.
fn words_of<'w>(words: &'w [WordTok], line: &ReflowLine) -> impl Iterator<Item = &'w WordTok> {
    let lo = line.words.start.min(words.len());
    let hi = line.words.end.min(words.len());
    words.get(lo..hi).unwrap_or(&[]).iter()
}
