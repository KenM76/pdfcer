//! Rewrite the show operators holding changed words (ISO 32000-2 §9.4.3),
//! leaving every other glyph, and the text matrix after each operator,
//! exactly where it was.

use std::collections::HashMap;

use crate::content::ContentStream;
use crate::page_tree::Page;
use crate::span::ByteSpan;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_edit::edit::block_encode::{BlockEncoding, encode_block};
use crate::text_edit::edit::{
    EditOptions, EditPlanTarget, FontWrites, OpRec, Rec, ShowData, ShowOp, mat_mul,
    resolve_font_dict, splice, writes_vertically,
};
use crate::text_edit::model::GlyphRef;
use crate::text_edit::{BlockEditError, ReflowApplyError};
use crate::text_extract::{ContentStreamRef, ExtractedGlyph, PageText};
use crate::view::DocumentView;

use super::WordFit;
use super::diff::Fate;
use super::words::Word;

mod emit;
mod layout;

pub(super) use layout::{Layout, layout};

/// The horizontal-scaling fit stays within these multiples of the
/// original scaling.
const FIT_BOUNDS: (f64, f64) = (0.25, 4.0);

/// What the rewrite reads.
pub(super) struct Ctx<'a, 'd> {
    pub(super) doc: &'a DocumentView<'d>,
    pub(super) page: &'a Page,
    pub(super) page_index: usize,
    pub(super) stream: &'a ContentStream,
    pub(super) recs: &'a [OpRec],
    pub(super) text: &'a PageText,
    pub(super) opts: &'a EditOptions,
}

/// The rewritten page content and what it took.
pub(super) struct Done {
    pub(super) content: Vec<u8>,
    pub(super) writes: FontWrites,
    pub(super) fits: Vec<WordFit>,
    pub(super) glyphs_added: Vec<char>,
    pub(super) disclosures: Vec<String>,
    /// The replacements and their encodings, for [`layout`].
    pub(super) reps: Vec<Rep>,
    pub(super) encs: Vec<BlockEncoding>,
}

/// What becomes of one slot of a rewritten show operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Act {
    Keep,
    Skip,
    /// Set replacement `n` here.
    Insert(usize),
}

/// A replaced word: where it was, its new codes and its fitted scaling.
pub(super) struct Rep {
    pub(super) old_text: String,
    pub(super) text: String,
    rec: usize,
    anchor: ByteSpan,
    origin: (f64, f64),
    /// The old word's width in unscaled text space.
    width: f64,
    pub(super) codes: Vec<u32>,
    /// Index into the encodings.
    pub(super) enc: usize,
    /// The horizontal scaling to set it at, as a ratio (`Tz` ÷ 100).
    pub(super) th: f64,
    clamped: bool,
}

/// Every page-stream glyph of the extraction, by the operator showing it.
struct OpIndex {
    by_end: HashMap<usize, usize>,
    slots: HashMap<usize, Vec<GlyphRef>>,
}

impl OpIndex {
    fn build(recs: &[OpRec], text: &PageText) -> Self {
        let by_end: HashMap<usize, usize> = recs
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r.rec, Rec::Show(_)))
            .map(|(i, r)| (r.end, i))
            .collect();
        let mut slots: HashMap<usize, Vec<GlyphRef>> = HashMap::new();
        for (ri, run) in text.runs.iter().enumerate() {
            for (gi, g) in run.glyphs.iter().enumerate() {
                let Some(p) = &g.provenance else { continue };
                if p.content_stream != ContentStreamRef::Page {
                    continue;
                }
                if let Some(&rec) = by_end.get(&p.operator_span.end()) {
                    slots
                        .entry(rec)
                        .or_default()
                        .push(GlyphRef { run: ri, glyph: gi });
                }
            }
        }
        Self { by_end, slots }
    }

    /// The operator record and slot index of glyph `g`.
    fn locate(&self, text: &PageText, g: GlyphRef) -> Result<(usize, usize), BlockEditError> {
        let prov = glyph(text, g)
            .and_then(|x| x.provenance.as_ref())
            .ok_or(ReflowApplyError::NoProvenance)?;
        if let ContentStreamRef::Form { object } = prov.content_stream {
            return Err(BlockEditError::unsupported(
                UnsupportedCause::InsideFormXObject {
                    object: Some(object),
                },
            ));
        }
        let rec = self.by_end.get(&prov.operator_span.end()).copied();
        let slot = rec.and_then(|r| self.slots.get(&r)?.iter().position(|s| *s == g));
        match (rec, slot) {
            (Some(r), Some(s)) => Ok((r, s)),
            _ => Err(not_found()),
        }
    }

    fn slot_count(&self, rec: usize) -> usize {
        self.slots.get(&rec).map_or(0, Vec::len)
    }
}

