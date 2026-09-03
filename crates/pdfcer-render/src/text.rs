//! # Text state and the code → glyph pipeline (ISO 32000-1 §9.3, §9.4, §9.6, §9.7)
//!
//! Everything between "the interpreter saw a `Tf`" and "the painter has
//! a GID and an advance". Spec sources, all in the PDF-spec RAG:
//! `iso32000__s__9.3.md` (the nine text-state parameters, Tables
//! 104–106), `iso32000__s__9.4.md` (text objects, `Trm`, the advance
//! formula, Tables 107–109), `iso32000__s__9.6.6.md` (simple-font
//! encoding, Tables 114/115), `iso32000__s__9.7.md` (composite fonts,
//! Tables 116/117/121), `iso32000__ref__text_pipeline.md` (the derived
//! end-to-end dispatch and its ten consolidated traps). Scope is fixed
//! by `docs/decisions/004-text-rendering-fonts.md` §4.3.
//!
//! ## Where the state lives, and why it is split in two
//!
//! §9.3's opening sentence is load-bearing: *"the text state comprises
//! those **graphics state** parameters that only affect text."* All nine
//! (`Tc Tw Th Tl Tf Tfs Tmode Trise Tk`) therefore sit inside
//! [`crate::gstate::GraphicsState`] and are **saved and restored by
//! `q`/`Q`** exactly like the line width. §9.3's scope rule confirms the
//! other half: they "may appear outside text objects", persist across
//! text objects within a content stream, and reset only per page.
//!
//! The three matrices are the opposite case. §9.4.1: `Tm`, `Tlm` and
//! `Trm` "shall not persist from one text object to the next" — `BT`
//! initializes `Tm`/`Tlm` to identity, `ET` discards them, and they are
//! not part of the graphics state at all. They live in [`TextObject`],
//! held by the interpreter for the duration of one `BT`…`ET` block, and
//! a `q`/`Q` pair inside a text object does **not** touch them.
//!
//! Getting this backwards is a silent-corruption bug in both
//! directions: putting `Tm` in the graphics state makes a mid-string
//! `Q` teleport the pen; leaving `Tf` out of it makes a `q … Tf … Q`
//! sequence keep the wrong font.
//!
//! ## Why the font is resolved once, at `Tf`, and not per glyph
//!
//! A simple font has exactly 256 character codes (§9.6.1: "each byte of
//! the string shall be treated as a separate character code"), so the
//! entire encoding chain — Annex D base table, `/Differences`, the
//! implicit-base rules, and §9.6.6.4's Branch A/B cmap ladder — is
//! evaluated for all 256 codes ONCE when `Tf` selects the font, and
//! collapsed into two flat arrays: [`SimpleFont::gids`] and
//! [`SimpleFont::widths`]. Painting a glyph is then an array index.
//!
//! That is not only a speed argument. Several links in the chain are
//! linear scans over the font program (a `post`-table name search, a
//! CFF charset walk), and doing them per painted glyph would make a
//! page of text quadratic in the glyph count — the kind of input-driven
//! blowup `ARCHITECTURE.md` §10 exists to prevent.
//!
//! Composite fonts cannot be flattened the same way — `Identity-H` has
//! 65,536 codes — but their per-code work is O(1) by construction
//! (identity, a two-byte array index, or one CFF charset lookup).
//!
//! ## The honesty contract (rule R20, `CLAUDE.md` rule 4)
//!
//! Distinct shortfalls are counted separately, because they mean
//! different things to an operator looking at a page:
//!
//! - **substituted** (bundled) — a real glyph was painted, but from a
//!   bundled Foxit Base-14 face rather than the document's own program.
//!   Shapes are pdfcer's plausible ones; positions are exact (they come
//!   from the PDF's own widths, decision 004 §3.6).
//! - **supplied** — a real glyph was painted from an OPERATOR-supplied
//!   face (decision 012), matched by name through the
//!   [`FontEnvironment::named`] seam the shell filled from a font
//!   folder. Still a substitute (positions from `/Widths`, only shapes
//!   are the operator's own), but a deliberate operator choice, so it is
//!   counted and disclosed distinctly from bundled (rule R63).
//! - **notdef** — the code resolved to no glyph at all. Something is
//!   missing from the page.
//! - **unsupported font** — the font's machinery is outside this Pass
//!   (a non-Identity CMap, `Identity-V`, a non-embedded composite). The
//!   text was **skipped**, not approximated.
//!
//!   ★ **Type 3 was on that list until `Pass 126.0` and is not any
//!   more.** It renders — see [`crate::type3`] — and its bucket now
//!   means only "Table 112's irreducible entries are missing", which is
//!   a much narrower thing than it used to mean. A counter whose
//!   MEANING changes under a reader is worse than one whose value
//!   does, so the change is stated here rather than left to be
//!   inferred from a smaller number.
//!
//! [`FontEnvironment::named`]: crate::font::FontEnvironment::named

use std::collections::HashMap;
use std::sync::Arc;

// decision 018: read paths take a `DocumentView` (graph + byte source), so
// the same code renders a loaded file or an editing session's unsaved state.
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::view::DocumentView;
use tiny_skia::Transform;

use crate::font::coredata::{self, BaseEncoding, Std14};
use crate::font::program::FontProgram;
use crate::font::{FallbackKey, FontData, FontEnvironment, GlyphSource, select};

/// `/Widths` and `/W` are expressed in glyph space, where 1000 units =
/// one text-space unit (§9.2.4; `iso32000__ref__text_pipeline.md` Stage
/// 2). This is a fixed property of the PDF metric arrays and is
/// **independent** of the font program's own units-per-em — which is
/// why outlines divide by [`FontProgram::upem`] and advances divide by
/// this constant. Conflating the two is decision 004 §3.3's named trap.
const GLYPH_SPACE_PER_TEXT_SPACE: f32 = 1000.0;

/// `/DW` default width for a CIDFont (Table 117: "Default value:
/// 1000"). Defaulting to 0 instead stacks every glyph at one point —
/// `iso32000__ref__text_pipeline.md` trap 6's composite-font twin.
const DEFAULT_CID_WIDTH: f32 = 1000.0;

/// Table 123 `Flags` bit 3 (`Symbolic`), 1-based in the spec, so
/// `1 << 2` here. Trap 2 of the text-pipeline reference: getting this
/// off by one silently flips the implicit-base-encoding decision.
const FLAG_SYMBOLIC: u32 = 1 << 2;
/// Table 123 `Flags` bit 19 (`ForceBold`).
const FLAG_FORCE_BOLD: u32 = 1 << 18;
/// `StemV` at or above which a descriptor-classified face is treated as
/// bold. Pure heuristic — no spec basis; §9.8.1 gives `StemV` as a
/// measurement, not a classification.
const BOLD_STEM_V: f64 = 140.0;

