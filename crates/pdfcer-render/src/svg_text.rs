//! Planning for SVG export with text kept as text (G033).
//!
//! The recorder wraps every shown string's outline paints in an
//! [`Op::Text`] carrying the glyphs' ids, Unicode values and glyph →
//! device transforms. [`plan`] decides, per run and in the writer's own
//! traversal order, whether that run can become one `<text>` element, and
//! gathers each font's character → glyph map. It then builds one web font
//! per font used ([`crate::font::webfont`]).
//!
//! A run becomes `<text>` only when doing so renders the same picture:
//!
//! - the font program is an sfnt (TrueType or CFF-flavoured OpenType);
//! - every paint in the run is a plain solid nonzero fill, with one colour,
//!   blend mode and clip across the run;
//! - every glyph maps to exactly one BMP, non-control character, and no
//!   character already maps to a different glyph of the same font (a
//!   `cmap` holds one glyph per character);
//! - every glyph sits on the first glyph's baseline at its size and
//!   orientation, so one `transform` plus per-character `x` places them all;
//! - the font builds and its licence permits embedding.
//!
//! Any other run is written as outlines, exactly as without the option,
//! and counted by reason in [`crate::svg::SvgTextOutcome`].

use std::collections::BTreeMap;
use std::sync::Arc;

use tiny_skia::{BlendMode, FillRule, Transform};

use crate::canvas::Brush;
use crate::display_list::{ClipId, Op, TextRunInfo};
use crate::font::webfont::{self, WebFont, WebFontError};
use crate::svg::SvgTextOutcome;
use crate::text::LoadedFont;

/// Tolerance on scale and skew between a glyph's transform and the run's
/// first glyph's (dimensionless).
const LINEAR_TOL: f32 = 1e-3;
/// Tolerance on baseline offset, in font units.
const BASELINE_TOL: f32 = 0.5;

/// Why one run stays as outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fallback {
    NotSfnt,
    Paint,
    Unmapped,
    Conflict,
    Geometry,
    FontBuild,
    Restricted,
}

/// A run that can be written as `<text>`.
#[derive(Debug)]
pub(crate) struct TextRunPlan {
    /// Index into [`TextPlan::fonts`].
    pub font: usize,
    /// The characters, one per glyph.
    pub chars: Vec<char>,
    /// Each character's x, in font units along the first glyph's baseline.
    pub xs: Vec<f32>,
    /// Glyph space of the first glyph → device.
    pub origin: Transform,
    /// Font units per em.
    pub upem: f32,
    pub rgba: [u8; 4],
    pub blend: BlendMode,
    pub clip: Option<ClipId>,
}

/// One embedded font.
#[derive(Debug)]
pub(crate) struct FontPlan {
    /// The CSS family pdfcer names it (`pdfcer-fN`).
    pub css_name: String,
    /// The PDF's own family, subset tag stripped, for a fallback name.
    pub family: String,
    pub font: Result<WebFont, WebFontError>,
}

/// Every run's decision, in traversal order, plus the fonts.
#[derive(Debug, Default)]
pub(crate) struct TextPlan {
    pub runs: Vec<Result<TextRunPlan, Fallback>>,
    pub fonts: Vec<FontPlan>,
}

impl TextPlan {
    /// The run's final decision, font build included.
    pub(crate) fn resolved(&self, i: usize) -> Result<&TextRunPlan, Fallback> {
        match self.runs.get(i) {
            Some(Ok(run)) => match &self.fonts[run.font].font {
                Ok(_) => Ok(run),
                Err(WebFontError::Restricted) => Err(Fallback::Restricted),
                Err(WebFontError::NotSfnt) => Err(Fallback::NotSfnt),
                Err(_) => Err(Fallback::FontBuild),
            },
            Some(Err(f)) => Err(*f),
            None => Err(Fallback::Paint),
        }
    }

    /// Count the outcome.
    pub(crate) fn outcome(&self) -> SvgTextOutcome {
        let mut o = SvgTextOutcome::default();
        for i in 0..self.runs.len() {
            match self.resolved(i) {
                Ok(_) => o.runs_as_text += 1,
                Err(Fallback::NotSfnt) => o.fallback_not_sfnt += 1,
                Err(Fallback::Paint) => o.fallback_paint += 1,
                Err(Fallback::Unmapped) => o.fallback_unmapped += 1,
                Err(Fallback::Conflict) => o.fallback_conflict += 1,
                Err(Fallback::Geometry) => o.fallback_geometry += 1,
                Err(Fallback::FontBuild) => o.fallback_font_build += 1,
                Err(Fallback::Restricted) => o.fallback_restricted += 1,
            }
        }
        // A font counts as embedded when some run is written with it.
        let mut used = vec![false; self.fonts.len()];
        for i in 0..self.runs.len() {
            if let Ok(run) = self.resolved(i) {
                used[run.font] = true;
            }
        }
        o.fonts_embedded = used.iter().filter(|u| **u).count();
        o
    }
}

/// Plan every [`Op::Text`] in `ops`, in the order the SVG writer visits
/// them (depth-first, layers included).
pub(crate) fn plan(ops: &[Op]) -> TextPlan {
    let mut planner = Planner::default();
    planner.walk(ops);
    let fonts = planner
        .fonts
        .into_iter()
        .map(|(font, chars, css_name)| {
            let family = family_of(&font.base_font);
            let built = webfont::build(font.data.bytes(), &chars, &family);
            FontPlan {
                css_name,
                family,
                font: built,
            }
        })
        .collect();
    TextPlan {
        runs: planner.runs,
        fonts,
    }
}

