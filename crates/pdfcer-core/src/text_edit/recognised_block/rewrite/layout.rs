//! The layout a recognised-block edit leaves, for drawing while the operator
//! types: every surviving glyph positioned in page space, kept words where
//! they are and each replacement at its old word's origin under its fitted
//! scaling (§9.4.4), grouped into one preview per font resource.

use std::collections::HashMap;

use crate::object::Dict;
use crate::page_tree::Rect;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_edit::edit::block_encode::BlockEncoding;
use crate::text_edit::edit::{
    EditLayout, ShowData, TextEditPreview, glyph_advance_with, mat_mul, resolve_font,
    resolve_font_dict,
};
use crate::text_edit::model::glyph_text;
use crate::text_edit::{BlockEditError, BlockEditLine};
use crate::text_extract::font::ExtractFont;

use super::super::diff::Fate;
use super::super::words::{Word, union};
use super::emit::replay;
use super::{Ctx, OpIndex, Rep, not_found, show};

/// Where the words land.
pub(in crate::text_edit::recognised_block) struct Layout {
    pub(in crate::text_edit::recognised_block) lines: Vec<BlockEditLine>,
    /// The box the surviving glyphs occupy; `None` when none survives.
    pub(in crate::text_edit::recognised_block) bbox: Option<Rect>,
    pub(in crate::text_edit::recognised_block) runs: Vec<TextEditPreview>,
}

/// One show operator's font and the start of each of its slots along the
/// text line (text space after scaling).
struct Play {
    font: ExtractFont,
    dict: Dict,
    starts: Vec<f64>,
}

/// A preview being gathered for one font resource.
struct Run {
    layout: EditLayout,
    program: Option<Vec<u8>>,
}

/// A line being gathered: its words' texts and the page-space baseline
/// points of its first glyph origin and last glyph end.
struct LineBuild {
    line: usize,
    text: String,
    start: (f64, f64),
    end: (f64, f64),
}

/// A placed word: its text and its baseline start and end in page space.
struct Placed {
    text: String,
    start: (f64, f64),
    end: (f64, f64),
}

/// One glyph to place: its character and code, its start along the text
/// line and its advance (text space after scaling), and the scaling it is
/// set at.
struct Glyph {
    ch: Option<char>,
    code: u32,
    x: f64,
    adv: f64,
    th: f64,
    /// The font's ascent and descent per unit size.
    metrics: (f64, f64),
}

/// The glyphs' positions after `fates` are applied to `old`: `reps` are the
/// replacements in reading order (empty when nothing changes).
///
/// # Errors
///
/// A glyph the plan cannot find again, or a font it cannot resolve.
pub(in crate::text_edit::recognised_block) fn layout(
    ctx: &Ctx<'_, '_>,
    old: &[Word],
    fates: &[Fate],
    reps: &[Rep],
    encs: &[BlockEncoding],
) -> Result<Layout, BlockEditError> {
    let mut b = Builder {
        ctx,
        index: OpIndex::build(ctx.recs, ctx.text),
        plays: HashMap::new(),
        runs: Vec::new(),
        lines: Vec::new(),
    };
    let mut ri = 0;
    for (w, fate) in old.iter().zip(fates) {
        let placed = match fate {
            Fate::Delete => continue,
            Fate::Keep => b.kept(w)?,
            Fate::Replace(_) => {
                let rep = reps.get(ri).ok_or_else(not_found)?;
                ri += 1;
                b.replaced(w, rep, encs.get(rep.enc).ok_or_else(not_found)?)?
            }
        };
        b.line(w.line, placed);
    }
    Ok(b.finish())
}

struct Builder<'a, 'b, 'd> {
    ctx: &'a Ctx<'b, 'd>,
    index: OpIndex,
    plays: HashMap<usize, Play>,
    runs: Vec<Run>,
    lines: Vec<LineBuild>,
}