/// The nine §9.3 text-state parameters (Table 104), minus `Tk`, which
/// has no operator and arrives only through `gs` `/TK` (§9.3.8 — the
/// transparent imaging model, out of this Pass's scope).
///
/// Lives inside the graphics state and is therefore saved/restored by
/// `q`/`Q` — see the module docs for why that is the spec's answer and
/// not a convenience.
#[derive(Debug, Clone)]
pub struct TextState {
    /// `Tc` — character spacing, in **unscaled** text space units
    /// (§9.3.2). Initial value 0.
    pub char_spacing: f32,
    /// `Tw` — word spacing, unscaled text space units (§9.3.3).
    /// Initial 0. Applies **only** to the single-byte code 32.
    pub word_spacing: f32,
    /// `Th` — horizontal scaling as a RATIO (§9.3.4). The `Tz`
    /// operator's operand is a *percentage*, so `100 Tz` stores `1.0`
    /// here; storing the raw operand scales the page 100×.
    pub horizontal_scale: f32,
    /// `Tl` — leading, unscaled text space units (§9.3.5). Initial 0.
    /// Used only by `T*`, `'`, `"` (and set by `TD`).
    pub leading: f32,
    /// `Tf` — the selected font, resolved from the resource dictionary
    /// at `Tf` time. **No initial value** (§9.3, Table 105: "they shall
    /// be specified explicitly by using `Tf` before any text is
    /// shown"), so `None` here means showing text is undefined — the
    /// renderer skips and diagnoses rather than substituting, which
    /// would be "sneaky".
    pub font: Option<Arc<LoadedFont>>,
    /// `Tfs` — font size, a scale factor (Table 105). No initial value.
    pub font_size: f32,
    /// `Tmode` — text rendering mode (Table 106). Initial 0 (fill).
    pub render_mode: u8,
    /// `Trise` — text rise, unscaled text space units (§9.3.7).
    /// Initial 0; positive moves the baseline UP.
    pub rise: f32,
}

impl Default for TextState {
    /// Table 105's initial values. `Th` is 1.0 because Table 105's
    /// initial `Tz` is the *percentage* 100.
    fn default() -> Self {
        Self {
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            font: None,
            font_size: 0.0,
            render_mode: 0,
            rise: 0.0,
        }
    }
}

/// The three matrices that exist **only** between `BT` and `ET`
/// (§9.4.1). `Trm` is not stored: it is "a temporary matrix;
/// conceptually, it is recomputed before each glyph is painted"
/// (§9.4.4 NOTE 2), so it is computed on demand by
/// [`TextState::glyph_to_user`].
#[derive(Debug, Clone, Copy)]
pub struct TextObject {
    /// `Tm` — the text matrix. Updated by every painted glyph's
    /// advance (§9.4.2: the showing operators "update `Tm` by altering
    /// its e and f translation components").
    pub tm: Transform,
    /// `Tlm` — the text line matrix, "the value of `Tm` at the
    /// beginning of a line of text". `Td`/`TD`/`T*` concatenate onto
    /// **this**, never onto `Tm` — concatenating onto `Tm` accumulates
    /// intra-line glyph advances into the line origin, which is
    /// §9.4.4's named common bug.
    pub tlm: Transform,
}

impl TextObject {
    /// `BT` — "initializing the text matrix `Tm` and the text line
    /// matrix `Tlm` to the identity matrix" (Table 107).
    #[must_use]
    pub fn new() -> Self {
        Self {
            tm: Transform::identity(),
            tlm: Transform::identity(),
        }
    }

    /// `Td` — "move to the start of the next line, offset from the
    /// start of the CURRENT LINE by (tx, ty)":
    /// `Tm = Tlm = translate(tx, ty) × Tlm` (Table 108).
    pub fn next_line_offset(&mut self, tx: f32, ty: f32) {
        self.tlm = Transform::from_translate(tx, ty).post_concat(self.tlm);
        self.tm = self.tlm;
    }

    /// `Tm` — "shall NOT be concatenated onto the current text matrix,
    /// but shall REPLACE it" (Table 108), and replaces `Tlm` too.
    pub fn set_matrix(&mut self, m: Transform) {
        self.tm = m;
        self.tlm = m;
    }

    /// Advance the pen after a glyph (or a `TJ` adjustment):
    /// `Tm = translate(tx, ty) × Tm` (§9.4.4).
    pub fn advance(&mut self, tx: f32, ty: f32) {
        self.tm = Transform::from_translate(tx, ty).post_concat(self.tm);
    }
}

impl Default for TextObject {
    fn default() -> Self {
        Self::new()
    }
}

impl TextState {
    /// The glyph-space → **user**-space transform for one glyph, i.e.
    /// §9.4.4's `Trm` with the `CTM` factor left off.
    ///
    /// ```text
    ///          | Tfs · Th    0      0 |
    ///  Trm  =  |    0       Tfs     0 |  ×  Tm  ×  CTM
    ///          |    0      Trise    1 |
    /// ```
    ///
    /// The `CTM` is deliberately excluded. Stroked text takes its line
    /// width from the graphics state "**in USER space rather than in
    /// text space**" (§9.3.6), so the glyph path must exist in user
    /// space at the moment tiny-skia computes the stroke geometry —
    /// exactly as `crate::interpret`'s path painter already does for
    /// `S`. The caller passes the `CTM` separately to
    /// `fill_path`/`stroke_path`.
    ///
    /// `upem` (never the assumed 1000 — decision 004 §3.3) converts the
    /// outline's font units to glyph space; a CFF or Type 1 program may
    /// carry a non-standard `FontMatrix`.
    #[must_use]
    pub fn glyph_to_user(&self, tm: Transform, upem: f32) -> Transform {
        let upem = if upem > 0.0 { upem } else { 1000.0 };
        let param = Transform::from_row(
            self.font_size * self.horizontal_scale,
            0.0,
            0.0,
            self.font_size,
            0.0,
            self.rise,
        );
        Transform::from_scale(1.0 / upem, 1.0 / upem)
            .post_concat(param)
            .post_concat(tm)
    }

    /// The horizontal displacement after showing one glyph (§9.4.4):
    ///
    /// ```text
    /// tx = ( (w0 − Tj/1000) · Tfs + Tc + Tw ) · Th
    /// ```
    ///
    /// `w0` arrives here already in **text space** (the caller divided
    /// the PDF's glyph-space width by 1000). `Th` multiplies the entire
    /// expression — the glyph displacement, the `TJ` adjustment, `Tc`
    /// AND `Tw` — which §9.3.4 states explicitly and which is easy to
    /// get wrong by applying `Th` to the glyph term alone.
    ///
    /// `apply_word_spacing` is the caller's §9.3.3 decision: `Tw` fires
    /// only for "the single-byte character code 32… It shall NOT apply
    /// to occurrences of the byte value 32 in multiple-byte codes."
    #[must_use]
    pub fn advance_for(&self, w0_text: f32, tj: f32, apply_word_spacing: bool) -> f32 {
        let tw = if apply_word_spacing {
            self.word_spacing
        } else {
            0.0
        };
        ((w0_text - tj / GLYPH_SPACE_PER_TEXT_SPACE) * self.font_size + self.char_spacing + tw)
            * self.horizontal_scale
    }

    /// A standalone `TJ` number element's displacement: §9.4.4's
    /// formula with `w0 = 0` and **without** `Tc`/`Tw`, "since no glyph
    /// was painted" (§9.4's implementation note).
    ///
    /// The sign is the trap: the number is **subtracted**, so a
    /// positive adjustment moves the next glyph LEFT. Sign errors here
    /// produce reversed kerning that looks almost right.
    #[must_use]
    pub fn adjustment(&self, tj: f32) -> f32 {
        (-tj / GLYPH_SPACE_PER_TEXT_SPACE) * self.font_size * self.horizontal_scale
    }

    /// Whether this rendering mode fills (`Tr` 0, 2, 4, 6 — Table 106).
    #[must_use]
    pub fn fills(&self) -> bool {
        matches!(self.render_mode, 0 | 2 | 4 | 6)
    }

    /// Whether this rendering mode strokes (`Tr` 1, 2, 5, 6).
    #[must_use]
    pub fn strokes(&self) -> bool {
        matches!(self.render_mode, 1 | 2 | 5 | 6)
    }
}

/// One character code taken off a shown string (§9.4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    /// The code value: one byte for a simple font, a 2-byte big-endian
    /// CID under `Identity-H`.
    pub value: u32,
    /// Whether §9.3.3's word-spacing rule applies — true **only** for a
    /// single-byte code whose value is 32.
    pub word_spacing_applies: bool,
}