#[derive(Default)]
struct Planner {
    runs: Vec<Result<TextRunPlan, Fallback>>,
    fonts: Vec<(Arc<LoadedFont>, BTreeMap<char, u16>, String)>,
}

impl Planner {
    fn walk(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Layer { ops, .. } => self.walk(ops),
                Op::Text { run, ops } => {
                    let decision = self.plan_run(run, ops);
                    self.runs.push(decision);
                }
                Op::Fill { .. } | Op::Stroke { .. } => {}
            }
        }
    }

    fn plan_run(&mut self, run: &TextRunInfo, ops: &[Op]) -> Result<TextRunPlan, Fallback> {
        if !is_sfnt(run.font.data.bytes()) {
            return Err(Fallback::NotSfnt);
        }
        let (rgba, blend, clip) = uniform_paint(ops).ok_or(Fallback::Paint)?;

        let mut chars = Vec::with_capacity(run.glyphs.len());
        let mut gids = Vec::with_capacity(run.glyphs.len());
        for g in &run.glyphs {
            let c = single_char(g.unicode.as_deref()).ok_or(Fallback::Unmapped)?;
            chars.push(c);
            gids.push(u16::try_from(g.gid).map_err(|_| Fallback::Unmapped)?);
        }
        let first = run.glyphs.first().ok_or(Fallback::Paint)?;
        let origin = first.to_device;
        let inverse = origin.invert().ok_or(Fallback::Geometry)?;
        let mut xs = Vec::with_capacity(run.glyphs.len());
        for g in &run.glyphs {
            let p = inverse.pre_concat(g.to_device);
            let aligned = (p.sx - 1.0).abs() < LINEAR_TOL
                && (p.sy - 1.0).abs() < LINEAR_TOL
                && p.kx.abs() < LINEAR_TOL
                && p.ky.abs() < LINEAR_TOL
                && p.ty.abs() < BASELINE_TOL
                && p.tx.is_finite();
            if !aligned {
                return Err(Fallback::Geometry);
            }
            xs.push(p.tx);
        }

        let slot = match self
            .fonts
            .iter()
            .position(|(f, _, _)| Arc::ptr_eq(f, &run.font))
        {
            Some(i) => i,
            None => {
                let name = format!("pdfcer-f{}", self.fonts.len());
                self.fonts
                    .push((Arc::clone(&run.font), BTreeMap::new(), name));
                self.fonts.len() - 1
            }
        };
        let map = &mut self.fonts[slot].1;
        // Check the whole run before recording any of it, so a refused run
        // leaves no characters behind.
        let mut pending = BTreeMap::new();
        for (&c, &g) in chars.iter().zip(&gids) {
            let known = map.get(&c).or_else(|| pending.get(&c));
            match known {
                Some(&existing) if existing != g => return Err(Fallback::Conflict),
                Some(_) => {}
                None => {
                    pending.insert(c, g);
                }
            }
        }
        map.extend(pending);

        Ok(TextRunPlan {
            font: slot,
            chars,
            xs,
            origin,
            upem: run.upem,
            rgba,
            blend,
            clip,
        })
    }
}

/// A plain sfnt, not a collection.
fn is_sfnt(data: &[u8]) -> bool {
    matches!(
        data.get(..4),
        Some([0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true")
    )
}

/// The run's one colour, blend and clip, when every paint is a plain solid
/// nonzero anti-aliased fill sharing them.
fn uniform_paint(ops: &[Op]) -> Option<([u8; 4], BlendMode, Option<ClipId>)> {
    let mut found: Option<([u8; 4], BlendMode, Option<ClipId>)> = None;
    for op in ops {
        let Op::Fill {
            brush, rule, clip, ..
        } = op
        else {
            return None;
        };
        let Brush::Solid { rgba } = &brush.brush else {
            return None;
        };
        if *rule != FillRule::Winding || !brush.anti_alias {
            return None;
        }
        let this = (*rgba, brush.blend, *clip);
        match found {
            None => found = Some(this),
            Some(prev) if prev == this => {}
            Some(_) => return None,
        }
    }
    found
}

/// Exactly one character a font `cmap` format 4 can carry and an SVG can
/// hold as text.
fn single_char(text: Option<&str>) -> Option<char> {
    let mut it = text?.chars();
    let c = it.next()?;
    if it.next().is_some() {
        return None;
    }
    let v = u32::from(c);
    let ok = v <= 0xFFFF && !c.is_control() && v != 0xFFFE && v != 0xFFFF;
    ok.then_some(c)
}

/// `/BaseFont` without its six-letter subset tag, reduced to characters
/// that need no quoting in a CSS family name.
fn family_of(base_font: &str) -> String {
    let name = match base_font.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => base_font,
    };
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "pdfcer".to_owned()
    } else {
        clean.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_tags_are_stripped_and_names_made_css_safe() {
        assert_eq!(family_of("ABCDEF+Arial-BoldMT"), "Arial-BoldMT");
        assert_eq!(family_of("abcdef+Arial"), "abcdef Arial");
        assert_eq!(family_of("Times New Roman,Bold"), "Times New Roman Bold");
        assert_eq!(family_of("'\";"), "pdfcer");
    }

    #[test]
    fn only_single_bmp_printable_characters_are_kept() {
        assert_eq!(single_char(Some("A")), Some('A'));
        assert_eq!(single_char(Some(" ")), Some(' '));
        assert_eq!(single_char(Some("fi")), None);
        assert_eq!(single_char(Some("\u{1F600}")), None);
        assert_eq!(single_char(Some("\u{7}")), None);
        assert_eq!(single_char(Some("")), None);
        assert_eq!(single_char(None), None);
    }
}