impl Builder<'_, '_, '_> {
    /// Operator `rec`'s font and slot starts, resolved once.
    fn play(&mut self, rec: usize) -> Result<&Play, BlockEditError> {
        if !self.plays.contains_key(&rec) {
            let s = show(self.ctx.recs, rec)?;
            let res = &self.ctx.page.resources;
            let unresolvable = || BlockEditError::unsupported(UnsupportedCause::FontUnresolvable);
            let dict = resolve_font_dict(self.ctx.doc, res, &s.font_name)
                .ok_or_else(unresolvable)?
                .clone();
            let font = resolve_font(self.ctx.doc, res, &s.font_name).ok_or_else(unresolvable)?;
            let (starts, _, _) = replay(&font, s);
            self.plays.insert(rec, Play { font, dict, starts });
        }
        self.plays.get(&rec).ok_or_else(not_found)
    }

    /// A kept word: each glyph at the start its operator gives it, in the
    /// operator's font.
    fn kept(&mut self, w: &Word) -> Result<Placed, BlockEditError> {
        let mut out: Option<Placed> = None;
        for &g in &w.glyphs {
            let (rec, slot) = self.index.locate(self.ctx.text, g)?;
            let s = show(self.ctx.recs, rec)?;
            let code = s.slots.get(slot).ok_or_else(not_found)?.code;
            let ch = glyph_text(self.ctx.text, g).chars().next();
            let (x, adv, metrics, dict, base) = {
                let play = self.play(rec)?;
                let x = play.starts.get(slot).copied().ok_or_else(not_found)?;
                let single = play.font.bytes_per_code() == 1;
                let adv =
                    glyph_advance_with(&play.font, code, s.tf_size, s.tc(), s.tw(), s.th(), single);
                let (dict, base) = (play.dict.clone(), play.font.base_font.clone());
                (x, adv, metrics(&play.font), dict, base)
            };
            let glyph = Glyph {
                ch,
                code,
                x,
                adv,
                th: s.th(),
                metrics,
            };
            if self.run_at(&s.font_name).is_none() {
                self.push_run(s.font_name.clone(), dict, base, s, None);
            }
            let run = self.run_at(&s.font_name).ok_or_else(not_found)?;
            place(run, s, &glyph);
            extend(&mut out, &w.text, s, x, x + adv);
        }
        out.ok_or_else(not_found)
    }

    /// A replaced word: its new codes from the old word's first glyph
    /// origin, under its fitted scaling, in the encoder's font.
    fn replaced(
        &mut self,
        w: &Word,
        rep: &Rep,
        enc: &BlockEncoding,
    ) -> Result<Placed, BlockEditError> {
        let first = w.glyphs.first().copied().ok_or_else(not_found)?;
        let (rec, slot) = self.index.locate(self.ctx.text, first)?;
        let s = show(self.ctx.recs, rec)?;
        let begin = self
            .play(rec)?
            .starts
            .get(slot)
            .copied()
            .ok_or_else(not_found)?;
        if self.run_at(&enc.font_resource).is_none() {
            let (dict, base) = (enc.font_dict.clone(), enc.font.base_font.clone());
            self.push_run(
                enc.font_resource.clone(),
                dict,
                base,
                s,
                enc.program.clone(),
            );
        }
        let run = self.run_at(&enc.font_resource).ok_or_else(not_found)?;
        let single = enc.font.bytes_per_code() == 1;
        let mut x = begin;
        let mut chars = rep.text.chars();
        for &code in &rep.codes {
            let adv =
                glyph_advance_with(&enc.font, code, s.tf_size, s.tc(), s.tw(), rep.th, single);
            let glyph = Glyph {
                ch: chars.next(),
                code,
                x,
                adv,
                th: rep.th,
                metrics: metrics(&enc.font),
            };
            place(run, s, &glyph);
            x += adv;
        }
        let mut out = None;
        extend(&mut out, &rep.text, s, begin, x);
        out.ok_or_else(not_found)
    }

    fn run_at(&mut self, key: &[u8]) -> Option<&mut Run> {
        self.runs.iter_mut().find(|r| r.layout.font_name == key)
    }

    /// Start the run for font resource `key` from `s`'s colours and mode.
    fn push_run(
        &mut self,
        key: Vec<u8>,
        dict: Dict,
        base_font: String,
        s: &ShowData,
        program: Option<Vec<u8>>,
    ) {
        self.runs.push(Run {
            layout: EditLayout {
                font_name: key,
                font_dict: dict,
                base_font,
                glyphs: Vec::new(),
                bbox: [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                fill: s.fill_color.clone(),
                stroke: s.stroke_color.clone(),
                render_mode: s.text_state.render_mode.value,
                fallback: None,
            },
            program,
        });
    }

    /// Add `placed` to line `line`, or start it.
    fn line(&mut self, line: usize, placed: Placed) {
        match self.lines.iter_mut().find(|l| l.line == line) {
            Some(l) => {
                l.text.push(' ');
                l.text.push_str(&placed.text);
                l.end = placed.end;
            }
            None => self.lines.push(LineBuild {
                line,
                text: placed.text,
                start: placed.start,
                end: placed.end,
            }),
        }
    }

    fn finish(self) -> Layout {
        let mut bbox: Option<Rect> = None;
        let mut runs = Vec::with_capacity(self.runs.len());
        for r in self.runs {
            let b = r.layout.bbox;
            let rect = Rect::from_corners(b[0], b[1], b[2], b[3]);
            bbox = Some(bbox.map_or(rect, |a| union(a, rect)));
            runs.push(TextEditPreview::new(
                self.ctx.page_index,
                r.layout,
                Vec::new(),
                None,
                r.program,
            ));
        }
        let lines = self
            .lines
            .into_iter()
            .map(|l| BlockEditLine {
                text: l.text,
                origin_x: l.start.0,
                baseline_y: l.start.1,
                width: (l.end.0 - l.start.0).hypot(l.end.1 - l.start.1),
            })
            .collect();
        Layout { lines, bbox, runs }
    }
}