/// A font dictionary resolved into everything the painter needs.
///
/// Immutable and `Arc`-shared: `q`/`Q` clone the graphics state, and a
/// content stream typically re-selects the same handful of fonts many
/// times, so this is built once per distinct font resource per page
/// (the interpreter caches by resource name).
#[derive(Debug)]
pub struct LoadedFont {
    /// `/BaseFont`, verbatim (subset tag included) — the name shown in
    /// the diagnostics panel when this font was substituted.
    pub base_font: String,
    /// The font program bytes actually used: the document's own
    /// embedded program, a bundled substitute face, or an
    /// operator-supplied face (decision 012).
    pub data: FontData,
    /// Where [`Self::data`]'s glyphs come from — the three trust levels
    /// of decision 012 (rule R20/R63). `Embedded` is the document's own
    /// program (exact); `Bundled` and `Supplied` are both substitutes
    /// (positions still exact from `/Widths`, only shapes differ), but
    /// are counted and disclosed separately so an operator can tell
    /// pdfcer's plausible shape from their own deliberately-supplied one.
    pub source: GlyphSource,
    /// Simple (§9.6) or composite (§9.7) machinery.
    pub kind: FontKind,
}

/// The font shapes this crate renders.
///
/// This said "the two font shapes ... Type 3 is recognized and skipped"
/// until `Pass 126.0`, which is the `R212` shape: a claim about the
/// module sitting where a reader meets it first, with nothing under test
/// to contradict it. There are three now.
#[derive(Debug)]
pub enum FontKind {
    /// `Type1` / `MMType1` / `TrueType` — one byte per glyph, fully
    /// flattened at load time.
    Simple(Box<SimpleFont>),
    /// `Type0` with an `Identity-H` CMap.
    Composite(CompositeFont),
    /// `Type3` (§9.6.5) — glyphs are **content streams**, not a font
    /// program.
    ///
    /// The odd one out in three ways that matter at every call site
    /// below, which is why it is a variant rather than a flag:
    /// [`LoadedFont::data`] holds **no program** (there is none to
    /// hold), widths are in `FontMatrix` units rather than thousandths,
    /// and painting a glyph means running an interpreter rather than
    /// looking up an outline. See [`crate::type3`].
    Type3(Box<crate::type3::Type3Font>),
}

/// A simple font, flattened (module docs).
#[derive(Debug)]
pub struct SimpleFont {
    /// Code → GID. `None` means the whole §9.6.6 ladder failed for that
    /// code — the glyph is painted as `.notdef` (GID 0) and counted.
    pub gids: [Option<u32>; 256],
    /// Code → advance width in **glyph space** (÷1000 for text space),
    /// from `/Widths`, else the AFM tables for a standard-14 font with
    /// no `/Widths`, else `/MissingWidth` (Table 122 default: 0).
    pub widths: [f32; 256],
}

/// A composite font restricted to `Identity-H` (§9.7.5.2, Table 118).
#[derive(Debug)]
pub struct CompositeFont {
    /// How a CID becomes a GID (§9.7.4.2).
    pub cid_to_gid: CidToGid,
    /// `/DW`, glyph space (Table 117; default 1000).
    pub default_width: f32,
    /// `/W` as `(first_cid, last_cid, width)` triples in file order.
    /// Deliberately NOT materialized per CID: CIDs run to 65,535 and
    /// ranges like `7080 8032 1000` are the point of the format
    /// (§9.7.4.3).
    pub widths: Vec<(u32, u32, f32)>,
}

/// CID → GID strategies (§9.7.4.2).
#[derive(Debug)]
pub enum CidToGid {
    /// `/CIDToGIDMap /Identity` (Table 117's default), or a
    /// `CIDFontType0` whose CFF Top DICT does not use CIDFont
    /// operators: the CID **is** the GID.
    Identity,
    /// `/CIDToGIDMap` stream: "the glyph index for a particular CID
    /// value `c` shall be a 2-byte value stored in bytes `2 × c` and
    /// `2 × c + 1`, where the first byte shall be the high-order byte"
    /// (Table 117). Out-of-range → GID 0.
    Stream(Vec<u8>),
    /// `CIDFontType0` with a CID-keyed CFF: the CID goes through the
    /// CFF `charset` (§9.7.4.2). "Although in many fonts the CID value
    /// and GID value are the same, the CID and GID values may differ."
    CffCharset,
}

impl LoadedFont {
    /// Split a shown string into character codes (§9.4.3): one byte per
    /// code for a simple font, two big-endian bytes per code under
    /// `Identity-H` ("pairs of bytes representing CIDs, high-order byte
    /// first", §9.7.5.2).
    ///
    /// An odd trailing byte under `Identity-H` is malformed and the
    /// spec's §9.7.6.3 recovery does not cleanly apply (the codespace
    /// is a single 2-byte range). pdfcer consumes it as a high byte with
    /// a zero low byte — a documented choice, not spec text — so that
    /// the string is fully consumed and cannot loop.
    #[must_use]
    pub fn codes(&self, string: &[u8]) -> Vec<Code> {
        match self.kind {
            // §9.6: a Type 3 font is a SIMPLE font — one byte per code,
            // and §9.3.3's word spacing applies to the single-byte code
            // 32 exactly as it does for Type 1. Only the glyph
            // *machinery* differs.
            FontKind::Simple(_) | FontKind::Type3(_) => string
                .iter()
                .map(|&b| Code {
                    value: u32::from(b),
                    word_spacing_applies: b == 32,
                })
                .collect(),
            FontKind::Composite(_) => string
                .chunks(2)
                .map(|pair| {
                    let hi = u32::from(pair.first().copied().unwrap_or(0));
                    let lo = u32::from(pair.get(1).copied().unwrap_or(0));
                    Code {
                        // §9.3.3: word spacing "shall NOT apply to
                        // occurrences of the byte value 32 in
                        // multiple-byte codes" — always false here.
                        value: (hi << 8) | lo,
                        word_spacing_applies: false,
                    }
                })
                .collect(),
        }
    }

    /// The advance for a code, **in text space** — the value §9.4.4's
    /// `w0` wants, with no further division.
    ///
    /// # ★ Why this exists at all, rather than a `/1000` at the call site
    ///
    /// Because for a Type 3 font that division is **wrong**, and it is
    /// wrong in a way that renders. Table 112: Type 3 widths "shall be
    /// interpreted in glyph space as specified by `FontMatrix`
    /// (**unlike** the widths of a Type 1 font, which are in thousandths
    /// of a unit of text space)". A caller that divides by 1000 and then
    /// lets the font matrix apply scales a typical advance by `1e-6`,
    /// collapsing a whole line of text onto one point.
    ///
    /// The call site used to spell `font.width(code) / 1000.0`, which is
    /// correct for two of the three font kinds and silently wrong for
    /// the third. Moving the conversion in here makes the units a
    /// property of the font rather than a habit of the caller, so the
    /// mistake is not available to make.
    #[must_use]
    pub fn advance_text_space(&self, code: u32) -> f32 {
        match &self.kind {
            FontKind::Type3(f) => f.advance_text_space(code),
            FontKind::Simple(_) | FontKind::Composite(_) => {
                self.width(code) / GLYPH_SPACE_PER_TEXT_SPACE
            }
        }
    }

    /// Whether this font's glyphs are content streams (§9.6.5).
    ///
    /// Asked at the paint site, where a Type 3 font takes a completely
    /// different route: no font program to parse, no outline to look up,
    /// an interpreter run instead.
    #[must_use]
    pub const fn is_type3(&self) -> bool {
        matches!(self.kind, FontKind::Type3(_))
    }

