//! The shared plan: locate the block, encode the new text in its first run's
//! font, measure and pack it, and lay the lines out in the block's frame.

use crate::content::ContentStream;
use crate::linebreak::greedy_pack;
use crate::page_tree::{self, Page, Rect};
use crate::text_edit::cause::UnsupportedCause;
use crate::text_extract::{self, ExtractOptions};
use crate::text_state::TextStateParam;
use crate::view::DocumentView;

use super::emit::{EmitInput, emit_lines};
use super::{BlockEditError, BlockEditLine, BlockEditOptions, BlockEditPreview, BlockEditReport};
use crate::text_edit::edit::block_encode::{BlockEncoding, encode_block};
use crate::text_edit::edit::{EditPlanTarget, FontWrites, glyph_advance_with, walk_records};
use crate::text_edit::model::{Block, EditableTextModel};
use crate::text_edit::reflow::{
    BlockAlignment, PageOverflow, ReflowEngine, ReflowPreview, ReflowRequest, align_origin_x,
    line_natural_width, reflow_recognition_options,
};
use crate::text_edit::reflow_apply::{BlockProvenance, block_provenance, table_error};
use crate::text_edit::reflow_spacing::JustifySpacing;
use crate::text_edit::reflow_style::style_of;
use crate::text_edit::reflow_walk::{BlockRegion, SpanStyle, locate_block_region};

/// Below this, in points, an indent or overflow is rounding.
const EPS: f64 = 0.01;

/// A planned block-text edit: the page's new content, the font objects the
/// encoding writes, and the preview the commit reports from.
pub(crate) struct BlockTextPlan {
    pub(crate) new_content: Vec<u8>,
    pub(crate) font_writes: FontWrites,
    pub(crate) preview: BlockEditPreview,
}

/// The new text split into paragraphs of words, and the flat string the
/// encoder sees: every word once, separated by single spaces.
struct Words {
    paragraphs: Vec<Vec<std::ops::Range<usize>>>,
    flat: Vec<char>,
    collapsed: bool,
}

