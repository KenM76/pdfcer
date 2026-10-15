//! The new text encoded look by look: each character in the font of the look
//! [`looks::assign`] gave it, one encode per distinct font (G199). When the
//! looks cannot all take the text, the whole text falls back to the first
//! run's look, as before, and says why.

use crate::span::ByteSpan;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_edit::edit::block_encode::{BlockEncoding, encode_block, show_at};
use crate::text_edit::edit::{EditOptions, EditPlanTarget, FontWrites, OpRec, ShowData};
use crate::text_edit::model::{Block, EditableTextModel};
use crate::text_edit::reflow_walk::SpanStyle;
use crate::view::DocumentView;

use super::BlockEditError;
use super::layout::Located;
use super::looks::{self, Looks};

/// The block's new text as codes, each character in its own look's font.
pub(super) struct Encoded {
    pub(super) looks: Looks,
    /// One encoding per distinct font, in first-use order.
    pub(super) fonts: Vec<BlockEncoding>,
    /// Per look, its index into `fonts`.
    pub(super) look_font: Vec<usize>,
    /// Per look, the style emitted: the look's own with the encoding's `Tf`
    /// resource (a same-face sibling's when one carries the text).
    pub(super) emit_styles: Vec<SpanStyle>,
    /// Per look, its first show operator's recorded state (preview colours).
    pub(super) shows: Vec<ShowData>,
    /// One code per character of the flat text.
    pub(super) codes: Vec<u32>,
    pub(super) writes: FontWrites,
    /// Why the per-look encoding was dropped for the first run's look.
    pub(super) fell_back: Option<String>,
}

impl Encoded {
    /// The look of character `i` and its font's encoding.
    pub(super) fn at(&self, i: usize) -> (usize, Option<&BlockEncoding>) {
        let look = self.looks.of_char.get(i).copied().unwrap_or(0);
        let font = self.look_font.get(look).and_then(|&f| self.fonts.get(f));
        (look, font)
    }

    /// The first look's encoding: what the report's font fields name.
    pub(super) fn first(&self) -> Option<&BlockEncoding> {
        self.fonts.first()
    }

    /// Characters any font gained, in first-gained order.
    pub(super) fn glyphs_added(&self) -> Vec<char> {
        let mut out: Vec<char> = Vec::new();
        for c in self.fonts.iter().flat_map(|f| f.glyphs_added.iter()) {
            if !out.contains(c) {
                out.push(*c);
            }
        }
        out
    }
}

/// What the encoders read besides the text.
pub(super) struct EncodeCtx<'a, 'd> {
    pub(super) doc: &'a DocumentView<'d>,
    pub(super) target: &'a EditPlanTarget,
    pub(super) recs: &'a [OpRec],
    pub(super) page_index: usize,
    pub(super) opts: &'a EditOptions,
}

/// Encode `flat` keeping every surviving character's look.
///
/// # Errors
///
/// The first run's font refusing the text (the fallback's error), or a
/// block glyph the region walk did not record.
pub(super) fn encode(
    ctx: &EncodeCtx<'_, '_>,
    model: &EditableTextModel<'_>,
    block: &Block,
    located: &Located,
    flat: &[char],
) -> Result<Encoded, BlockEditError> {
    let looks = looks::assign(model, block, &located.region, flat)?;
    let mut fell_back = None;
    if looks.styles.len() > 1 {
        match per_look(ctx, looks, flat) {
            Ok(e) => return Ok(e),
            Err(why) => fell_back = Some(why),
        }
    }
    let single = Looks {
        styles: vec![located.style.clone()],
        anchors: vec![located.anchor_span],
        of_char: vec![0; flat.len()],
        inherited: 0,
    };
    let enc = encode_font(ctx, located.anchor_span, &flat.iter().collect::<String>())?;
    let mut e = assemble(ctx, single, vec![enc], vec![0], flat)?;
    e.fell_back = fell_back;
    Ok(e)
}