    /// The Type 3 model, or `None` for the other two kinds.
    #[must_use]
    pub fn type3(&self) -> Option<&crate::type3::Type3Font> {
        match &self.kind {
            FontKind::Type3(f) => Some(f),
            FontKind::Simple(_) | FontKind::Composite(_) => None,
        }
    }

    /// Advance width for a code, in **glyph space** (1000 = one text
    /// space unit).
    ///
    /// ★ Prefer [`Self::advance_text_space`]. This returns a raw number
    /// whose UNITS DEPEND ON THE FONT KIND — thousandths for a simple or
    /// composite font, `FontMatrix` units for a Type 3 — so dividing its
    /// result by 1000 is correct for two kinds and silently wrong for the
    /// third.
    #[must_use]
    pub fn width(&self, code: u32) -> f32 {
        match &self.kind {
            FontKind::Type3(f) => usize::try_from(code)
                .ok()
                .and_then(|i| f.widths.get(i))
                .copied()
                .unwrap_or(0.0),
            FontKind::Simple(f) => usize::try_from(code)
                .ok()
                .and_then(|i| f.widths.get(i))
                .copied()
                .unwrap_or(0.0),
            FontKind::Composite(f) => f
                .widths
                .iter()
                .find(|&&(first, last, _)| code >= first && code <= last)
                .map_or(f.default_width, |&(_, _, w)| w),
        }
    }

    /// Code → GID, or `None` when nothing in the applicable fallback
    /// ladder resolved it (the caller counts a notdef and paints GID 0).
    ///
    /// `program` is the parsed font program, needed only for the
    /// [`CidToGid::CffCharset`] case; simple fonts were flattened at
    /// load time and consult nothing.
    #[must_use]
    pub fn gid(&self, code: u32, program: Option<&FontProgram<'_>>) -> Option<u32> {
        match &self.kind {
            // A Type 3 font has no GIDs, because it has no program to
            // index into. Its codes resolve to a glyph NAME and then to a
            // content stream (§9.6.5 steps a and b) -- see
            // `crate::type3::Type3Font::proc_for`. Returning `None` here
            // would be read by the caller as "notdef", which is a
            // different fact, so no caller may reach this for a Type 3
            // font; `is_type3()` is the guard.
            FontKind::Type3(_) => None,
            FontKind::Simple(f) => usize::try_from(code)
                .ok()
                .and_then(|i| f.gids.get(i))
                .copied()
                .flatten(),
            FontKind::Composite(f) => match &f.cid_to_gid {
                CidToGid::Identity => Some(code),
                CidToGid::Stream(map) => {
                    // Bounds first: `2×cid + 1` must be inside the
                    // stream (§9.7 gotchas — subset maps are routinely
                    // shorter than the CID space).
                    let hi = usize::try_from(code).ok()?.checked_mul(2)?;
                    let (a, b) = (*map.get(hi)?, *map.get(hi + 1)?);
                    Some(u32::from(u16::from_be_bytes([a, b])))
                }
                CidToGid::CffCharset => {
                    let cid = u16::try_from(code).ok()?;
                    Some(program?.cff_cid_to_gid(cid))
                }
            },
        }
    }
}

/// Why a font dictionary could not be rendered by this Pass.
///
/// Distinct from "the font loaded but a code has no glyph": these mean
/// the whole font is out of scope, the text is **skipped**, and
/// `Diagnostics::fonts_unsupported` is incremented (never approximated
/// — decision 004 §4.3's deferred list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedFont {
    /// `/Subtype /Type3` (§9.6.5) with Table 112's **irreducible**
    /// entries missing.
    ///
    /// ★ This meant "pdfcer does not render Type 3 at all" until
    /// `Pass 126.0`, and its doc comment said so. It now means something
    /// much narrower, and the narrowing matters to whoever reads the
    /// counter: `/CharProcs` absent (there are no glyph descriptions to
    /// run) or `/FontMatrix` absent (there is no mapping from glyph
    /// space to text space, and guessing the conventional `[0.001 …]`
    /// would render a nonstandard font a thousand times too large).
    ///
    /// Everything else recovers. In particular a font with **no usable
    /// `/Encoding`** is NOT refused: §9.6.6.3 makes that a font whose
    /// every code resolves to no glyph, which is a blank page by the
    /// standard rather than a feature pdfcer lacks — and refusing it
    /// would skip the text's ADVANCES too, moving everything after it on
    /// the line.
    Type3,
    /// A `Type0` font whose CMap is not `Identity-H`: a predefined CJK
    /// CMap or an embedded CMap stream (§9.7.5, deferred with its own
    /// Adobe-CMap-resource licensing check).
    NonIdentityCmap,
    /// `Identity-V` — the codes decode identically to `Identity-H`, but
    /// painting them with horizontal advances would be confidently
    /// wrong, so the text is skipped instead (§9.7.5.2; vertical
    /// metrics `DW2`/`W2` are outside this Pass).
    VerticalWriting,
    /// A composite font with no embedded program. §9.7.5.2: "the
    /// `Identity-H` and `Identity-V` CMaps shall not be used with a
    /// non-embedded font" — there is no defined mapping from an
    /// arbitrary CID to a substitute face's glyphs, so guessing one
    /// would paint confident nonsense.
    CompositeNotEmbedded,
    /// `/Subtype` absent or unrecognized (Table 110).
    UnknownSubtype,
    /// The font program (embedded or substitute) failed to parse.
    UnusableProgram,
}

impl UnsupportedFont {
    /// A stable, machine-readable reason key for the by-reason
    /// diagnostic breakdown (rule R20 / fuzzy-never-sneaky): a future
    /// "why is text missing?" is answered by a counter, not by
    /// re-instrumenting the loader. The strings are part of the CLI's
    /// stdout contract and the GUI disclosure — append variants, never
    /// rename an existing key.
    #[must_use]
    pub const fn reason_key(&self) -> &'static str {
        match self {
            Self::Type3 => "Type3",
            Self::NonIdentityCmap => "NonIdentityCmap",
            Self::VerticalWriting => "VerticalWriting",
            Self::CompositeNotEmbedded => "CompositeNotEmbedded",
            Self::UnknownSubtype => "UnknownSubtype",
            Self::UnusableProgram => "UnusableProgram",
        }
    }

    /// The full, ordered set of reason keys — so consumers (the CLI
    /// stable line, the GUI panel) can emit every bucket in a fixed
    /// order even when its count is zero, keeping the output diffable.
    #[must_use]
    pub const fn all_reason_keys() -> [&'static str; 6] {
        [
            "Type3",
            "NonIdentityCmap",
            "VerticalWriting",
            "CompositeNotEmbedded",
            "UnknownSubtype",
            "UnusableProgram",
        ]
    }
}

/// Resolve a font dictionary into a [`LoadedFont`] (§9.5 Table 110
/// dispatch, then §9.6 or §9.7).
///
/// # Errors
///
/// [`UnsupportedFont`] when the font's machinery is outside decision
/// 004 §4.3's Pass 1 scope. The caller counts it and skips the text —
/// it must never fall back to "render something."
pub fn load(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    env: &FontEnvironment,
) -> Result<LoadedFont, UnsupportedFont> {
    let subtype = doc
        .resolve(font_dict.get(b"Subtype").unwrap_or(&Object::Null))
        .as_name()
        .map(|n| n.as_bytes().to_vec())
        .unwrap_or_default();
    let base_font = name_of(doc, font_dict, b"BaseFont").unwrap_or_default();

    match subtype.as_slice() {
        b"Type1" | b"MMType1" | b"TrueType" => load_simple(doc, font_dict, env, base_font),
        b"Type0" => load_composite(doc, font_dict, base_font),
        b"Type3" => crate::type3::Type3Font::load(doc, font_dict)
            .map(|t3| LoadedFont {
                base_font,
                // ★ EMPTY, and it is not a placeholder. A Type 3 font
                // HAS no program — §9.6.5's whole point is that the font
                // dictionary defines the glyphs rather than describing a
                // program elsewhere. Every consumer of `data` is gated on
                // `is_type3()` being false; see `interpret::show_string`,
                // which must not try to parse this.
                data: crate::font::FontData::new(Vec::new()),
                // The document's own glyph descriptions, exactly. Nothing
                // is substituted and nothing can be — there is no shape
                // to substitute FOR.
                source: crate::font::GlyphSource::Embedded,
                kind: FontKind::Type3(Box::new(t3)),
            })
            // Only when Table 112's irreducible entries are missing — see
            // `Type3Font::load` for which two and why the others recover.
            .ok_or(UnsupportedFont::Type3),
        _ => Err(UnsupportedFont::UnknownSubtype),
    }
}