fn metrics(font: &ExtractFont) -> (f64, f64) {
    (f64::from(font.ascent()), f64::from(font.descent()))
}

/// One glyph into `run`: `[Tfs·Th 0 0 Tfs tx Trise] × Tm × CTM` (§9.4.4),
/// the run's box grown by its advance and the font's ascent and descent.
fn place(run: &mut Run, s: &ShowData, g: &Glyph) {
    let line = mat_mul(s.text_matrix, s.ctm);
    let (tfs, rise) = (s.tf_size, s.text_state.rise.value);
    let m = mat_mul([tfs * g.th, 0.0, 0.0, tfs, g.x, rise], line);
    run.layout.glyphs.push((g.ch, g.code, m));
    let (asc, desc) = g.metrics;
    let (lo, hi) = (desc * tfs + rise, asc * tfs + rise);
    for (cx, cy) in [(g.x, lo), (g.x + g.adv, lo), (g.x, hi), (g.x + g.adv, hi)] {
        let (px, py) = at(line, cx, cy);
        let b = &mut run.layout.bbox;
        *b = [b[0].min(px), b[1].min(py), b[2].max(px), b[3].max(py)];
    }
}

/// Grow `out` to cover text-space `[from, to]` on `s`'s baseline.
fn extend(out: &mut Option<Placed>, text: &str, s: &ShowData, from: f64, to: f64) {
    let line = mat_mul(s.text_matrix, s.ctm);
    let end = at(line, to, 0.0);
    match out {
        Some(p) => p.end = end,
        None => {
            *out = Some(Placed {
                text: text.to_owned(),
                start: at(line, from, 0.0),
                end,
            });
        }
    }
}

/// Text-space `(x, y)` through `m`.
fn at(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}