fn glyph(text: &PageText, g: GlyphRef) -> Option<&ExtractedGlyph> {
    text.runs.get(g.run)?.glyphs.get(g.glyph)
}

/// The refusal for operators or words the plan cannot find again.
pub(super) fn not_found() -> BlockEditError {
    BlockEditError::unsupported(UnsupportedCause::ShowOperatorsNotFound)
}

fn failed(detail: &str) -> BlockEditError {
    BlockEditError::unsupported(UnsupportedCause::CommitFailed {
        detail: detail.to_owned(),
    })
}

/// Record `rec`'s show operator, or [`not_found`] when it is not one.
pub(super) fn show(recs: &[OpRec], rec: usize) -> Result<&ShowData, BlockEditError> {
    match recs.get(rec).map(|r| &r.rec) {
        Some(Rec::Show(s)) => Ok(s),
        _ => Err(not_found()),
    }
}

/// Rewrite the operators holding the words `fates` changes.
///
/// # Errors
///
/// A changed word in a form XObject, in a `'`/`"` operator, in vertical
/// text or at an unknown position; a refused character; or an edit that
/// would change more than one `/Contents` stream.
pub(super) fn rewrite(
    ctx: &Ctx<'_, '_>,
    old: &[Word],
    fates: &[Fate],
) -> Result<Done, BlockEditError> {
    let index = OpIndex::build(ctx.recs, ctx.text);
    let (acts, mut reps) = plan_acts(ctx, &index, old, fates)?;
    let mut affected: Vec<usize> = acts.keys().copied().collect();
    affected.sort_unstable();
    for &r in &affected {
        check_op(ctx, show(ctx.recs, r)?, index.slot_count(r))?;
    }
    let (encs, writes) = encode(ctx, &mut reps)?;
    for rep in &mut reps {
        fit(ctx, rep, &encs)?;
    }
    let mut edits = Vec::with_capacity(affected.len());
    for &r in &affected {
        let (Some(a), Some(op)) = (acts.get(&r), ctx.recs.get(r)) else {
            continue;
        };
        let bytes = emit::op(ctx, show(ctx.recs, r)?, a, &reps, &encs)?;
        edits.push((op.start, op.end, bytes));
    }
    let content = splice(&ctx.stream.buf, &mut edits);
    one_stream(ctx, &content)?;
    Ok(finish(content, writes, reps, encs))
}

/// Slot actions per affected operator, keyed by its record index.
type OpActs = HashMap<usize, Vec<Act>>;