/// §9.6 simple font: pick a program, resolve all 256 codes, tabulate
/// all 256 widths.
fn load_simple(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    env: &FontEnvironment,
    base_font: String,
) -> Result<LoadedFont, UnsupportedFont> {
    let descriptor = dict_of(doc, font_dict, b"FontDescriptor");
    let flags = descriptor
        .and_then(|d| doc.resolve(d.get(b"Flags")?).as_int())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let italic_angle = descriptor
        .and_then(|d| doc.resolve(d.get(b"ItalicAngle")?).as_number())
        .unwrap_or(0.0);
    let missing_width = descriptor
        .and_then(|d| doc.resolve(d.get(b"MissingWidth")?).as_number())
        .unwrap_or(0.0) as f32;
    let std14 = coredata::std14_by_base_font(select::strip_subset_tag(&base_font));

    let embedded = descriptor.and_then(|d| embedded_program(doc, d));
    let (data, source) = match embedded {
        Some(bytes) => (FontData::new(bytes), GlyphSource::Embedded),
        None => substitute_face(env, &base_font, flags, italic_angle, descriptor, doc)?,
    };
    // §9.6.6.1's implicit-base decision table hinges on whether the
    // program is EMBEDDED, not on whether we have bytes — a substitute
    // face's built-in encoding (bundled OR supplied) is not the
    // document's font's built-in encoding, so a non-embedded font keeps
    // the StandardEncoding / built-in-std-14-encoding arm.
    let embedded_program_present = source == GlyphSource::Embedded;

    let program = FontProgram::parse(data.bytes()).map_err(|_| UnsupportedFont::UnusableProgram)?;
    let names = encoding_table(doc, font_dict, embedded_program_present, flags, std14);
    let gids = resolve_gids(&program, &names);

    // METRICS-ONLY std-14 widening (§9.6.2.2 has no answer for this file).
    //
    // A font with no `/Widths`, no embedded program, and a name that is
    // not literally one of the fourteen — `/BaseFont /Arial` is the
    // canonical case — used to fall all the way through to
    // `/MissingWidth`, whose default is **0**. Every glyph then advanced
    // by nothing and the whole run stacked on one point. Measured on
    // `pdfium/testing/resources/bookmarks.pdf`: "Page1" rendered as an
    // unreadable pile, while pdfium and Acrobat both laid it out
    // correctly — both alias Arial to Helvetica. pdfcer disclosed only
    // `substituted=1`, which says the SHAPES are pdfcer's and says nothing
    // about the positions being wrong.
    //
    // `select::by_name` already knew: it maps Arial to `Sans` in order to
    // pick the face. That knowledge simply never reached the width
    // ladder. Taking the metrics from the family pdfcer is ALREADY drawing
    // keeps shapes and advances from disagreeing about which font this
    // is, and matches what both reference renderers do.
    //
    // Deliberately NOT reused for `encoding_table` above: `std14` there
    // drives §9.6.6.1's symbolic classification, and widening it would
    // change which encoding a non-embedded Arial gets. This is a metrics
    // fix; it must not become an encoding change.
    //
    // Gated on `!embedded_program_present`, because a font that ships its
    // own program and omits `/Widths` should take advances from that
    // program, not from Helvetica's AFM table. That case is left alone.
    let metrics_std14 = std14.or_else(|| {
        if embedded_program_present {
            return None;
        }
        select::by_name(select::strip_subset_tag(&base_font)).map(std14_for_fallback)
    });
    let widths = width_table(
        doc,
        font_dict,
        &names,
        &program,
        metrics_std14,
        missing_width,
    );

    Ok(LoadedFont {
        base_font,
        data,
        source,
        kind: FontKind::Simple(Box::new(SimpleFont { gids, widths })),
    })
}

/// §9.7 composite font, `Identity-H` only.
fn load_composite(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    base_font: String,
) -> Result<LoadedFont, UnsupportedFont> {
    // Table 121: `/Encoding` is required and is either a predefined
    // CMap name or a CMap stream. Only the two Identity names are in
    // scope, and only the horizontal one is painted.
    match name_of(doc, font_dict, b"Encoding").as_deref() {
        Some("Identity-H") => {}
        Some("Identity-V") => return Err(UnsupportedFont::VerticalWriting),
        _ => return Err(UnsupportedFont::NonIdentityCmap),
    }

    // "PDF supports only a single descendant" (§9.7.1) — but it is
    // still an ARRAY (Table 121; §9.7 gotcha: code that reads it as a
    // dictionary fails on every composite font).
    let descendant = doc
        .resolve(font_dict.get(b"DescendantFonts").unwrap_or(&Object::Null))
        .as_array()
        .and_then(|a| a.first())
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
        .ok_or(UnsupportedFont::NonIdentityCmap)?;

    let descriptor = dict_of(doc, descendant, b"FontDescriptor");
    // §9.7.5.2 forbids Identity-H with a non-embedded font, and there
    // is no defined CID → substitute-face mapping, so this is a hard
    // stop rather than a substitution.
    let bytes = descriptor
        .and_then(|d| embedded_program(doc, d))
        .ok_or(UnsupportedFont::CompositeNotEmbedded)?;
    let data = FontData::new(bytes);
    let program = FontProgram::parse(data.bytes()).map_err(|_| UnsupportedFont::UnusableProgram)?;

    let is_type0 = doc
        .resolve(descendant.get(b"Subtype").unwrap_or(&Object::Null))
        .as_name()
        .is_some_and(|n| n.as_bytes() == b"CIDFontType0");

    let cid_to_gid = if is_type0 {
        // §9.7.4.2: CIDs index the charset when the Top DICT uses
        // CIDFont operators; otherwise they ARE the GIDs.
        if program.is_cid_cff() {
            CidToGid::CffCharset
        } else {
            CidToGid::Identity
        }
    } else {
        // CIDFontType2. `/CIDToGIDMap` may be an indirect reference to
        // either a name or a stream (§9.7 gotchas), and is only legal
        // here.
        match doc.resolve(descendant.get(b"CIDToGIDMap").unwrap_or(&Object::Null)) {
            // `doc.slice` (decision 018 §4) — a session view has two
            // buffers, so a span cannot be applied to one of them alone.
            Object::Stream(s) => doc
                .slice(s.data_span)
                .and_then(|raw| pdfcer_core::filters::decode_stream(&s.dict, raw).ok())
                .map_or(CidToGid::Identity, CidToGid::Stream),
            _ => CidToGid::Identity,
        }
    };

    let default_width = doc
        .resolve(descendant.get(b"DW").unwrap_or(&Object::Null))
        .as_number()
        .unwrap_or(f64::from(DEFAULT_CID_WIDTH)) as f32;
    let widths = parse_w_array(doc, descendant);

    Ok(LoadedFont {
        base_font,
        data,
        // A composite font that reached here HAS an embedded program
        // (the no-program case is the `CompositeNotEmbedded` hard skip
        // above). Operator-supplied composite substitution is decision
        // 012's named non-goal (FF2, R65) — `env` is deliberately not
        // consulted here.
        source: GlyphSource::Embedded,
        kind: FontKind::Composite(CompositeFont {
            cid_to_gid,
            default_width,
            widths,
        }),
    })
}