impl Words {
    fn split(text: &str) -> Self {
        let mut flat: Vec<char> = Vec::new();
        let mut paragraphs = Vec::new();
        for para in text.split('\n') {
            let mut words = Vec::new();
            for w in para.split_whitespace() {
                if !flat.is_empty() {
                    flat.push(' ');
                }
                let start = flat.len();
                flat.extend(w.chars());
                words.push(start..flat.len());
            }
            paragraphs.push(words);
        }
        let rebuilt: Vec<String> = paragraphs
            .iter()
            .map(|p| {
                p.iter()
                    .map(|r| {
                        flat.get(r.clone())
                            .unwrap_or(&[])
                            .iter()
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let collapsed = rebuilt.join("\n") != text.trim_end_matches(['\r', '\n']);
        Self {
            paragraphs,
            flat,
            collapsed,
        }
    }
}

/// Everything the layout reads about the block as it stands.
pub(super) struct Frame {
    pub(super) llx: f64,
    pub(super) wrap_width: f64,
    pub(super) first_indent: f64,
    pub(super) first_baseline: f64,
    pub(super) leading: f64,
    pub(super) alignment: BlockAlignment,
}

/// One laid-out line: its word ranges (into `Words::flat`), origin, natural
/// width and the justify slack per gap.
pub(super) struct PlacedLine {
    pub(super) words: Vec<std::ops::Range<usize>>,
    pub(super) origin_x: f64,
    pub(super) baseline_y: f64,
    pub(super) width: f64,
    pub(super) per_gap: f64,
    /// The line ends its paragraph and another follows: a typed break.
    pub(super) typed_break: bool,
}

/// Plan replacing block `block_index` on page `page_index` with `text`.
///
/// # Errors
///
/// [`BlockEditError`]: an unlocatable or unsupported block, a refused
/// character (all named), or empty text.
pub(crate) fn plan_block_text(
    doc: &DocumentView<'_>,
    page_index: usize,
    block_index: usize,
    text: &str,
    opts: &BlockEditOptions,
) -> Result<BlockTextPlan, BlockEditError> {
    let words = Words::split(text);
    if words.flat.is_empty() {
        return Err(BlockEditError::EmptyText);
    }
    let pages = page_tree::pages_in(doc).map_err(crate::text_edit::ReflowApplyError::from)?;
    let page = pages
        .get(page_index)
        .ok_or(crate::text_edit::ReflowApplyError::PageIndex(page_index))?;
    let options = ExtractOptions::default().with_provenance(true);
    let extracted = text_extract::extract_page_view(doc, page, page_index, &options)
        .map_err(crate::text_edit::ReflowApplyError::from)?;
    let cells = crate::text_edit::detect_cell_regions(doc, page_index).map_err(table_error)?;
    let model =
        EditableTextModel::recognize_with_cells(&extracted, &reflow_recognition_options(), &cells);
    let req = ReflowRequest::new()
        .with_wrap_width_opt(opts.wrap_width)
        .with_page_cropbox(page.crop_box);
    let shape = ReflowEngine::new(&model)
        .preview(block_index, &req)
        .map_err(crate::text_edit::ReflowApplyError::from)?;
    let stream =
        ContentStream::from_page(doc, page).map_err(crate::text_edit::ReflowApplyError::from)?;
    let block = model
        .blocks()
        .get(block_index)
        .ok_or(BlockEditError::unsupported(
            UnsupportedCause::ShowOperatorsNotFound,
        ))?;
    let located = locate(&model, block, &stream)?;
    let target = EditPlanTarget::page(page)?;
    let recs = walk_records(doc, &page.resources, &stream);
    let flat: String = words.flat.iter().collect();
    let enc = encode_block(
        doc,
        &target,
        &recs,
        page_index,
        located.anchor_span,
        &flat,
        &opts.edit,
    )?;
    if enc.codes.len() != words.flat.len() {
        return Err(BlockEditError::unsupported(
            UnsupportedCause::CommitFailed {
                detail: "the encoder did not give one code per character".to_owned(),
            },
        ));
    }
    let frame = frame_of(&model, block, &shape);
    let measure = Measure::new(&enc, &located.style, &located.prov);
    let lines = place(&words, &frame, &measure);
    let ctx = PlanCtx {
        page,
        page_index,
        stream: &stream,
        block_index,
        shape: &shape,
        frame: &frame,
        words: &words,
        enc,
        located,
    };
    finish(ctx, &lines, &measure)
}

/// The block's provenance, its region (justify base applied) and the style
/// and show operator of its first glyph.
pub(super) struct Located {
    pub(super) prov: BlockProvenance,
    pub(super) region: BlockRegion,
    pub(super) style: SpanStyle,
    pub(super) anchor_span: crate::span::ByteSpan,
    pub(super) looks: usize,
    pub(super) spacing_note: Option<String>,
}

/// [`BlockEditReport::looks`] for `block` on `page`; `None` when the block
/// cannot be located as `edit_block_text` would locate it.
pub(crate) fn block_looks(
    doc: &DocumentView<'_>,
    page: &Page,
    model: &EditableTextModel<'_>,
    block: &Block,
) -> Option<usize> {
    let stream = ContentStream::from_page(doc, page).ok()?;
    locate(model, block, &stream).ok().map(|l| l.looks)
}

fn locate(
    model: &EditableTextModel<'_>,
    block: &Block,
    stream: &ContentStream,
) -> Result<Located, BlockEditError> {
    let prov = block_provenance(model, block)?;
    let mut region = locate_block_region(stream, &prov.op_spans)?;
    let spacing = JustifySpacing::detect(model, block);
    spacing.apply_to(&mut region);
    let first = block
        .line_indices
        .first()
        .and_then(|&li| model.lines().get(li))
        .and_then(|l| l.glyphs.first().copied())
        .ok_or(BlockEditError::unsupported(
            UnsupportedCause::NoShowOperators,
        ))?;
    let style = style_of(model, &region, first)?.clone();
    let anchor_span = model
        .provenance(first)
        .ok_or(crate::text_edit::ReflowApplyError::NoProvenance)?
        .operator_span;
    let mut looks: Vec<&SpanStyle> = Vec::new();
    for s in region.styles.values() {
        if !looks.iter().any(|l| l.same_look(s)) {
            looks.push(s);
        }
    }
    let looks = looks.len();
    Ok(Located {
        prov,
        region,
        style,
        anchor_span,
        looks,
        spacing_note: spacing.disclosure(),
    })
}

/// The frame the new lines go in: the block's left edge, the wrap width, the
/// first line's indent, top baseline, leading and alignment.
fn frame_of(model: &EditableTextModel<'_>, block: &Block, shape: &ReflowPreview) -> Frame {
    let alignment = shape.alignment.alignment;
    let llx = shape.old_bbox.llx;
    let first_llx = block
        .line_indices
        .first()
        .and_then(|&li| model.lines().get(li))
        .map_or(llx, |l| l.bbox.llx);
    let indent = first_llx - llx;
    let keeps_indent = matches!(alignment, BlockAlignment::Left | BlockAlignment::Justified);
    Frame {
        llx,
        wrap_width: shape.wrap_width,
        first_indent: if keeps_indent && indent > EPS {
            indent
        } else {
            0.0
        },
        first_baseline: shape
            .lines
            .first()
            .map_or(shape.old_bbox.ury, |l| l.baseline_y),
        leading: shape.leading,
        alignment,
    }
}

/// Advances of the encoded text in page user space along the line.
pub(super) struct Measure {
    /// User-space units per text-space unit along the line (`Tm.a × CTM.a`).
    pub(super) scale: f64,
    /// Per character of `Words::flat`, its advance in text space (§9.4.4).
    pub(super) advances: Vec<f64>,
}

impl Measure {
    fn new(enc: &BlockEncoding, style: &SpanStyle, prov: &BlockProvenance) -> Self {
        let get = |p| style.ambient.get(p).value;
        let th = get(TextStateParam::HorizScale) / 100.0;
        let (tc, tw) = (
            get(TextStateParam::CharSpacing),
            get(TextStateParam::WordSpacing),
        );
        let single_byte = enc.font.bytes_per_code() == 1;
        let advances = enc
            .codes
            .iter()
            .map(|&c| glyph_advance_with(&enc.font, c, style.size, tc, tw, th, single_byte))
            .collect();
        Self {
            scale: prov.tm_a * prov.ctm_a,
            advances,
        }
    }

    /// The user-space width of `flat[range]`.
    pub(super) fn width(&self, range: std::ops::Range<usize>) -> f64 {
        self.advances.get(range).unwrap_or(&[]).iter().sum::<f64>() * self.scale
    }
}

/// Pack each paragraph at the wrap width and place every line: the first
/// line of the block keeps its indent, baselines step by the leading across
/// paragraphs, and a blank paragraph is a blank line.
fn place(words: &Words, frame: &Frame, m: &Measure) -> Vec<PlacedLine> {
    let mut out: Vec<PlacedLine> = Vec::new();
    for (pi, para) in words.paragraphs.iter().enumerate() {
        if let Some(prev) = out.last_mut() {
            prev.typed_break = true;
        }
        let widths: Vec<f64> = para.iter().map(|r| m.width(r.clone())).collect();
        let space = para
            .first()
            .and_then(|r| (r.end < m.advances.len()).then(|| m.width(r.end..r.end + 1)))
            .unwrap_or(0.0);
        let indent = |s: usize| {
            if pi == 0 && s == 0 {
                frame.first_indent
            } else {
                0.0
            }
        };
        let ranges = greedy_pack(widths.len(), frame.wrap_width, |s, e| {
            indent(s) + line_natural_width(&widths, space, s, e)
        });
        if ranges.is_empty() {
            out.push(placed(Vec::new(), out.len(), 0.0, 0.0, frame, 0.0));
            continue;
        }
        let count = ranges.len();
        for (li, r) in ranges.into_iter().enumerate() {
            let natural = line_natural_width(&widths, space, r.start, r.end);
            let lead = indent(r.start);
            let room = frame.wrap_width - lead;
            let gaps = r.len().saturating_sub(1);
            let per_gap = if frame.alignment.is_justified() && li + 1 < count && gaps > 0 {
                ((room - natural) / gaps as f64).max(0.0)
            } else {
                0.0
            };
            let ws = para.get(r).unwrap_or(&[]).to_vec();
            out.push(placed(ws, out.len(), lead, natural, frame, per_gap));
        }
    }
    out
}

fn placed(
    words: Vec<std::ops::Range<usize>>,
    index: usize,
    lead: f64,
    natural: f64,
    frame: &Frame,
    per_gap: f64,
) -> PlacedLine {
    PlacedLine {
        words,
        origin_x: align_origin_x(
            frame.alignment,
            frame.llx + lead,
            frame.wrap_width - lead,
            natural,
        ),
        baseline_y: frame.first_baseline - frame.leading * index as f64,
        width: natural,
        per_gap,
        typed_break: false,
    }
}

/// The plan's inputs past layout.
struct PlanCtx<'p> {
    page: &'p Page,
    page_index: usize,
    stream: &'p ContentStream,
    block_index: usize,
    shape: &'p ReflowPreview,
    frame: &'p Frame,
    words: &'p Words,
    enc: BlockEncoding,
    located: Located,
}

/// Emit, splice and report.
fn finish(
    ctx: PlanCtx<'_>,
    lines: &[PlacedLine],
    m: &Measure,
) -> Result<BlockTextPlan, BlockEditError> {
    let content_id = ctx
        .page
        .contents
        .first()
        .ok_or(BlockEditError::unsupported(UnsupportedCause::NoContents))?;
    let extra = ctx.page.contents.len().saturating_sub(1) as u64;
    let emitted = emit_lines(&EmitInput {
        stream: ctx.stream,
        located: &ctx.located,
        enc: &ctx.enc,
        flat: &ctx.words.flat,
        lines,
        measure: m,
        page_index: ctx.page_index,
    })?;
    let mut edits = vec![(
        ctx.located.region.start,
        ctx.located.region.end,
        emitted.body,
    )];
    let new_content = crate::text_edit::edit::splice(&ctx.stream.buf, &mut edits);
    let report = report_of(
        &ctx,
        lines,
        content_id.num,
        extra,
        emitted.marked_removed,
        emitted.leak_closed,
    );
    let new_bbox = new_box(ctx.shape, ctx.frame, lines.len());
    let text_lines = lines
        .iter()
        .map(|l| BlockEditLine {
            text: line_text(&ctx.words.flat, &l.words),
            origin_x: l.origin_x,
            baseline_y: l.baseline_y,
            width: l.width,
        })
        .collect();
    let mut glyphs = emitted.glyphs;
    glyphs.bbox = [new_bbox.llx, new_bbox.lly, new_bbox.urx, new_bbox.ury];
    glyphs.disclosures.clone_from(&report.disclosures);
    let enc = ctx.enc;
    Ok(BlockTextPlan {
        new_content,
        font_writes: enc.writes,
        preview: BlockEditPreview {
            lines: text_lines,
            leading: ctx.frame.leading,
            old_bbox: ctx.shape.old_bbox,
            new_bbox,
            glyphs,
            report,
        },
    })
}

fn line_text(flat: &[char], words: &[std::ops::Range<usize>]) -> String {
    words
        .iter()
        .map(|r| {
            flat.get(r.clone())
                .unwrap_or(&[])
                .iter()
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The box the new lines occupy: top-anchored at the old box, the old
/// descent below the last baseline.
fn new_box(shape: &ReflowPreview, frame: &Frame, line_count: usize) -> Rect {
    let old = shape.old_bbox;
    let descent = old_last_baseline(shape) - old.lly;
    let last = frame.first_baseline - frame.leading * line_count.saturating_sub(1) as f64;
    Rect {
        llx: frame.llx,
        lly: last - descent,
        urx: frame.llx + frame.wrap_width,
        ury: old.ury,
    }
}

/// The block's original bottom baseline: the old preview's top baseline less
/// its line count's leading steps (the reflow frame's own arithmetic).
fn old_last_baseline(shape: &ReflowPreview) -> f64 {
    let top = shape
        .lines
        .first()
        .map_or(shape.old_bbox.lly, |l| l.baseline_y);
    top - shape.leading * shape.lines_before.saturating_sub(1) as f64
}

fn report_of(
    ctx: &PlanCtx<'_>,
    lines: &[PlacedLine],
    content_object: u32,
    extra: u64,
    marked_removed: usize,
    leak_closed: bool,
) -> BlockEditReport {
    let new_bbox = new_box(ctx.shape, ctx.frame, lines.len());
    let old = ctx.shape.old_bbox;
    let drop = old.lly - new_bbox.lly;
    let mut report = BlockEditReport {
        block_index: ctx.block_index,
        lines_before: ctx.shape.lines_before,
        lines_after: lines.len(),
        wrap_width: ctx.frame.wrap_width,
        alignment: ctx.frame.alignment,
        looks: ctx.located.looks,
        height_delta: new_bbox.height() - old.height(),
        overflow_pt: (drop > EPS).then_some(drop),
        page_overflow: page_overflow(
            ctx.page.crop_box,
            new_bbox,
            lines,
            old_last_baseline(ctx.shape) - old.lly,
        ),
        base_font: ctx.enc.font.base_font.clone(),
        font_substituted_from: ctx.enc.substituted_from.clone(),
        glyphs_added: ctx.enc.glyphs_added.clone(),
        tagged_mcid: ctx.located.prov.mcid,
        marked_content_removed: marked_removed,
        content_object,
        extra_objects_emptied: extra,
        disclosures: Vec::new(),
    };
    report.disclosures = super::emit::disclosures(
        &report,
        &ctx.enc,
        &ctx.located,
        ctx.words.collapsed,
        leak_closed,
    );
    report
}

/// The new box past the cropbox bottom or right edge, if it is.
fn page_overflow(
    crop: Rect,
    bbox: Rect,
    lines: &[PlacedLine],
    descent: f64,
) -> Option<PageOverflow> {
    let past_bottom_pt = (crop.lly - bbox.lly).max(0.0);
    let past_right_pt = (bbox.urx - crop.urx).max(0.0);
    if past_bottom_pt <= EPS && past_right_pt <= EPS {
        return None;
    }
    let lines_outside = lines
        .iter()
        .filter(|l| l.baseline_y - descent < crop.lly)
        .count();
    Some(PageOverflow {
        past_bottom_pt,
        lines_outside,
        past_right_pt,
    })
}