/// Each affected operator's slot actions, and the replacements in reading
/// order.
fn plan_acts(
    ctx: &Ctx<'_, '_>,
    index: &OpIndex,
    old: &[Word],
    fates: &[Fate],
) -> Result<(OpActs, Vec<Rep>), BlockEditError> {
    let mut acts: OpActs = HashMap::new();
    let mut reps = Vec::new();
    for (w, fate) in old.iter().zip(fates) {
        let new_text = match fate {
            Fate::Keep => continue,
            Fate::Replace(t) => Some(t),
            Fate::Delete => None,
        };
        if let Some(t) = new_text {
            reps.push(rep(ctx, index, w, t)?);
        }
        for (gi, &g) in w.glyphs.iter().enumerate() {
            let (rec, slot) = index.locate(ctx.text, g)?;
            let list = acts
                .entry(rec)
                .or_insert_with(|| vec![Act::Keep; index.slot_count(rec)]);
            let act = match new_text {
                Some(_) if gi == 0 => Act::Insert(reps.len() - 1),
                _ => Act::Skip,
            };
            if let Some(a) = list.get_mut(slot) {
                *a = act;
            }
        }
    }
    Ok((acts, reps))
}

/// A replacement for `w`, measured but not yet encoded or fitted.
fn rep(ctx: &Ctx<'_, '_>, index: &OpIndex, w: &Word, text: &str) -> Result<Rep, BlockEditError> {
    let (Some(&first), Some(&last)) = (w.glyphs.first(), w.glyphs.last()) else {
        return Err(not_found());
    };
    let (Some(g0), Some(gl)) = (glyph(ctx.text, first), glyph(ctx.text, last)) else {
        return Err(not_found());
    };
    let anchor = g0
        .provenance
        .as_ref()
        .map(|p| p.operator_span)
        .ok_or(ReflowApplyError::NoProvenance)?;
    let (rec, _) = index.locate(ctx.text, first)?;
    let s = show(ctx.recs, rec)?;
    let m = mat_mul(s.text_matrix, s.ctm);
    let scale = m[0].hypot(m[1]);
    if scale < 1e-9 {
        return Err(BlockEditError::unsupported(UnsupportedCause::DegenerateCtm));
    }
    let (x0, y0) = (f64::from(g0.x), f64::from(g0.y));
    let (ex, ey) = gl.advance_end();
    let (dx, dy) = (f64::from(g0.direction.0), f64::from(g0.direction.1));
    let width = ((f64::from(ex) - x0) * dx + (f64::from(ey) - y0) * dy) / scale;
    Ok(Rep {
        old_text: w.text.clone(),
        text: text.to_owned(),
        rec,
        anchor,
        origin: (x0, y0),
        width,
        codes: Vec::new(),
        enc: 0,
        th: s.th(),
        clamped: false,
    })
}

/// Refuse an operator the rewrite cannot reproduce.
fn check_op(ctx: &Ctx<'_, '_>, s: &ShowData, glyphs: usize) -> Result<(), BlockEditError> {
    if matches!(s.op, ShowOp::Quote | ShowOp::DoubleQuote) {
        return Err(BlockEditError::unsupported(UnsupportedCause::QuoteOperator));
    }
    if !s.matrix_known {
        return Err(failed(
            "the text position before a changed word is not known (an unresolvable font earlier in the text object)",
        ));
    }
    if s.slots.len() != glyphs {
        return Err(not_found());
    }
    let dict = resolve_font_dict(ctx.doc, &ctx.page.resources, &s.font_name).ok_or(
        BlockEditError::unsupported(UnsupportedCause::FontUnresolvable),
    )?;
    if writes_vertically(ctx.doc, dict) {
        return Err(BlockEditError::unsupported(
            UnsupportedCause::VerticalWriting,
        ));
    }
    Ok(())
}