/// `/W` (§9.7.4.3) — a two-shape state machine:
///
/// ```text
/// c        [ w1 w2 … wn ]     widths for n consecutive CIDs from c
/// cfirst   clast   w          one width for the whole cfirst..clast range
/// ```
///
/// Shape is decided by the type of the element AFTER `c`: an array
/// means shape 1, an integer means shape 2.
fn parse_w_array(doc: &DocumentView<'_>, descendant: &Dict) -> Vec<(u32, u32, f32)> {
    let Some(items) = doc
        .resolve(descendant.get(b"W").unwrap_or(&Object::Null))
        .as_array()
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < items.len() {
        let Some(first) = items
            .get(i)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
            .and_then(|v| u32::try_from(v).ok())
        else {
            break;
        };
        let Some(next) = items.get(i + 1).map(|o| doc.resolve(o)) else {
            break;
        };
        if let Some(list) = next.as_array() {
            for (k, w) in list.iter().enumerate() {
                if let Some(w) = doc.resolve(w).as_number()
                    && let Ok(k) = u32::try_from(k)
                    && let Some(cid) = first.checked_add(k)
                {
                    out.push((cid, cid, w as f32));
                }
            }
            i += 2;
        } else if let Some(last) = next.as_int().and_then(|v| u32::try_from(v).ok()) {
            let w = items
                .get(i + 2)
                .map(|o| doc.resolve(o))
                .and_then(Object::as_number)
                .unwrap_or(0.0) as f32;
            out.push((first, last.max(first), w));
            i += 3;
        } else {
            break;
        }
    }
    out
}

/// Pick a substitute face for a font with no embedded program, and
/// report which trust level it came from (decision 004 §4.2, decision
/// 012 §3).
///
/// Precedence, first match wins:
///
/// 1. `env.named(base_font)` — an operator-supplied face registered
///    under the EXACT `/BaseFont` string ⇒ [`GlyphSource::Supplied`].
/// 2. `env.named(strip_subset_tag(base_font))` — the same, but after
///    dropping a `ABCDEF+` subset tag (§9.6.4), so a supplied `Calibri`
///    matches a document's `ABCDEF+Calibri` ⇒ [`GlyphSource::Supplied`].
///    (Style suffixes like `Calibri,Bold` are matched by the shell
///    registering that spelling as its own key; the render side does
///    not decompose them — decision 012 M1.)
/// 3. The bundled standard-14 name table, then the `FontDescriptor`
///    classification (§9.8.1 Table 123) ⇒ [`GlyphSource::Bundled`].
///
/// Note the strip-tag retry is step 2 and is NEW in decision 012: the
/// pre-012 code tried only the verbatim string, so a supplied `Calibri`
/// silently missed `ABCDEF+Calibri` and fell through to bundled
/// Helvetica.
fn substitute_face(
    env: &FontEnvironment,
    base_font: &str,
    flags: u32,
    italic_angle: f64,
    descriptor: Option<&Dict>,
    doc: &DocumentView<'_>,
) -> Result<(FontData, GlyphSource), UnsupportedFont> {
    // (1) exact, then (2) subset-tag-stripped — an operator's own face.
    if let Some(data) = env.named(base_font) {
        return Ok((data.clone(), GlyphSource::Supplied));
    }
    let stripped = select::strip_subset_tag(base_font);
    if stripped != base_font
        && let Some(data) = env.named(stripped)
    {
        return Ok((data.clone(), GlyphSource::Supplied));
    }
    // (3) the bundled Base-14 floor — pdfcer's own plausible shape.
    let bold = flags & FLAG_FORCE_BOLD != 0
        || descriptor
            .and_then(|d| doc.resolve(d.get(b"StemV")?).as_number())
            .is_some_and(|v| v >= BOLD_STEM_V)
        || base_font.to_ascii_lowercase().contains("bold");
    let key = select::by_name(base_font)
        .unwrap_or_else(|| select::by_descriptor(flags, italic_angle, bold));
    env.fallback(key)
        .cloned()
        .map(|data| (data, GlyphSource::Bundled))
        .ok_or(UnsupportedFont::UnusableProgram)
}

/// Build the code → glyph-name table (§9.6.6.1, Table 114).
///
/// `embedded` selects the implicit-base arm of Table 114's decision
/// table; `flags`/`std14` supply the symbolic classification and the
/// standard-14 built-in encoding.
///
/// `None` at a code means "this encoding has no name here" — the caller
/// then falls through to the font program's own built-in encoding,
/// which is precisely what the implicit-base rule asks for.
fn encoding_table(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    embedded: bool,
    flags: u32,
    std14: Option<Std14>,
) -> Vec<Option<String>> {
    let encoding = doc.resolve(font_dict.get(b"Encoding").unwrap_or(&Object::Null));

    // Step 1: the base table.
    let base: Option<BaseEncoding> = match encoding {
        Object::Name(n) => predefined_encoding(n.as_bytes()),
        Object::Dict(d) => match d.get(b"BaseEncoding").map(|o| doc.resolve(o)) {
            Some(Object::Name(n)) => predefined_encoding(n.as_bytes()),
            _ => implicit_base(embedded, flags, std14),
        },
        _ => implicit_base(embedded, flags, std14),
    };

    let mut table: Vec<Option<String>> = (0..256u16)
        .map(|c| {
            let code = u8::try_from(c).unwrap_or(0);
            base.and_then(|b| coredata::encoding_glyph_name(b, code))
                .map(str::to_owned)
        })
        .collect();

    // Step 2: `/Differences` over the base (§9.6.6.1's verbatim parse:
    // an integer sets the current code, each following name assigns and
    // increments). A leading name with no integer is malformed — the
    // RAG says diagnose rather than guess a start of 0, and skipping is
    // that diagnosis here (the interpreter has no handle on this call).
    if let Object::Dict(d) = encoding
        && let Some(diffs) = d.get(b"Differences").map(|o| doc.resolve(o))
        && let Some(items) = diffs.as_array()
    {
        let mut cur: Option<usize> = None;
        for item in items {
            match doc.resolve(item) {
                Object::Integer(v) => cur = usize::try_from(*v).ok(),
                Object::Real(v) => cur = usize::try_from(*v as i64).ok(),
                Object::Name(n) => {
                    if let Some(code) = cur {
                        // Codes outside 0–255 are meaningless for a
                        // simple font: ignore, keep counting.
                        if let Some(slot) = table.get_mut(code) {
                            *slot = Some(String::from_utf8_lossy(n.as_bytes()).into_owned());
                        }
                        cur = code.checked_add(1);
                    }
                }
                _ => {}
            }
        }
    }
    table
}

/// Table 114's `BaseEncoding` values, plus the `StandardEncoding`
/// recovery.
///
/// §9.6.6.1 says conforming readers "shall NOT have a predefined
/// encoding named `StandardEncoding`", so it is not a legal value — but
/// real producers write it, and reading it as the Annex D.2 STD column
/// is the only sensible recovery. `MacExpertEncoding` is legal and its
/// table is not yet in the corpus; `None` sends those fonts to the
/// program's built-in encoding.
fn predefined_encoding(name: &[u8]) -> Option<BaseEncoding> {
    match name {
        b"WinAnsiEncoding" => Some(BaseEncoding::WinAnsi),
        b"MacRomanEncoding" => Some(BaseEncoding::MacRoman),
        b"StandardEncoding" => Some(BaseEncoding::Standard),
        _ => None,
    }
}