/// One encode per distinct font; `Err` carries the reason for the fallback.
fn per_look(ctx: &EncodeCtx<'_, '_>, looks: Looks, flat: &[char]) -> Result<Encoded, String> {
    let mut keys: Vec<&[u8]> = Vec::new();
    let mut look_font = Vec::with_capacity(looks.styles.len());
    for s in &looks.styles {
        let i = keys.iter().position(|k| *k == s.font.as_slice());
        look_font.push(i.unwrap_or_else(|| {
            keys.push(&s.font);
            keys.len() - 1
        }));
    }
    let mut fonts = Vec::with_capacity(keys.len());
    for fi in 0..keys.len() {
        let first_look = look_font.iter().position(|&f| f == fi).unwrap_or(0);
        let Some(anchor) = looks.anchors.get(first_look).copied() else {
            return Err("a look has no show operator".to_owned());
        };
        let text: String = flat
            .iter()
            .zip(&looks.of_char)
            .filter(|(_, l)| look_font.get(**l) == Some(&fi))
            .map(|(c, _)| *c)
            .collect();
        fonts.push(encode_font(ctx, anchor, &text).map_err(|e| e.to_string())?);
    }
    assemble(ctx, looks, fonts, look_font, flat).map_err(|e| e.to_string())
}

fn encode_font(
    ctx: &EncodeCtx<'_, '_>,
    span: ByteSpan,
    text: &str,
) -> Result<BlockEncoding, BlockEditError> {
    let enc = encode_block(
        ctx.doc,
        ctx.target,
        ctx.recs,
        ctx.page_index,
        span,
        text,
        ctx.opts,
    )?;
    if enc.codes.len() != text.chars().count() {
        return Err(BlockEditError::unsupported(
            UnsupportedCause::CommitFailed {
                detail: "the encoder did not give one code per character".to_owned(),
            },
        ));
    }
    Ok(enc)
}

/// Interleave each font's codes back into text order and merge the writes.
fn assemble(
    ctx: &EncodeCtx<'_, '_>,
    looks: Looks,
    mut fonts: Vec<BlockEncoding>,
    look_font: Vec<usize>,
    flat: &[char],
) -> Result<Encoded, BlockEditError> {
    let mut cursors = vec![0usize; fonts.len()];
    let mut codes = Vec::with_capacity(flat.len());
    for l in looks.of_char.iter().take(flat.len()) {
        let fi = look_font.get(*l).copied().unwrap_or(0);
        let (Some(cur), Some(f)) = (cursors.get_mut(fi), fonts.get(fi)) else {
            continue;
        };
        codes.push(f.codes.get(*cur).copied().unwrap_or(0));
        *cur += 1;
    }
    let mut writes = FontWrites::default();
    for f in &mut fonts {
        writes =
            writes
                .merged(std::mem::take(&mut f.writes))
                .ok_or(BlockEditError::unsupported(
                    UnsupportedCause::CommitFailed {
                        detail: "two of the block's fonts would both gain a new font program"
                            .to_owned(),
                    },
                ))?;
    }
    let mut emit_styles = Vec::with_capacity(looks.styles.len());
    let mut shows = Vec::with_capacity(looks.styles.len());
    for (l, s) in looks.styles.iter().enumerate() {
        let f = look_font.get(l).and_then(|&i| fonts.get(i));
        let mut style = s.clone();
        if let Some(f) = f {
            style.font.clone_from(&f.font_resource);
        }
        emit_styles.push(style);
        let show = looks
            .anchors
            .get(l)
            .and_then(|&span| show_at(ctx.recs, ctx.page_index, span))
            .or_else(|| f.map(|f| f.anchor.clone()));
        shows.extend(show);
    }
    Ok(Encoded {
        looks,
        fonts,
        look_font,
        emit_styles,
        shows,
        codes,
        writes,
        fell_back: None,
    })
}