/// Encode the replacements, once per font: each group's texts joined, in
/// the font of its first word's operator, then split back per word.
fn encode(
    ctx: &Ctx<'_, '_>,
    reps: &mut [Rep],
) -> Result<(Vec<BlockEncoding>, FontWrites), BlockEditError> {
    let target = EditPlanTarget::page(ctx.page)?;
    let mut keys: Vec<Vec<u8>> = Vec::new();
    let mut group = Vec::with_capacity(reps.len());
    for r in reps.iter() {
        let font = show(ctx.recs, r.rec)?.font_name.clone();
        let i = keys.iter().position(|k| *k == font).unwrap_or_else(|| {
            keys.push(font);
            keys.len() - 1
        });
        group.push(i);
    }
    let mut encs = Vec::with_capacity(keys.len());
    let mut writes = FontWrites::default();
    for gi in 0..keys.len() {
        let members: Vec<usize> = (0..reps.len())
            .filter(|&i| group.get(i) == Some(&gi))
            .collect();
        let text: String = members
            .iter()
            .filter_map(|&i| reps.get(i).map(|r| r.text.as_str()))
            .collect();
        let anchor = members
            .first()
            .and_then(|&i| reps.get(i))
            .map(|r| r.anchor)
            .ok_or_else(not_found)?;
        let mut enc = encode_block(
            ctx.doc,
            &target,
            ctx.recs,
            ctx.page_index,
            anchor,
            &text,
            ctx.opts,
        )?;
        if enc.codes.len() != text.chars().count() {
            return Err(failed("the encoder did not give one code per character"));
        }
        let mut codes = enc.codes.iter().copied();
        for &i in &members {
            let Some(r) = reps.get_mut(i) else { continue };
            r.codes = codes.by_ref().take(r.text.chars().count()).collect();
            r.enc = encs.len();
        }
        writes = writes
            .merged(std::mem::take(&mut enc.writes))
            .ok_or_else(|| failed("two fonts would both gain a new font program"))?;
        encs.push(enc);
    }
    Ok((encs, writes))
}

/// Fit `rep`'s scaling so its new codes span the old word's width.
fn fit(ctx: &Ctx<'_, '_>, rep: &mut Rep, encs: &[BlockEncoding]) -> Result<(), BlockEditError> {
    let s = show(ctx.recs, rep.rec)?;
    let enc = encs.get(rep.enc).ok_or_else(not_found)?;
    let natural = emit::natural_width(s, enc, &rep.codes);
    if natural <= 1e-9 || rep.width <= 1e-9 {
        return Ok(());
    }
    let wanted = rep.width / natural / s.th();
    let ratio = wanted.clamp(FIT_BOUNDS.0, FIT_BOUNDS.1);
    rep.clamped = (ratio - wanted).abs() > 1e-9;
    rep.th = s.th() * ratio;
    Ok(())
}

/// Refuse a result that changes more than one `/Contents` stream: the
/// commit would fold them, taking an OCR layer into the page's content.
fn one_stream(ctx: &Ctx<'_, '_>, content: &[u8]) -> Result<(), BlockEditError> {
    if ctx.page.contents.len() < 2 {
        return Ok(());
    }
    let parts: Option<Vec<Vec<u8>>> = ctx
        .page
        .contents
        .iter()
        .map(|&id| crate::ocr::refold::decoded(ctx.doc, id))
        .collect();
    let parts = parts.ok_or_else(|| failed("a /Contents stream could not be decoded"))?;
    if crate::edit::stream_localize::single_part_change(&parts, content).is_none() {
        return Err(BlockEditError::unsupported(
            UnsupportedCause::EditSpansStreams,
        ));
    }
    Ok(())
}

fn finish(content: Vec<u8>, writes: FontWrites, reps: Vec<Rep>, encs: Vec<BlockEncoding>) -> Done {
    let mut glyphs_added: Vec<char> = Vec::new();
    let mut disclosures: Vec<String> = Vec::new();
    for e in &encs {
        for c in &e.glyphs_added {
            if !glyphs_added.contains(c) {
                glyphs_added.push(*c);
            }
        }
        for d in &e.disclosures {
            if !disclosures.contains(d) {
                disclosures.push(d.clone());
            }
        }
    }
    let fits = reps
        .iter()
        .map(|r| WordFit {
            old_text: r.old_text.clone(),
            new_text: r.text.clone(),
            origin: r.origin,
            horizontal_scale: r.th * 100.0,
            clamped: r.clamped,
        })
        .collect();
    Done {
        content,
        writes,
        fits,
        glyphs_added,
        disclosures,
        reps,
        encs,
    }
}