/// Table 114's implicit-base decision table:
///
/// | Program embedded? | Symbolic flag | Implicit base |
/// |---|---|---|
/// | yes | either | the program's built-in encoding (`None` here) |
/// | no | nonsymbolic | `StandardEncoding` |
/// | no | symbolic | the font's built-in encoding |
///
/// For a non-embedded standard-14 font the "built-in encoding" is
/// knowable without a program (`Symbol` and `ZapfDingbats` have
/// documented tables in Annex D.5/D.6), which is what
/// `std14_builtin_encoding` supplies.
///
/// The symbolic test uses `Symbolic` alone rather than
/// `Symbolic && !Nonsymbolic`: the two flags "shall not both be set or
/// both be clear" but frequently are, and this is the RAG's recommended
/// tiebreak (prefer symbolic when both set → the built-in encoding;
/// prefer nonsymbolic when both clear → Standard, which at least yields
/// Latin text).
fn implicit_base(embedded: bool, flags: u32, std14: Option<Std14>) -> Option<BaseEncoding> {
    if embedded {
        return None;
    }
    if let Some(f) = std14 {
        return Some(coredata::std14_builtin_encoding(f));
    }
    if flags & FLAG_SYMBOLIC != 0 {
        None
    } else {
        Some(BaseEncoding::Standard)
    }
}

/// Run every code through §9.6.6's glyph ladder ONCE (module docs).
///
/// Order, per §9.6.6.2 and §9.6.6.4:
///
/// 1. With a glyph name, on an **sfnt** program — Branch A:
///    name → Unicode (AGL) → `(3, 1)` cmap; else name → Mac OS Roman
///    code → `(1, 0)` cmap; else the `post` table.
/// 2. With a glyph name, on a **name-keyed** program (bare CFF,
///    Type 1, and every bundled substitute) — the name directly.
/// 3. Without a name, or when 1/2 failed — Branch B / the program's
///    own built-in encoding, keyed by the raw code.
/// 4. Nothing → `None`, painted as `.notdef` and counted (§9.6.6.2:
///    "if an encoding maps to a character name that does not exist in
///    the Type 1 font program, the `.notdef` glyph shall be
///    substituted").
fn resolve_gids(program: &FontProgram<'_>, names: &[Option<String>]) -> [Option<u32>; 256] {
    // Reverse Mac OS Roman table for Branch A's second chain, built
    // once per font rather than once per code.
    let mac: HashMap<&'static str, u8> = (0..=255u8)
        .filter_map(|c| coredata::encoding_glyph_name(BaseEncoding::MacRoman, c).map(|n| (n, c)))
        .collect();

    let num_glyphs = program.num_glyphs();
    let mut out = [None; 256];
    for (code, slot) in out.iter_mut().enumerate() {
        let code8 = u8::try_from(code).unwrap_or(0);
        let name = names.get(code).and_then(Option::as_deref);
        let gid = name
            .and_then(|n| {
                coredata::glyph_name_to_unicode(n)
                    .and_then(|ch| program.glyph_for_char(ch))
                    .or_else(|| {
                        mac.get(n)
                            .copied()
                            .and_then(|mc| program.glyph_for_mac_code(mc))
                    })
                    .or_else(|| program.glyph_for_name(n))
            })
            .or_else(|| program.glyph_for_builtin_code(code8))
            // GID 0 IS `.notdef` — treat "resolved to notdef" the same
            // as "did not resolve" so the diagnostic is honest.
            .filter(|&g| g != 0 && g < num_glyphs);
        *slot = gid;
    }
    out
}

/// Build the code → width table (§9.2.4 / Table 111 / the text
/// pipeline's Stage 2).
///
/// Ladder: `/Widths[code − /FirstChar]` → the AFM tables for a
/// standard-14 font with no `/Widths` (keyed by glyph NAME, hence the
/// encoding table and the program's built-in names) → `/MissingWidth`
/// (Table 122 default 0).
fn width_table(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    names: &[Option<String>],
    program: &FontProgram<'_>,
    std14: Option<Std14>,
    missing_width: f32,
) -> [f32; 256] {
    let first_char = doc
        .resolve(font_dict.get(b"FirstChar").unwrap_or(&Object::Null))
        .as_int()
        .unwrap_or(0);
    let widths = doc
        .resolve(font_dict.get(b"Widths").unwrap_or(&Object::Null))
        .as_array();

    let mut out = [missing_width; 256];
    for (code, slot) in out.iter_mut().enumerate() {
        if let Some(list) = widths {
            let idx = i64::try_from(code).unwrap_or(i64::MAX) - first_char;
            if let Ok(idx) = usize::try_from(idx)
                && let Some(w) = list
                    .get(idx)
                    .map(|o| doc.resolve(o))
                    .and_then(Object::as_number)
            {
                *slot = w as f32;
                continue;
            }
        }
        // No `/Widths` coverage. A standard-14 font legitimately omits
        // the array entirely (§9.6.2.2), and its metrics come from the
        // AFM tables — keyed by glyph name, so use the PDF encoding's
        // name when there is one and the program's built-in name
        // otherwise.
        if let Some(f) = std14 {
            let code8 = u8::try_from(code).unwrap_or(0);
            let name = names
                .get(code)
                .and_then(Option::as_deref)
                .or_else(|| program.builtin_glyph_name(code8));
            if let Some(w) = name.and_then(|n| coredata::std14_width(f, n)) {
                *slot = f32::from(w);
            }
        }
    }
    out
}

/// The standard-14 face whose AFM metrics stand in for a substitute slot.
///
/// The two enumerations describe the same fourteen faces from opposite
/// ends — [`FallbackKey`] names the slot pdfcer draws GLYPHS from,
/// [`Std14`] names the table pdfcer reads WIDTHS from — so this is a
/// total, information-preserving mapping rather than a guess.
///
/// It exists because those two answers were previously reached
/// independently: a non-embedded `/BaseFont /Arial` selected the `Sans`
/// face for shapes and found no std-14 match for metrics, so it drew
/// Helvetica glyphs and advanced them by zero.
const fn std14_for_fallback(key: FallbackKey) -> Std14 {
    match key {
        FallbackKey::Sans => Std14::Helvetica,
        FallbackKey::SansBold => Std14::HelveticaBold,
        FallbackKey::SansItalic => Std14::HelveticaOblique,
        FallbackKey::SansBoldItalic => Std14::HelveticaBoldOblique,
        FallbackKey::Serif => Std14::TimesRoman,
        FallbackKey::SerifBold => Std14::TimesBold,
        FallbackKey::SerifItalic => Std14::TimesItalic,
        FallbackKey::SerifBoldItalic => Std14::TimesBoldItalic,
        FallbackKey::Fixed => Std14::Courier,
        FallbackKey::FixedBold => Std14::CourierBold,
        FallbackKey::FixedItalic => Std14::CourierOblique,
        FallbackKey::FixedBoldItalic => Std14::CourierBoldOblique,
        FallbackKey::Symbol => Std14::Symbol,
        FallbackKey::Dingbats => Std14::ZapfDingbats,
    }
}

/// Decode the embedded font program from a `FontDescriptor` (§9.8,
/// Table 122; §9.9 Table 126 for the three stream keys).
///
/// Order is `FontFile2` (sfnt) → `FontFile3` (bare CFF / OpenType) →
/// `FontFile` (bare Type 1), which is preference order, not spec order:
/// at most one is present in a conforming descriptor, and probing the
/// most common first costs nothing.
fn embedded_program(doc: &DocumentView<'_>, descriptor: &Dict) -> Option<Vec<u8>> {
    for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
        let Some(obj) = descriptor.get(key).map(|o| doc.resolve(o)) else {
            continue;
        };
        if let Object::Stream(stream) = obj
            // `doc.slice` (decision 018 §4): an embedded font program a
            // future Pass stages this session must be readable too.
            && let Some(raw) = doc.slice(stream.data_span)
            && let Ok(bytes) = pdfcer_core::filters::decode_stream(&stream.dict, raw)
            && !bytes.is_empty()
        {
            return Some(bytes);
        }
    }
    None
}

/// A resolved name-valued entry, as a `String`.
fn name_of(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<String> {
    let n = doc.resolve(dict.get(key)?).as_name()?;
    Some(String::from_utf8_lossy(n.as_bytes()).into_owned())
}

/// A resolved dictionary-valued entry.
fn dict_of<'a>(doc: &'a DocumentView<'a>, dict: &'a Dict, key: &[u8]) -> Option<&'a Dict> {
    doc.resolve(dict.get(key)?).as_dict()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn text_state_defaults_match_table_105() {
        let ts = TextState::default();
        assert_eq!(ts.char_spacing, 0.0);
        assert_eq!(ts.word_spacing, 0.0);
        // Tz's operand is a PERCENTAGE; Th is the ratio.
        assert_eq!(ts.horizontal_scale, 1.0);
        assert_eq!(ts.leading, 0.0);
        assert_eq!(ts.render_mode, 0);
        assert_eq!(ts.rise, 0.0);
        assert!(ts.font.is_none(), "Tf has NO initial value (§9.3)");
    }

    #[test]
    fn advance_formula_applies_th_to_every_term() {
        // §9.3.4 / §9.4.4: Th multiplies the glyph displacement, the TJ
        // adjustment, Tc AND Tw — not just the glyph term.
        let ts = TextState {
            char_spacing: 2.0,
            word_spacing: 5.0,
            horizontal_scale: 0.5,
            font_size: 10.0,
            ..TextState::default()
        };
        // w0 = 0.5 text space, no TJ, word spacing applies:
        //   ((0.5 − 0)·10 + 2 + 5) · 0.5 = 6.0
        assert!((ts.advance_for(0.5, 0.0, true) - 6.0).abs() < 1e-6);
        // Without word spacing: ((0.5)·10 + 2) · 0.5 = 3.5
        assert!((ts.advance_for(0.5, 0.0, false) - 3.5).abs() < 1e-6);
    }

    #[test]
    fn tj_adjustment_is_subtracted_and_in_thousandths() {
        let ts = TextState {
            font_size: 12.0,
            horizontal_scale: 1.0,
            char_spacing: 3.0, // must NOT appear: no glyph was painted
            ..TextState::default()
        };
        // A POSITIVE TJ number moves the next glyph LEFT.
        assert!((ts.adjustment(1000.0) + 12.0).abs() < 1e-6);
        assert!((ts.adjustment(-500.0) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn td_concatenates_onto_tlm_not_tm() {
        // The §9.4.4 named bug: after an intra-line advance, `Td` must
        // offset from the LINE origin, not from the pen.
        let mut t = TextObject::new();
        t.next_line_offset(10.0, 0.0);
        t.advance(37.0, 0.0); // a glyph was painted
        t.next_line_offset(0.0, -14.0);
        assert!((t.tm.tx - 10.0).abs() < 1e-6, "tx was {}", t.tm.tx);
        assert!((t.tm.ty + 14.0).abs() < 1e-6, "ty was {}", t.tm.ty);
    }

    #[test]
    fn tm_replaces_rather_than_concatenates() {
        let mut t = TextObject::new();
        t.next_line_offset(50.0, 50.0);
        t.set_matrix(Transform::from_row(1.0, 0.0, 0.0, 1.0, 5.0, 5.0));
        assert!((t.tm.tx - 5.0).abs() < 1e-6);
        assert!((t.tlm.tx - 5.0).abs() < 1e-6);
    }

    #[test]
    fn render_modes_table_106() {
        let mode = |m| TextState {
            render_mode: m,
            ..TextState::default()
        };
        assert!(mode(0).fills() && !mode(0).strokes());
        assert!(!mode(1).fills() && mode(1).strokes());
        assert!(mode(2).fills() && mode(2).strokes());
        // Mode 3 is invisible — the OCR text-layer mode.
        assert!(!mode(3).fills() && !mode(3).strokes());
        // Mode 7 adds to the clip path and paints nothing.
        assert!(!mode(7).fills() && !mode(7).strokes());
    }

    #[test]
    fn identity_h_codes_are_two_byte_be_and_never_word_spaced() {
        let font = LoadedFont {
            base_font: "X".into(),
            data: FontData::new(Vec::new()),
            source: GlyphSource::Embedded,
            kind: FontKind::Composite(CompositeFont {
                cid_to_gid: CidToGid::Identity,
                default_width: 1000.0,
                widths: Vec::new(),
            }),
        };
        // 0x0020 contains the byte 32 but is NOT a single-byte code 32
        // (§9.3.3) — word spacing must stay inert.
        let codes = font.codes(&[0x00, 0x20, 0x01, 0xF4]);
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0].value, 0x0020);
        assert!(!codes[0].word_spacing_applies);
        assert_eq!(codes[1].value, 500);
        // Odd trailing byte: consumed as a high byte (documented
        // choice, not spec text).
        assert_eq!(font.codes(&[0xAB]).len(), 1);
        assert_eq!(font.codes(&[0xAB])[0].value, 0xAB00);
    }

    #[test]
    fn simple_font_codes_are_one_byte_and_32_word_spaces() {
        let font = LoadedFont {
            base_font: "X".into(),
            data: FontData::new(Vec::new()),
            source: GlyphSource::Embedded,
            kind: FontKind::Simple(Box::new(SimpleFont {
                gids: [None; 256],
                widths: [0.0; 256],
            })),
        };
        let codes = font.codes(b"A B");
        assert_eq!(codes.len(), 3);
        assert!(!codes[0].word_spacing_applies);
        assert!(codes[1].word_spacing_applies);
    }

    #[test]
    fn cid_widths_prefer_ranges_then_dw() {
        let font = LoadedFont {
            base_font: "X".into(),
            data: FontData::new(Vec::new()),
            source: GlyphSource::Embedded,
            kind: FontKind::Composite(CompositeFont {
                cid_to_gid: CidToGid::Identity,
                default_width: 1000.0,
                // §9.7.4.3 EXAMPLE 1.
                widths: vec![(120, 120, 400.0), (121, 121, 325.0), (7080, 8032, 1000.0)],
            }),
        };
        assert_eq!(font.width(120), 400.0);
        assert_eq!(font.width(121), 325.0);
        assert_eq!(font.width(7500), 1000.0);
        assert_eq!(font.width(3), 1000.0); // DW
    }

    #[test]
    fn cid_to_gid_stream_is_big_endian_and_bounds_checked() {
        let font = LoadedFont {
            base_font: "X".into(),
            data: FontData::new(Vec::new()),
            source: GlyphSource::Embedded,
            kind: FontKind::Composite(CompositeFont {
                // CID 0 -> 0x0000, CID 1 -> 0x0102, CID 2 -> 0x00FF
                cid_to_gid: CidToGid::Stream(vec![0x00, 0x00, 0x01, 0x02, 0x00, 0xFF]),
                default_width: 1000.0,
                widths: Vec::new(),
            }),
        };
        assert_eq!(font.gid(1, None), Some(0x0102));
        assert_eq!(font.gid(2, None), Some(0x00FF));
        // Past the end of a subset map: no glyph, not a panic.
        assert_eq!(font.gid(9999, None), None);
    }
}
